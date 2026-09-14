//! Experimental decision support for externally supplied family candidates.

use super::{
    AuthorityBoundary, ContractBinding, ExtractorBinding, MemberBinding, ResponsibilityFamilyError,
    ResponsibilityFamilyState, validate_state,
};
use reviewgraphen_core::{ContentHash, StableId, canonical_json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const CANDIDATE_SCHEMA: &str = "reviewgraphen.benchmark.responsibility_family_candidate.v1";
pub const UNIVERSE_SCHEMA: &str =
    "reviewgraphen.benchmark.responsibility_family_decision_universe.v1";
pub const ASSESSMENT_SCHEMA: &str = "reviewgraphen.benchmark.responsibility_family_assessment.v1";
pub const ASSESSMENT_INPUT_SCHEMA: &str =
    "reviewgraphen.benchmark.responsibility_family_assessment_input.v1";
pub const PROPOSAL_SCHEMA: &str =
    "reviewgraphen.benchmark.responsibility_family_decision_proposal.v1";

pub type DecisionAuthority = AuthorityBoundary;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResponsibilityFamilyCandidate {
    pub schema: String,
    pub candidate_id: StableId,
    pub proposed_family_id: StableId,
    pub snapshot_id: StableId,
    pub proposed_common_contract: ContractBinding,
    pub extractor: ExtractorBinding,
    pub members: Vec<MemberBinding>,
    pub unknowns: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionProperty {
    CommonChangeReason,
    CommonContractApplies,
    CompatibilityPreservation,
    PerformancePreservation,
    PurposeConstraintsPreserved,
    SeparationRationale,
    SharedConformanceFeasibility,
    SharedValidatorFeasibility,
    TypedErrorPreservation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionDisposition {
    Supports,
    Opposes,
    Inconclusive,
    NotApplicable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionOption {
    SharedValidator,
    SharedConformanceTest,
    IntentionalSeparation,
    Inconclusive,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionInputBinding {
    pub candidate_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionDenominator {
    pub members: u64,
    pub obligations: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionObligation {
    pub id: StableId,
    pub property: DecisionProperty,
    pub member_id: Option<StableId>,
    pub source_ids: Vec<StableId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionObligationUniverse {
    pub schema: String,
    pub candidate_id: StableId,
    pub proposed_family_id: StableId,
    pub snapshot_id: StableId,
    pub input: DecisionInputBinding,
    pub authority: DecisionAuthority,
    pub denominator: DecisionDenominator,
    pub obligations: Vec<DecisionObligation>,
    pub unknowns: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionResult {
    pub obligation_id: StableId,
    pub disposition: DecisionDisposition,
    pub source_ids: Vec<StableId>,
    pub evidence_ids: Vec<StableId>,
    pub verification_ids: Vec<StableId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionAssessment {
    pub schema: String,
    pub candidate_hash: ContentHash,
    pub obligation_universe_hash: ContentHash,
    pub results: Vec<DecisionResult>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionAssessmentEntry {
    pub property: DecisionProperty,
    pub member_ids: Vec<StableId>,
    pub disposition: DecisionDisposition,
    pub evidence_ids: Vec<StableId>,
    pub verification_ids: Vec<StableId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionAssessmentInput {
    pub schema: String,
    pub candidate_hash: ContentHash,
    pub entries: Vec<DecisionAssessmentEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionCounts {
    pub supports: u64,
    pub opposes: u64,
    pub unresolved: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionProposal {
    pub schema: String,
    pub candidate_id: StableId,
    pub proposed_family_id: StableId,
    pub snapshot_id: StableId,
    pub candidate_hash: ContentHash,
    pub obligation_universe_hash: ContentHash,
    pub assessment_hash: ContentHash,
    pub authority: DecisionAuthority,
    pub denominator: DecisionDenominator,
    pub counts: DecisionCounts,
    pub proposed_option: DecisionOption,
    pub unresolved_obligation_ids: Vec<StableId>,
    pub unknowns: Vec<String>,
}

#[derive(Debug, Error)]
pub enum DecisionError {
    #[error("invalid responsibility-family candidate: {0}")]
    InvalidCandidate(&'static str),
    #[error("invalid responsibility-family assessment: {0}")]
    InvalidAssessment(&'static str),
    #[error("decision proposal is stale or tampered")]
    ProposalMismatch,
    #[error("decision count overflow")]
    CountOverflow,
    #[error("canonicalization failed: {0}")]
    Canonical(#[from] reviewgraphen_core::DomainError),
}

pub type Result<T> = std::result::Result<T, DecisionError>;

pub fn obligations(
    candidate: &ResponsibilityFamilyCandidate,
) -> Result<DecisionObligationUniverse> {
    validate_candidate(candidate)?;
    let candidate_hash = ContentHash::sha256(&canonical_json(candidate)?);
    let all_sources = candidate
        .members
        .iter()
        .flat_map(|member| member.source_ids.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut items = Vec::new();
    for property in [
        DecisionProperty::CommonChangeReason,
        DecisionProperty::SeparationRationale,
        DecisionProperty::SharedConformanceFeasibility,
    ] {
        items.push(obligation(&candidate_hash, property, None, &all_sources)?);
    }
    for member in &candidate.members {
        for property in [
            DecisionProperty::CommonContractApplies,
            DecisionProperty::CompatibilityPreservation,
            DecisionProperty::PerformancePreservation,
            DecisionProperty::PurposeConstraintsPreserved,
            DecisionProperty::SharedValidatorFeasibility,
            DecisionProperty::TypedErrorPreservation,
        ] {
            items.push(obligation(
                &candidate_hash,
                property,
                Some(&member.member_id),
                &member.source_ids,
            )?);
        }
    }
    items.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(DecisionObligationUniverse {
        schema: UNIVERSE_SCHEMA.to_owned(),
        candidate_id: candidate.candidate_id.clone(),
        proposed_family_id: candidate.proposed_family_id.clone(),
        snapshot_id: candidate.snapshot_id.clone(),
        input: DecisionInputBinding { candidate_hash },
        authority: DecisionAuthority::non_authority(),
        denominator: DecisionDenominator {
            members: count(candidate.members.len())?,
            obligations: count(items.len())?,
        },
        obligations: items,
        unknowns: candidate.unknowns.clone(),
    })
}

pub fn propose(
    candidate: &ResponsibilityFamilyCandidate,
    assessment: &DecisionAssessment,
) -> Result<DecisionProposal> {
    let universe = obligations(candidate)?;
    validate_assessment(&universe, assessment)?;
    let result_by_id = assessment
        .results
        .iter()
        .map(|result| (&result.obligation_id, result.disposition))
        .collect::<BTreeMap<_, _>>();
    let disposition = |property, member_id: Option<&StableId>| {
        universe
            .obligations
            .iter()
            .find(|item| item.property == property && item.member_id.as_ref() == member_id)
            .and_then(|item| result_by_id.get(&item.id).copied())
            .expect("validated complete assessment")
    };
    let family = |property| disposition(property, None);
    let all_members = |property| {
        candidate.members.iter().all(|member| {
            disposition(property, Some(&member.member_id)) == DecisionDisposition::Supports
        })
    };
    let any_member_opposes = |property| {
        candidate.members.iter().any(|member| {
            disposition(property, Some(&member.member_id)) == DecisionDisposition::Opposes
        })
    };
    let separation_supported =
        family(DecisionProperty::SeparationRationale) == DecisionDisposition::Supports;
    let common_supported = all_members(DecisionProperty::CommonContractApplies);
    let change_supported =
        family(DecisionProperty::CommonChangeReason) == DecisionDisposition::Supports;
    let conformance_supported =
        family(DecisionProperty::SharedConformanceFeasibility) == DecisionDisposition::Supports;
    let purpose_supported = all_members(DecisionProperty::PurposeConstraintsPreserved);
    let validator_safe = all_members(DecisionProperty::SharedValidatorFeasibility)
        && all_members(DecisionProperty::TypedErrorPreservation)
        && all_members(DecisionProperty::CompatibilityPreservation)
        && all_members(DecisionProperty::PerformancePreservation);
    let proposed_option = if !separation_supported
        && common_supported
        && change_supported
        && conformance_supported
        && purpose_supported
        && validator_safe
    {
        DecisionOption::SharedValidator
    } else if !separation_supported
        && common_supported
        && change_supported
        && conformance_supported
        && purpose_supported
    {
        DecisionOption::SharedConformanceTest
    } else if separation_supported
        && (family(DecisionProperty::CommonChangeReason) == DecisionDisposition::Opposes
            || any_member_opposes(DecisionProperty::CommonContractApplies))
    {
        DecisionOption::IntentionalSeparation
    } else {
        DecisionOption::Inconclusive
    };
    let unresolved_obligation_ids = universe
        .obligations
        .iter()
        .filter(|item| {
            matches!(
                result_by_id[&item.id],
                DecisionDisposition::Inconclusive | DecisionDisposition::NotApplicable
            )
        })
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    let supports = assessment
        .results
        .iter()
        .filter(|item| item.disposition == DecisionDisposition::Supports)
        .count();
    let opposes = assessment
        .results
        .iter()
        .filter(|item| item.disposition == DecisionDisposition::Opposes)
        .count();
    Ok(DecisionProposal {
        schema: PROPOSAL_SCHEMA.to_owned(),
        candidate_id: candidate.candidate_id.clone(),
        proposed_family_id: candidate.proposed_family_id.clone(),
        snapshot_id: candidate.snapshot_id.clone(),
        candidate_hash: universe.input.candidate_hash.clone(),
        obligation_universe_hash: ContentHash::sha256(&canonical_json(&universe)?),
        assessment_hash: ContentHash::sha256(&canonical_json(assessment)?),
        authority: DecisionAuthority::non_authority(),
        denominator: universe.denominator,
        counts: DecisionCounts {
            supports: count(supports)?,
            opposes: count(opposes)?,
            unresolved: count(unresolved_obligation_ids.len())?,
        },
        proposed_option,
        unresolved_obligation_ids,
        unknowns: candidate.unknowns.clone(),
    })
}

pub fn build_assessment(
    candidate: &ResponsibilityFamilyCandidate,
    input: &DecisionAssessmentInput,
) -> Result<DecisionAssessment> {
    let universe = obligations(candidate)?;
    if input.schema != ASSESSMENT_INPUT_SCHEMA
        || input.candidate_hash != universe.input.candidate_hash
        || input.entries.is_empty()
    {
        return Err(DecisionError::InvalidAssessment(
            "assessment input header is invalid",
        ));
    }
    let mut supplied = BTreeMap::new();
    for entry in &input.entries {
        if !sorted_ids(&entry.member_ids)
            || !sorted_ids(&entry.evidence_ids)
            || !sorted_ids(&entry.verification_ids)
            || (matches!(
                entry.disposition,
                DecisionDisposition::Supports | DecisionDisposition::Opposes
            ) && (entry.evidence_ids.is_empty() || entry.verification_ids.is_empty()))
        {
            return Err(DecisionError::InvalidAssessment(
                "assessment entry order or decisive evidence is invalid",
            ));
        }
        if entry.member_ids.is_empty() {
            insert_entry(&mut supplied, entry, None)?;
        } else {
            for member_id in &entry.member_ids {
                insert_entry(&mut supplied, entry, Some(member_id))?;
            }
        }
    }
    let expected = universe
        .obligations
        .iter()
        .map(|item| (item.property, item.member_id.clone()))
        .collect::<BTreeSet<_>>();
    if supplied.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(DecisionError::InvalidAssessment(
            "assessment entries do not exactly cover the obligation universe",
        ));
    }
    let results = universe
        .obligations
        .iter()
        .map(|obligation| {
            let entry = supplied[&(obligation.property, obligation.member_id.clone())];
            DecisionResult {
                obligation_id: obligation.id.clone(),
                disposition: entry.disposition,
                source_ids: obligation.source_ids.clone(),
                evidence_ids: entry.evidence_ids.clone(),
                verification_ids: entry.verification_ids.clone(),
            }
        })
        .collect();
    Ok(DecisionAssessment {
        schema: ASSESSMENT_SCHEMA.to_owned(),
        candidate_hash: universe.input.candidate_hash.clone(),
        obligation_universe_hash: ContentHash::sha256(&canonical_json(&universe)?),
        results,
    })
}

pub fn validate_proposal(
    candidate: &ResponsibilityFamilyCandidate,
    assessment: &DecisionAssessment,
    proposal: &DecisionProposal,
) -> Result<()> {
    if propose(candidate, assessment)? == *proposal {
        Ok(())
    } else {
        Err(DecisionError::ProposalMismatch)
    }
}

fn validate_candidate(candidate: &ResponsibilityFamilyCandidate) -> Result<()> {
    if candidate.schema != CANDIDATE_SCHEMA
        || candidate.candidate_id.kind() != "responsibility-family-candidate"
        || candidate.proposed_family_id.kind() != "responsibility-family"
    {
        return Err(DecisionError::InvalidCandidate(
            "invalid identity or schema",
        ));
    }
    let state = ResponsibilityFamilyState {
        schema: super::STATE_SCHEMA.to_owned(),
        family_id: candidate.proposed_family_id.clone(),
        snapshot_id: candidate.snapshot_id.clone(),
        decision_basis_id: StableId::parse("decision:candidate-validation")
            .expect("static stable ID"),
        common_contract: candidate.proposed_common_contract.clone(),
        extractor: candidate.extractor.clone(),
        members: candidate.members.clone(),
        unknowns: candidate.unknowns.clone(),
    };
    validate_state(&state).map_err(map_state_error)
}

fn map_state_error(error: ResponsibilityFamilyError) -> DecisionError {
    match error {
        ResponsibilityFamilyError::InvalidState(reason) => DecisionError::InvalidCandidate(reason),
        _ => DecisionError::InvalidCandidate("candidate state validation failed"),
    }
}

fn obligation(
    candidate_hash: &ContentHash,
    property: DecisionProperty,
    member_id: Option<&StableId>,
    source_ids: &[StableId],
) -> Result<DecisionObligation> {
    let bindings = BTreeMap::from([
        (
            "candidate_hash".to_owned(),
            Value::String(candidate_hash.to_string()),
        ),
        (
            "member_id".to_owned(),
            member_id.map_or(Value::Null, |id| Value::String(id.to_string())),
        ),
        (
            "property".to_owned(),
            serde_json::to_value(property).expect("closed property serializes"),
        ),
    ]);
    Ok(DecisionObligation {
        id: StableId::derived("responsibility-family-decision-obligation", &bindings)?,
        property,
        member_id: member_id.cloned(),
        source_ids: source_ids.to_vec(),
    })
}

fn validate_assessment(
    universe: &DecisionObligationUniverse,
    assessment: &DecisionAssessment,
) -> Result<()> {
    if assessment.schema != ASSESSMENT_SCHEMA
        || assessment.candidate_hash != universe.input.candidate_hash
        || assessment.obligation_universe_hash != ContentHash::sha256(&canonical_json(universe)?)
        || assessment.results.len() != universe.obligations.len()
    {
        return Err(DecisionError::InvalidAssessment(
            "header or denominator mismatch",
        ));
    }
    for (expected, result) in universe.obligations.iter().zip(&assessment.results) {
        if result.obligation_id != expected.id
            || result.source_ids.is_empty()
            || !sorted_ids(&result.source_ids)
            || !sorted_ids(&result.evidence_ids)
            || !sorted_ids(&result.verification_ids)
            || (matches!(
                result.disposition,
                DecisionDisposition::Supports | DecisionDisposition::Opposes
            ) && (result.evidence_ids.is_empty() || result.verification_ids.is_empty()))
        {
            return Err(DecisionError::InvalidAssessment(
                "result closure, order, or decisive evidence is invalid",
            ));
        }
    }
    Ok(())
}

fn insert_entry<'a>(
    supplied: &mut BTreeMap<(DecisionProperty, Option<StableId>), &'a DecisionAssessmentEntry>,
    entry: &'a DecisionAssessmentEntry,
    member_id: Option<&StableId>,
) -> Result<()> {
    if supplied
        .insert((entry.property, member_id.cloned()), entry)
        .is_some()
    {
        return Err(DecisionError::InvalidAssessment(
            "assessment entry coverage overlaps",
        ));
    }
    Ok(())
}

fn sorted_ids(ids: &[StableId]) -> bool {
    ids.windows(2).all(|pair| pair[0] < pair[1])
}

fn count(value: usize) -> Result<u64> {
    u64::try_from(value).map_err(|_| DecisionError::CountOverflow)
}

#[cfg(test)]
mod tests;
