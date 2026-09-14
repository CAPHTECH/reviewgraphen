//! Accepted, versioned responsibility-family product state.
//!
//! This state is separate from ProgramSpace: source similarity is a candidate
//! signal, while family acceptance is an explicit human decision bound to
//! evidence and verification records.

use crate::{ContentHash, DomainError, StableId, canonical_json};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const RESPONSIBILITY_FAMILY_STATE_V1_SCHEMA: &str =
    "reviewgraphen.responsibility_family_state.v1";
const MAX_MEMBERS: usize = 4_096;
const MAX_ITEMS: usize = 256;
const MAX_TEXT_BYTES: usize = 4_096;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FamilyMaintenanceDecisionV1 {
    SharedValidator,
    SharedConformanceTest,
    IntentionalSeparation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyContractV1 {
    pub id: String,
    pub hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyExtractorV1 {
    pub id: String,
    pub version: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyMemberV1 {
    pub member_id: StableId,
    pub path: String,
    pub symbol: String,
    pub anchor: ContentHash,
    pub purpose_constraints: BTreeSet<String>,
    pub source_ids: Vec<StableId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyAcceptanceV1 {
    pub human_decision_id: StableId,
    pub proposal_hash: ContentHash,
    pub evidence_ids: Vec<StableId>,
    pub verification_ids: Vec<StableId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyAuthorityV1 {
    pub classification: String,
    pub accepted: bool,
    pub verified: bool,
    pub human_accepted: bool,
    pub sign_off: bool,
}

impl FamilyAuthorityV1 {
    fn family_accepted() -> Self {
        Self {
            classification: "accepted_responsibility_family".to_owned(),
            accepted: true,
            verified: false,
            human_accepted: true,
            sign_off: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedResponsibilityFamilyStateV1 {
    pub schema: String,
    pub family_id: StableId,
    pub snapshot_id: StableId,
    pub decision: FamilyMaintenanceDecisionV1,
    pub common_contract: FamilyContractV1,
    pub extractor: FamilyExtractorV1,
    pub members: Vec<FamilyMemberV1>,
    pub implementation_denominator: u64,
    pub endpoint_denominator: u64,
    pub acceptance: FamilyAcceptanceV1,
    pub unknowns: Vec<String>,
    pub authority: FamilyAuthorityV1,
}

impl AcceptedResponsibilityFamilyStateV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        family_id: StableId,
        snapshot_id: StableId,
        decision: FamilyMaintenanceDecisionV1,
        common_contract: FamilyContractV1,
        extractor: FamilyExtractorV1,
        members: Vec<FamilyMemberV1>,
        endpoint_denominator: u64,
        acceptance: FamilyAcceptanceV1,
        unknowns: Vec<String>,
    ) -> Result<Self, DomainError> {
        let implementation_denominator = u64::try_from(members.len()).map_err(|_| {
            DomainError::Validation("responsibility-family member count overflow".to_owned())
        })?;
        let state = Self {
            schema: RESPONSIBILITY_FAMILY_STATE_V1_SCHEMA.to_owned(),
            family_id,
            snapshot_id,
            decision,
            common_contract,
            extractor,
            members,
            implementation_denominator,
            endpoint_denominator,
            acceptance,
            unknowns,
            authority: FamilyAuthorityV1::family_accepted(),
        };
        state.validate()?;
        Ok(state)
    }

    pub fn from_json_slice(bytes: &[u8]) -> Result<Self, DomainError> {
        let state: Self = serde_json::from_slice(bytes).map_err(|error| {
            DomainError::Validation(format!("invalid responsibility-family state: {error}"))
        })?;
        state.validate()?;
        if canonical_json(&state)? != bytes {
            return Err(DomainError::Validation(
                "responsibility-family state must be canonical JSON".to_owned(),
            ));
        }
        Ok(state)
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, DomainError> {
        self.validate()?;
        canonical_json(self)
    }

    pub fn content_hash(&self) -> Result<ContentHash, DomainError> {
        Ok(ContentHash::sha256(&self.canonical_bytes()?))
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        let sorted_ids = |ids: &[StableId], kind: &str| {
            !ids.is_empty()
                && ids.len() <= MAX_ITEMS
                && ids.windows(2).all(|pair| pair[0] < pair[1])
                && ids.iter().all(|id| id.kind() == kind)
        };
        if self.schema != RESPONSIBILITY_FAMILY_STATE_V1_SCHEMA
            || self.family_id.kind() != "responsibility-family"
            || self.snapshot_id.kind() != "snapshot"
            || self.members.is_empty()
            || self.members.len() > MAX_MEMBERS
            || self.implementation_denominator != self.members.len() as u64
            || self.endpoint_denominator == 0
            || !text(&self.common_contract.id)
            || !text(&self.extractor.id)
            || !text(&self.extractor.version)
            || self.acceptance.human_decision_id.kind() != "decision"
            || !sorted_ids(&self.acceptance.evidence_ids, "evidence")
            || !sorted_ids(&self.acceptance.verification_ids, "verification")
            || self.unknowns.len() > MAX_ITEMS
            || self.unknowns.iter().any(|value| !text(value))
            || !self.unknowns.windows(2).all(|pair| pair[0] < pair[1])
            || self.authority != FamilyAuthorityV1::family_accepted()
        {
            return Err(DomainError::Validation(
                "invalid accepted responsibility-family header or authority".to_owned(),
            ));
        }
        if !self
            .members
            .windows(2)
            .all(|pair| pair[0].member_id < pair[1].member_id)
        {
            return Err(DomainError::Validation(
                "responsibility-family members must be strictly ordered".to_owned(),
            ));
        }
        for member in &self.members {
            if member.member_id.kind() != "responsibility-member"
                || !safe_path(&member.path)
                || !text(&member.symbol)
                || member.purpose_constraints.len() > MAX_ITEMS
                || member.purpose_constraints.iter().any(|value| !text(value))
                || member.source_ids.is_empty()
                || member.source_ids.len() > MAX_ITEMS
                || !member.source_ids.windows(2).all(|pair| pair[0] < pair[1])
            {
                return Err(DomainError::Validation(
                    "invalid accepted responsibility-family member".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

fn text(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_TEXT_BYTES && !value.chars().any(char::is_control)
}

fn safe_path(path: &str) -> bool {
    text(path)
        && !path.starts_with('/')
        && !path.contains('\\')
        && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(kind: &str, name: &str) -> StableId {
        StableId::parse(format!("{kind}:{name}")).unwrap()
    }

    fn state() -> AcceptedResponsibilityFamilyStateV1 {
        AcceptedResponsibilityFamilyStateV1::new(
            id("responsibility-family", "path-policy"),
            id("snapshot", "one"),
            FamilyMaintenanceDecisionV1::SharedConformanceTest,
            FamilyContractV1 {
                id: "path.base@1".to_owned(),
                hash: ContentHash::sha256(b"contract"),
            },
            FamilyExtractorV1 {
                id: "rust-shape".to_owned(),
                version: "1".to_owned(),
            },
            vec![FamilyMemberV1 {
                member_id: id("responsibility-member", "one"),
                path: "rust/a/src/lib.rs".to_owned(),
                symbol: "validate".to_owned(),
                anchor: ContentHash::sha256(b"body"),
                purpose_constraints: BTreeSet::from(["typed error preserved".to_owned()]),
                source_ids: vec![id("function", "validate")],
            }],
            3,
            FamilyAcceptanceV1 {
                human_decision_id: id("decision", "accept"),
                proposal_hash: ContentHash::sha256(b"proposal"),
                evidence_ids: vec![id("evidence", "source")],
                verification_ids: vec![id("verification", "test")],
            },
            vec!["performance across all callers is unknown".to_owned()],
        )
        .unwrap()
    }

    #[test]
    fn accepted_state_is_canonical_strict_and_keeps_denominators_separate() {
        let state = state();
        assert_eq!(state.implementation_denominator, 1);
        assert_eq!(state.endpoint_denominator, 3);
        let bytes = state.canonical_bytes().unwrap();
        assert_eq!(
            AcceptedResponsibilityFamilyStateV1::from_json_slice(&bytes).unwrap(),
            state
        );
        let mut value = serde_json::to_value(&state).unwrap();
        value["authority"]["verified"] = serde_json::json!(true);
        let tampered = canonical_json(&value).unwrap();
        assert!(AcceptedResponsibilityFamilyStateV1::from_json_slice(&tampered).is_err());
    }

    #[test]
    fn accepted_state_requires_human_decision_evidence_and_verification() {
        let mut missing_evidence = state();
        missing_evidence.acceptance.evidence_ids.clear();
        assert!(missing_evidence.validate().is_err());
        let mut missing_verification = state();
        missing_verification.acceptance.verification_ids.clear();
        assert!(missing_verification.validate().is_err());
        let mut wrong_authority = state();
        wrong_authority.acceptance.human_decision_id = id("claim", "not-human-decision");
        assert!(wrong_authority.validate().is_err());
    }
}
