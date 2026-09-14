use super::*;
use reviewgraphen_core::{ContentHash, StableId};
use std::collections::BTreeSet;

const PLAN_SCHEMA_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/responsibility-family-v1/responsibility-reinspection-plan-v1.schema.json"
));

fn id(value: &str) -> StableId {
    StableId::parse(value).unwrap()
}

fn hash(byte: char) -> ContentHash {
    ContentHash::parse(format!("sha256:{}", byte.to_string().repeat(8))).unwrap()
}

fn member(name: &str, anchor: char) -> MemberBinding {
    MemberBinding {
        member_id: id(&format!("responsibility-member:{name}")),
        path: format!("src/{name}.rs"),
        symbol: format!("validate_{name}"),
        anchor: hash(anchor),
        purpose_constraints: BTreeSet::from(["preserve_typed_error".to_owned()]),
        source_ids: vec![id(&format!("artifact:{name}"))],
    }
}

fn state(snapshot: &str) -> ResponsibilityFamilyState {
    ResponsibilityFamilyState {
        schema: STATE_SCHEMA.to_owned(),
        family_id: id("responsibility-family:path-policy"),
        snapshot_id: id(snapshot),
        decision_basis_id: id("decision:path-policy-v1"),
        common_contract: ContractBinding {
            id: "snapshot-relative-path@1".to_owned(),
            hash: hash('c'),
        },
        extractor: ExtractorBinding {
            id: "syn-item-fn-tokens".to_owned(),
            version: "1".to_owned(),
        },
        members: vec![member("alpha", 'a'), member("beta", 'b')],
        unknowns: vec!["token equality is not semantic equivalence".to_owned()],
    }
}

#[test]
fn snapshot_change_alone_preserves_all_members() {
    let plan = plan(&state("snapshot:before"), &state("snapshot:after")).unwrap();
    assert_eq!(
        plan.denominator,
        Denominator {
            before: 2,
            after: 2
        }
    );
    assert!(plan.obligations.is_empty());
    assert_eq!(plan.preserved_member_ids.len(), 2);
    assert_eq!(plan.authority, AuthorityBoundary::non_authority());
}

#[test]
fn member_anchor_change_reopens_only_that_member() {
    let before = state("snapshot:before");
    let mut after = state("snapshot:after");
    after.members[1].anchor = hash('d');
    let plan = plan(&before, &after).unwrap();
    assert_eq!(plan.obligations.len(), 1);
    assert_eq!(
        plan.obligations[0].member_id,
        id("responsibility-member:beta")
    );
    assert_eq!(
        plan.obligations[0].reasons,
        BTreeSet::from([ReinspectionReason::MemberAnchorChanged])
    );
    validate_plan(&before, &after, &plan).unwrap();
}

#[test]
fn common_contract_change_reopens_every_current_member() {
    let before = state("snapshot:before");
    let mut after = state("snapshot:after");
    after.common_contract.hash = hash('d');
    let plan = plan(&before, &after).unwrap();
    assert_eq!(plan.obligations.len(), 2);
    assert!(plan.obligations.iter().all(|item| {
        item.reasons
            .contains(&ReinspectionReason::CommonContractChanged)
    }));
}

#[test]
fn purpose_constraint_change_reopens_only_that_member() {
    let before = state("snapshot:before");
    let mut after = state("snapshot:after");
    after.members[0]
        .purpose_constraints
        .insert("maximum_4096_bytes".to_owned());
    let plan = plan(&before, &after).unwrap();
    assert_eq!(plan.obligations.len(), 1);
    assert_eq!(
        plan.obligations[0].reasons,
        BTreeSet::from([ReinspectionReason::PurposeConstraintsChanged])
    );
}

