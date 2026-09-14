//! Experimental, non-authoritative responsibility-family reinspection contract.
//!
//! The input states are externally declared bases. Comparing them does not
//! accept a family, verify a claim, or execute a test.

use reviewgraphen_core::{ContentHash, StableId, canonical_json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub mod decision;
pub mod discovery;
pub mod search;

pub const STATE_SCHEMA: &str = "reviewgraphen.benchmark.responsibility_family_state.v1";
pub const PLAN_SCHEMA: &str = "reviewgraphen.benchmark.responsibility_reinspection_plan.v1";
pub const PROPERTY: &str = "test.shared_conformance";

const MAX_MEMBERS: usize = 4096;
const MAX_TEXT_BYTES: usize = 4096;
const MAX_CONSTRAINTS: usize = 64;
const MAX_UNKNOWNS: usize = 64;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContractBinding {
    pub id: String,
    pub hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractorBinding {
    pub id: String,
    pub version: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemberBinding {
    pub member_id: StableId,
    pub path: String,
    pub symbol: String,
    pub anchor: ContentHash,
    pub purpose_constraints: BTreeSet<String>,
    pub source_ids: Vec<StableId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResponsibilityFamilyState {
    pub schema: String,
    pub family_id: StableId,
    pub snapshot_id: StableId,
    pub decision_basis_id: StableId,
    pub common_contract: ContractBinding,
    pub extractor: ExtractorBinding,
    pub members: Vec<MemberBinding>,
    pub unknowns: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityBoundary {
    pub classification: String,
    pub accepted: bool,
    pub verified: bool,
    pub human_accepted: bool,
    pub sign_off: bool,
}

impl AuthorityBoundary {
    #[must_use]
    pub fn non_authority() -> Self {
        Self {
            classification: "non_authority".to_owned(),
            accepted: false,
            verified: false,
            human_accepted: false,
            sign_off: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReinspectionReason {
    CommonContractChanged,
    DecisionBasisChanged,
    ExtractorChanged,
    MemberAdded,
    MemberAnchorChanged,
    MemberRemoved,
    PurposeConstraintsChanged,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Denominator {
    pub before: u64,
    pub after: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlanInputBinding {
    pub before_state_hash: ContentHash,
    pub after_state_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReinspectionObligation {
    pub id: StableId,
    pub member_id: StableId,
    pub property: String,
    pub reasons: BTreeSet<ReinspectionReason>,
    pub before_anchor: Option<ContentHash>,
    pub after_anchor: Option<ContentHash>,
    pub source_ids: Vec<StableId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReinspectionPlan {
    pub schema: String,
    pub family_id: StableId,
    pub before_snapshot_id: StableId,
    pub after_snapshot_id: StableId,
    pub input: PlanInputBinding,
    pub authority: AuthorityBoundary,
    pub denominator: Denominator,
    pub obligations: Vec<ReinspectionObligation>,
    pub preserved_member_ids: Vec<StableId>,
    pub unknowns: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ResponsibilityFamilyError {
    #[error("invalid responsibility-family state: {0}")]
    InvalidState(&'static str),
    #[error("responsibility-family IDs differ")]
    FamilyMismatch,
    #[error("responsibility-family count overflow")]
    CountOverflow,
    #[error("reinspection plan is stale or tampered")]
    PlanMismatch,
    #[error("responsibility-family canonicalization failed: {0}")]
    Canonical(#[from] reviewgraphen_core::DomainError),
}

pub type Result<T> = std::result::Result<T, ResponsibilityFamilyError>;

/// Build a deterministic, non-authoritative reinspection plan.
pub fn plan(
    before: &ResponsibilityFamilyState,
    after: &ResponsibilityFamilyState,
) -> Result<ReinspectionPlan> {
    validate_state(before)?;
    validate_state(after)?;
    if before.family_id != after.family_id {
        return Err(ResponsibilityFamilyError::FamilyMismatch);
    }

    let before_hash = ContentHash::sha256(&canonical_json(before)?);
    let after_hash = ContentHash::sha256(&canonical_json(after)?);
    let before_members = member_map(before);
    let after_members = member_map(after);
    let all_member_ids = before_members
        .keys()
        .chain(after_members.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let global_reasons = global_reasons(before, after);
    let mut obligations = Vec::new();
    let mut preserved_member_ids = Vec::new();

    for member_id in all_member_ids {
        let old = before_members.get(&member_id).copied();
        let new = after_members.get(&member_id).copied();
        let mut reasons = global_reasons.clone();
        match (old, new) {
            (None, Some(_)) => {
                reasons.insert(ReinspectionReason::MemberAdded);
            }
            (Some(_), None) => {
                reasons.insert(ReinspectionReason::MemberRemoved);
            }
            (Some(old), Some(new)) => {
                if old.anchor != new.anchor {
                    reasons.insert(ReinspectionReason::MemberAnchorChanged);
                }
                if old.purpose_constraints != new.purpose_constraints {
                    reasons.insert(ReinspectionReason::PurposeConstraintsChanged);
                }
            }
            (None, None) => unreachable!("member came from the union"),
        }
        if reasons.is_empty() {
            preserved_member_ids.push(member_id);
            continue;
        }
        let mut source_ids = old
            .into_iter()
            .flat_map(|member| member.source_ids.iter().cloned())
            .chain(
                new.into_iter()
                    .flat_map(|member| member.source_ids.iter().cloned()),
            )
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        source_ids.sort();
        let before_anchor = old.map(|member| member.anchor.clone());
        let after_anchor = new.map(|member| member.anchor.clone());
        let id = obligation_id(
            &before.family_id,
            &member_id,
            &reasons,
            before_anchor.as_ref(),
            after_anchor.as_ref(),
            &before_hash,
            &after_hash,
        )?;
        obligations.push(ReinspectionObligation {
            id,
            member_id,
            property: PROPERTY.to_owned(),
            reasons,
            before_anchor,
            after_anchor,
            source_ids,
        });
    }
    obligations.sort_by(|left, right| left.id.cmp(&right.id));
    preserved_member_ids.sort();
    let unknowns = before
        .unknowns
        .iter()
        .chain(&after.unknowns)
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    Ok(ReinspectionPlan {
        schema: PLAN_SCHEMA.to_owned(),
        family_id: before.family_id.clone(),
        before_snapshot_id: before.snapshot_id.clone(),
        after_snapshot_id: after.snapshot_id.clone(),
        input: PlanInputBinding {
            before_state_hash: before_hash,
            after_state_hash: after_hash,
        },
        authority: AuthorityBoundary::non_authority(),
        denominator: Denominator {
            before: u64::try_from(before.members.len())
                .map_err(|_| ResponsibilityFamilyError::CountOverflow)?,
            after: u64::try_from(after.members.len())
                .map_err(|_| ResponsibilityFamilyError::CountOverflow)?,
        },
        obligations,
        preserved_member_ids,
        unknowns,
    })
}

/// Recompute the complete plan and reject stale or changed fields.
pub fn validate_plan(
    before: &ResponsibilityFamilyState,
    after: &ResponsibilityFamilyState,
    candidate: &ReinspectionPlan,
) -> Result<()> {
    if plan(before, after)? == *candidate {
        Ok(())
    } else {
        Err(ResponsibilityFamilyError::PlanMismatch)
    }
}

fn validate_state(state: &ResponsibilityFamilyState) -> Result<()> {
    if state.schema != STATE_SCHEMA
        || state.family_id.kind() != "responsibility-family"
        || state.snapshot_id.kind() != "snapshot"
        || state.decision_basis_id.kind() != "decision"
        || state.members.is_empty()
        || state.members.len() > MAX_MEMBERS
        || !text(&state.common_contract.id)
        || !text(&state.extractor.id)
        || !text(&state.extractor.version)
        || state.unknowns.len() > MAX_UNKNOWNS
        || state.unknowns.iter().any(|value| !text(value))
        || !strictly_sorted_unique(&state.unknowns)
    {
        return Err(ResponsibilityFamilyError::InvalidState(
            "invalid header or bounds",
        ));
    }
    if !state
        .members
        .windows(2)
        .all(|pair| pair[0].member_id < pair[1].member_id)
    {
        return Err(ResponsibilityFamilyError::InvalidState(
            "members must be strictly ordered by member_id",
        ));
    }
    for member in &state.members {
        if member.member_id.kind() != "responsibility-member"
            || !safe_relative_path(&member.path)
            || !text(&member.symbol)
            || member.purpose_constraints.len() > MAX_CONSTRAINTS
            || member.purpose_constraints.iter().any(|value| !text(value))
            || member.source_ids.is_empty()
            || !member.source_ids.windows(2).all(|pair| pair[0] < pair[1])
        {
            return Err(ResponsibilityFamilyError::InvalidState("invalid member"));
        }
    }
    Ok(())
}

fn member_map(state: &ResponsibilityFamilyState) -> BTreeMap<StableId, &MemberBinding> {
    state
        .members
        .iter()
        .map(|member| (member.member_id.clone(), member))
        .collect()
}

fn global_reasons(
    before: &ResponsibilityFamilyState,
    after: &ResponsibilityFamilyState,
) -> BTreeSet<ReinspectionReason> {
    let mut reasons = BTreeSet::new();
    if before.common_contract != after.common_contract {
        reasons.insert(ReinspectionReason::CommonContractChanged);
    }
    if before.decision_basis_id != after.decision_basis_id {
        reasons.insert(ReinspectionReason::DecisionBasisChanged);
    }
    if before.extractor != after.extractor {
        reasons.insert(ReinspectionReason::ExtractorChanged);
    }
    reasons
}

#[allow(clippy::too_many_arguments)]
fn obligation_id(
    family_id: &StableId,
    member_id: &StableId,
    reasons: &BTreeSet<ReinspectionReason>,
    before_anchor: Option<&ContentHash>,
    after_anchor: Option<&ContentHash>,
    before_state_hash: &ContentHash,
    after_state_hash: &ContentHash,
) -> Result<StableId> {
    let bindings = BTreeMap::from([
        ("after_anchor".to_owned(), optional_hash(after_anchor)),
        (
            "after_state_hash".to_owned(),
            Value::String(after_state_hash.to_string()),
        ),
        ("before_anchor".to_owned(), optional_hash(before_anchor)),
        (
            "before_state_hash".to_owned(),
            Value::String(before_state_hash.to_string()),
        ),
        ("family_id".to_owned(), Value::String(family_id.to_string())),
        ("member_id".to_owned(), Value::String(member_id.to_string())),
        ("property".to_owned(), Value::String(PROPERTY.to_owned())),
        (
            "reasons".to_owned(),
            serde_json::to_value(reasons).expect("closed reason enum serializes"),
        ),
    ]);
    Ok(StableId::derived("responsibility-reinspection", &bindings)?)
}

fn optional_hash(hash: Option<&ContentHash>) -> Value {
    hash.map_or(Value::Null, |value| Value::String(value.to_string()))
}

fn text(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_TEXT_BYTES && !value.chars().any(char::is_control)
}

fn safe_relative_path(path: &str) -> bool {
    text(path)
        && !path.starts_with('/')
        && !path.contains('\\')
        && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
}

fn strictly_sorted_unique(values: &[String]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

#[cfg(test)]
mod tests;