#[test]
fn additions_and_removals_are_explicit() {
    let before = state("snapshot:before");
    let mut after = state("snapshot:after");
    after.members.remove(0);
    after.members.push(member("gamma", 'e'));
    after
        .members
        .sort_by(|left, right| left.member_id.cmp(&right.member_id));
    let plan = plan(&before, &after).unwrap();
    assert_eq!(
        plan.denominator,
        Denominator {
            before: 2,
            after: 2
        }
    );
    assert_eq!(plan.obligations.len(), 2);
    assert!(
        plan.obligations
            .iter()
            .any(|item| item.reasons.contains(&ReinspectionReason::MemberAdded))
    );
    assert!(
        plan.obligations
            .iter()
            .any(|item| item.reasons.contains(&ReinspectionReason::MemberRemoved))
    );
}

#[test]
fn global_changes_reopen_added_and_removed_members_too() {
    let before = state("snapshot:before");
    let mut after = state("snapshot:after");
    after.common_contract.hash = hash('d');
    after.members.remove(0);
    after.members.push(member("gamma", 'e'));
    after
        .members
        .sort_by(|left, right| left.member_id.cmp(&right.member_id));

    let plan = plan(&before, &after).unwrap();
    assert_eq!(plan.obligations.len(), 3);
    assert!(plan.obligations.iter().all(|item| {
        item.reasons
            .contains(&ReinspectionReason::CommonContractChanged)
    }));
    assert!(
        plan.obligations
            .iter()
            .any(|item| item.reasons.contains(&ReinspectionReason::MemberAdded))
    );
    assert!(
        plan.obligations
            .iter()
            .any(|item| item.reasons.contains(&ReinspectionReason::MemberRemoved))
    );
}

#[test]
fn decision_or_extractor_change_reopens_the_union() {
    let before = state("snapshot:before");
    let mut after = state("snapshot:after");
    after.decision_basis_id = id("decision:path-policy-v2");
    let decision = plan(&before, &after).unwrap();
    assert!(decision.obligations.iter().all(|item| {
        item.reasons
            .contains(&ReinspectionReason::DecisionBasisChanged)
    }));
    after.decision_basis_id = before.decision_basis_id.clone();
    after.extractor.version = "2".to_owned();
    let extractor = plan(&before, &after).unwrap();
    assert!(
        extractor
            .obligations
            .iter()
            .all(|item| item.reasons.contains(&ReinspectionReason::ExtractorChanged))
    );
}

#[test]
fn validation_rejects_duplicate_members_and_tampered_plan() {
    let before = state("snapshot:before");
    let mut invalid = state("snapshot:after");
    invalid.members.push(invalid.members[0].clone());
    assert!(plan(&before, &invalid).is_err());

    let mut invalid_snapshot = state("artifact:not-a-snapshot");
    invalid_snapshot.members[0].anchor = hash('d');
    assert!(plan(&before, &invalid_snapshot).is_err());

    let mut after = state("snapshot:after");
    after.members[0].anchor = hash('d');
    let mut planned = plan(&before, &after).unwrap();
    planned.obligations[0].property = "tampered".to_owned();
    assert!(validate_plan(&before, &after, &planned).is_err());
}

#[test]
fn plan_serialization_satisfies_the_closed_schema() {
    let before = state("snapshot:before");
    let mut after = state("snapshot:after");
    after.members[0].anchor = hash('d');
    let document = serde_json::to_value(plan(&before, &after).unwrap()).unwrap();
    let schema: serde_json::Value = serde_json::from_str(PLAN_SCHEMA_JSON).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    assert!(validator.is_valid(&document));

    let mut tampered = document;
    tampered["authority"]["undeclared_authority"] = serde_json::json!(true);
    assert!(!validator.is_valid(&tampered));
}

#[test]
fn identical_inputs_produce_identical_plan_bytes() {
    let before = state("snapshot:before");
    let mut after = state("snapshot:after");
    after.members[0].anchor = hash('d');
    assert_eq!(
        reviewgraphen_core::canonical_json(&plan(&before, &after).unwrap()).unwrap(),
        reviewgraphen_core::canonical_json(&plan(&before, &after).unwrap()).unwrap()
    );
}
