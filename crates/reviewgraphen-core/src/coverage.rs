use crate::{
    ContentHash, DecisionOutcomeV3, DomainError, EvidenceRelationV3, FindingStatusV3,
    ObligationLifecycle, ReviewAggregate, StableId, VerificationOutcomeV3, event::EventLogV4,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[allow(dead_code)] // consumed by the following M6 reducer slice
pub(crate) const HISTORICAL_COVERAGE_SCHEMA_V4: &str =
    "reviewgraphen.historical_coverage_snapshot.v4";

#[allow(dead_code)] // consumed by the following M6 reducer slice
#[derive(Serialize)]
struct HistoricalCoverageIdentityV4<'a> {
    universe_id: &'a StableId,
    snapshot_id: &'a StableId,
    profile_id: &'a str,
    policy_version: &'a str,
    rule_set_hash: &'a ContentHash,
    extractor_set_hash: &'a ContentHash,
    rule_pack_version: &'a str,
    denominator_obligation_ids: &'a BTreeSet<StableId>,
    denominator_count: u64,
    completed_obligation_ids: &'a BTreeSet<StableId>,
    completed_count: u64,
    evidence_supported_obligation_ids: &'a BTreeSet<StableId>,
    evidence_supported_count: u64,
    verified_obligation_ids: &'a BTreeSet<StableId>,
    verified_count: u64,
    fresh_obligation_ids: &'a BTreeSet<StableId>,
    fresh_count: u64,
    human_accepted_obligation_ids: &'a BTreeSet<StableId>,
    human_accepted_count: u64,
}

/// Internal immutable source-coverage record for M6 historical reduction.
/// It is derived from one pinned replay aggregate and is never persisted or
/// accepted as authority.
#[allow(dead_code)] // consumed by the following M6 reducer slice
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct HistoricalCoverageSnapshotV4 {
    schema: &'static str,
    id: StableId,
    universe_id: StableId,
    snapshot_id: StableId,
    profile_id: String,
    policy_version: String,
    rule_set_hash: ContentHash,
    extractor_set_hash: ContentHash,
    rule_pack_version: String,
    denominator_obligation_ids: BTreeSet<StableId>,
    denominator_count: u64,
    completed_obligation_ids: BTreeSet<StableId>,
    completed_count: u64,
    evidence_supported_obligation_ids: BTreeSet<StableId>,
    evidence_supported_count: u64,
    verified_obligation_ids: BTreeSet<StableId>,
    verified_count: u64,
    fresh_obligation_ids: BTreeSet<StableId>,
    fresh_count: u64,
    human_accepted_obligation_ids: BTreeSet<StableId>,
    human_accepted_count: u64,
}

#[allow(dead_code)] // consumed by the following M6 reducer slice
impl HistoricalCoverageSnapshotV4 {
    pub(crate) fn retained_bytes_for_target_projection(&self) -> u64 {
        let mut total = u64::try_from(std::mem::size_of::<Self>()).unwrap_or(u64::MAX);
        for bytes in [
            self.id.allocated_bytes(),
            self.universe_id.allocated_bytes(),
            self.snapshot_id.allocated_bytes(),
            self.profile_id.capacity(),
            self.policy_version.capacity(),
            self.rule_set_hash.allocated_bytes(),
            self.extractor_set_hash.allocated_bytes(),
            self.rule_pack_version.capacity(),
        ] {
            total = total.saturating_add(u64::try_from(bytes).unwrap_or(u64::MAX));
        }
        for ids in [
            &self.denominator_obligation_ids,
            &self.completed_obligation_ids,
            &self.evidence_supported_obligation_ids,
            &self.verified_obligation_ids,
            &self.fresh_obligation_ids,
            &self.human_accepted_obligation_ids,
        ] {
            total = total.saturating_add(
                u64::try_from(
                    ids.len()
                        .saturating_mul(std::mem::size_of::<StableId>() + 128),
                )
                .unwrap_or(u64::MAX),
            );
            for id in ids {
                total =
                    total.saturating_add(u64::try_from(id.allocated_bytes()).unwrap_or(u64::MAX));
            }
        }
        total
    }

    pub(crate) fn derive(log: &EventLogV4) -> crate::Result<Self> {
        let source = log.historical_coverage_source_v4()?;
        Self::derive_from_parts(source.aggregate(), source.v3(), source.cover())
    }

    pub(crate) fn derive_from_parts(
        aggregate: &ReviewAggregate,
        v3: &crate::event::V3RunAggregate,
        cover: &crate::ContextCoverV4,
    ) -> crate::Result<Self> {
        Self::derive_from_optional_parts(aggregate, v3, Some(cover))
    }

    pub(crate) fn derive_without_m5(
        aggregate: &ReviewAggregate,
        v3: &crate::event::V3RunAggregate,
    ) -> crate::Result<Self> {
        Self::derive_from_optional_parts(aggregate, v3, None)
    }

    fn derive_from_optional_parts(
        aggregate: &ReviewAggregate,
        v3: &crate::event::V3RunAggregate,
        cover: Option<&crate::ContextCoverV4>,
    ) -> crate::Result<Self> {
        let denominator_obligation_ids = aggregate.universe().obligation_ids().clone();
        let plan_id = match cover {
            Some(cover) => cover.plan_id(),
            None => {
                let mut plans = aggregate.review_plans();
                let plan = plans.next().ok_or(DomainError::HistoricalPrefixMismatch(
                    "historical no-M5 coverage has no plan",
                ))?;
                if plans.next().is_some() {
                    return Err(DomainError::HistoricalPrefixMismatch(
                        "historical no-M5 coverage has multiple plans",
                    ));
                }
                plan.id()
            }
        };

        let mut execution_ids = BTreeSet::new();
        for execution in aggregate
            .executions()
            .filter(|execution| execution.plan_id() == plan_id)
        {
            if execution.obligation_ids().len() != 1
                || !execution
                    .obligation_ids()
                    .is_subset(&denominator_obligation_ids)
                || execution.snapshot_id() != aggregate.program().snapshot_id()
                || !execution_ids.insert(execution.id().clone())
            {
                return Err(DomainError::HistoricalPrefixMismatch(
                    "historical plan execution scope is not unique and exact",
                ));
            }
        }

        let mut claim_ids = BTreeSet::new();
        let mut claim_obligation = BTreeMap::<StableId, StableId>::new();
        let mut claims_by_execution = BTreeMap::<StableId, BTreeSet<StableId>>::new();
        for claim in aggregate
            .execution_claims()
            .filter(|claim| execution_ids.contains(claim.execution_id()))
        {
            if claim.obligation_ids().len() != 1
                || !claim
                    .obligation_ids()
                    .is_subset(&denominator_obligation_ids)
                || !claim_ids.insert(claim.id().clone())
            {
                return Err(DomainError::HistoricalPrefixMismatch(
                    "historical plan claim scope is not unique and exact",
                ));
            }
            claim_obligation.insert(
                claim.id().clone(),
                claim
                    .obligation_ids()
                    .first()
                    .ok_or(DomainError::HistoricalPrefixMismatch(
                        "historical plan claim has no obligation",
                    ))?
                    .clone(),
            );
            claims_by_execution
                .entry(claim.execution_id().clone())
                .or_default()
                .insert(claim.id().clone());
        }
        for execution in aggregate
            .executions()
            .filter(|execution| execution_ids.contains(execution.id()))
        {
            if claims_by_execution
                .get(execution.id())
                .is_none_or(|ids| ids != execution.parsed_claim_ids())
                || execution.outcome().is_structured() == execution.parsed_claim_ids().is_empty()
            {
                return Err(DomainError::HistoricalPrefixMismatch(
                    "historical execution/claim closure is incomplete",
                ));
            }
        }
        if !claim_ids
            .iter()
            .all(|claim_id| v3.historical_assessments().contains_key(claim_id))
        {
            return Err(DomainError::HistoricalPrefixMismatch(
                "historical plan claim assessment is missing",
            ));
        }

        let mut reproduces_by_claim = BTreeMap::<StableId, BTreeSet<StableId>>::new();
        for binding in v3
            .historical_bindings()
            .values()
            .filter(|binding| claim_ids.contains(binding.claim_id()))
        {
            if !v3.historical_evidence().contains_key(binding.evidence_id()) {
                return Err(DomainError::HistoricalPrefixMismatch(
                    "historical evidence binding target is missing",
                ));
            }
            if binding.relation() == EvidenceRelationV3::Reproduces {
                reproduces_by_claim
                    .entry(binding.claim_id().clone())
                    .or_default()
                    .insert(binding.evidence_id().clone());
            }
        }
        let evidence_supported_obligation_ids = reproduces_by_claim
            .keys()
            .map(|claim_id| {
                claim_obligation.get(claim_id).cloned().ok_or(
                    DomainError::HistoricalPrefixMismatch(
                        "historical evidence binding claim is outside plan closure",
                    ),
                )
            })
            .collect::<crate::Result<BTreeSet<_>>>()?;

        let mut passed_by_claim = BTreeMap::<StableId, BTreeSet<StableId>>::new();
        for verification in v3
            .historical_verifications()
            .values()
            .filter(|verification| claim_ids.contains(verification.claim_id()))
        {
            if verification.outcome() == VerificationOutcomeV3::Passed
                && !verification.evidence_ids().is_empty()
                && verification.evidence_ids().iter().all(|evidence_id| {
                    reproduces_by_claim
                        .get(verification.claim_id())
                        .is_some_and(|ids| ids.contains(evidence_id))
                })
            {
                passed_by_claim
                    .entry(verification.claim_id().clone())
                    .or_default()
                    .insert(verification.id().clone());
            }
        }
        let verified_obligation_ids = passed_by_claim
            .keys()
            .map(|claim_id| {
                claim_obligation.get(claim_id).cloned().ok_or(
                    DomainError::HistoricalPrefixMismatch(
                        "historical passed verification claim is outside plan closure",
                    ),
                )
            })
            .collect::<crate::Result<BTreeSet<_>>>()?;
        // ADR 0021 freezes V4 fresh-verified as exactly native verified. Keep
        // a distinct owned set/field even though the values are equal.
        let fresh_obligation_ids = verified_obligation_ids.clone();

        let mut human_accepted_obligation_ids = BTreeSet::new();
        for (claim_id, assessment) in v3
            .historical_assessments()
            .iter()
            .filter(|(claim_id, _)| claim_ids.contains(*claim_id))
        {
            if assessment.decision_conflict() || !passed_by_claim.contains_key(claim_id) {
                continue;
            }
            let (Some(decision_id), Some(finding_id)) = (
                assessment.active_decision_id(),
                assessment.current_finding_id(),
            ) else {
                continue;
            };
            let decision = v3.historical_decisions().get(decision_id).ok_or(
                DomainError::HistoricalPrefixMismatch("historical active decision is missing"),
            )?;
            let finding = v3.historical_findings().get(finding_id).ok_or(
                DomainError::HistoricalPrefixMismatch("historical current finding is missing"),
            )?;
            let empty_reproduces = BTreeSet::new();
            let all_reproduces = reproduces_by_claim
                .get(claim_id)
                .unwrap_or(&empty_reproduces);
            if decision.claim_id() == claim_id
                && decision.outcome() == DecisionOutcomeV3::Accept
                && finding.claim_id() == claim_id
                && finding.status() == FindingStatusV3::Accepted
                && finding.decision_id() == Some(decision_id)
                && finding.evidence_ids().len() == all_reproduces.len()
                && finding
                    .evidence_ids()
                    .iter()
                    .zip(all_reproduces)
                    .all(|(left, right)| left == right)
            {
                let passed =
                    passed_by_claim
                        .get(claim_id)
                        .ok_or(DomainError::HistoricalPrefixMismatch(
                            "historical accepted claim has no passed verification",
                        ))?;
                if finding.verification_ids().len() != passed.len()
                    || !finding
                        .verification_ids()
                        .iter()
                        .zip(passed)
                        .all(|(left, right)| left == right)
                {
                    continue;
                }
                human_accepted_obligation_ids.insert(
                    claim_obligation.get(claim_id).cloned().ok_or(
                        DomainError::HistoricalPrefixMismatch(
                            "historical accepted claim is outside plan closure",
                        ),
                    )?,
                );
            }
        }

        let completed_obligation_ids = aggregate
            .obligations()
            .filter(|obligation| obligation.lifecycle() == ObligationLifecycle::Completed)
            .map(|obligation| obligation.id().clone())
            .collect::<BTreeSet<_>>();
        for (name, ids) in [
            ("completed", &completed_obligation_ids),
            ("evidence-supported", &evidence_supported_obligation_ids),
            ("verified", &verified_obligation_ids),
            ("fresh", &fresh_obligation_ids),
            ("human-accepted", &human_accepted_obligation_ids),
        ] {
            if !ids.is_subset(&denominator_obligation_ids) {
                return Err(DomainError::Validation(format!(
                    "historical coverage {name} numerator is outside its denominator"
                )));
            }
        }
        let universe = aggregate.universe();
        let mut value = Self {
            schema: HISTORICAL_COVERAGE_SCHEMA_V4,
            id: StableId::parse("historical-coverage-snapshot-v4:pending")?,
            universe_id: universe.id().clone(),
            snapshot_id: universe.snapshot_id().clone(),
            profile_id: universe.profile_id().to_owned(),
            policy_version: universe.policy_version().to_owned(),
            rule_set_hash: universe.rule_set_hash().clone(),
            extractor_set_hash: universe.extractor_set_hash().clone(),
            rule_pack_version: universe.rule_pack_version().to_owned(),
            denominator_count: denominator_obligation_ids.len() as u64,
            denominator_obligation_ids,
            completed_count: completed_obligation_ids.len() as u64,
            completed_obligation_ids,
            evidence_supported_count: evidence_supported_obligation_ids.len() as u64,
            evidence_supported_obligation_ids,
            verified_count: verified_obligation_ids.len() as u64,
            verified_obligation_ids,
            fresh_count: fresh_obligation_ids.len() as u64,
            fresh_obligation_ids,
            human_accepted_count: human_accepted_obligation_ids.len() as u64,
            human_accepted_obligation_ids,
        };
        let hash = ContentHash::sha256(&crate::canonical_json(&value.identity())?);
        value.id = StableId::parse(format!("historical-coverage-snapshot-v4:{hash}"))?;
        Ok(value)
    }

    fn identity(&self) -> HistoricalCoverageIdentityV4<'_> {
        HistoricalCoverageIdentityV4 {
            universe_id: &self.universe_id,
            snapshot_id: &self.snapshot_id,
            profile_id: &self.profile_id,
            policy_version: &self.policy_version,
            rule_set_hash: &self.rule_set_hash,
            extractor_set_hash: &self.extractor_set_hash,
            rule_pack_version: &self.rule_pack_version,
            denominator_obligation_ids: &self.denominator_obligation_ids,
            denominator_count: self.denominator_count,
            completed_obligation_ids: &self.completed_obligation_ids,
            completed_count: self.completed_count,
            evidence_supported_obligation_ids: &self.evidence_supported_obligation_ids,
            evidence_supported_count: self.evidence_supported_count,
            verified_obligation_ids: &self.verified_obligation_ids,
            verified_count: self.verified_count,
            fresh_obligation_ids: &self.fresh_obligation_ids,
            fresh_count: self.fresh_count,
            human_accepted_obligation_ids: &self.human_accepted_obligation_ids,
            human_accepted_count: self.human_accepted_count,
        }
    }

    pub(crate) fn id(&self) -> &StableId {
        &self.id
    }
    pub(crate) fn body_hash(&self) -> crate::Result<ContentHash> {
        Ok(ContentHash::sha256(&crate::canonical_json(self)?))
    }
    pub(crate) fn denominator_obligation_ids(&self) -> &BTreeSet<StableId> {
        &self.denominator_obligation_ids
    }
    pub(crate) fn completed_obligation_ids(&self) -> &BTreeSet<StableId> {
        &self.completed_obligation_ids
    }
    pub(crate) fn evidence_supported_obligation_ids(&self) -> &BTreeSet<StableId> {
        &self.evidence_supported_obligation_ids
    }
    pub(crate) fn verified_obligation_ids(&self) -> &BTreeSet<StableId> {
        &self.verified_obligation_ids
    }
    pub(crate) fn fresh_obligation_ids(&self) -> &BTreeSet<StableId> {
        &self.fresh_obligation_ids
    }
    pub(crate) fn human_accepted_obligation_ids(&self) -> &BTreeSet<StableId> {
        &self.human_accepted_obligation_ids
    }
}

/// Explicit count ratio. A zero numerator is always `0.0`, including an empty universe.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Ratio {
    /// Completed or qualified record count/weight.
    pub numerator: f64,
    /// Declared eligible universe count/weight.
    pub denominator: f64,
    /// `numerator / denominator`, or `0.0` if denominator is zero.
    pub percentage: f64,
}

impl Ratio {
    fn new(numerator: f64, denominator: f64) -> Self {
        let percentage = if numerator == 0.0 || denominator == 0.0 {
            0.0
        } else {
            numerator / denominator
        };
        Self {
            numerator,
            denominator,
            percentage,
        }
    }
}

/// Raw and risk-weighted coverage for one state axis.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct CoverageMeasure {
    /// Record-count coverage.
    pub raw: Ratio,
    /// Risk-weighted coverage over the same eligible denominator.
    pub weighted: Ratio,
}

/// M1 coverage report. Completion, evidence, verification, and freshness remain distinct.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Coverage {
    /// Versioned universe covered by this report.
    pub universe_id: StableId,
    /// Eligible raw denominator; excluded candidates are retained in the universe descriptor.
    pub denominator: usize,
    /// Completed/executed obligations only.
    pub raw: CoverageMeasure,
    /// Obligations with a supported claim.
    pub evidence_supported: CoverageMeasure,
    /// Obligations with a passed verification, regardless of freshness.
    pub verified: CoverageMeasure,
    /// Obligations with a passed verification explicitly marked fresh.
    pub fresh: CoverageMeasure,
    /// Obligations whose claims have a distinct explicit human acceptance.
    pub human_accepted: CoverageMeasure,
    /// Explicitly recorded exclusions outside the eligible denominator.
    pub exclusion_count: usize,
    /// Explicit excluded weight outside the eligible weighted denominator.
    pub excluded_weight: f64,
}

impl Coverage {
    /// Computes all M1 measures from the aggregate without inferring trust state.
    #[must_use]
    pub fn from_aggregate(aggregate: &ReviewAggregate) -> Self {
        let obligations = aggregate.obligations().collect::<Vec<_>>();
        let denominator = obligations.len();
        let total_weight = obligations.iter().map(|item| item.weight()).sum::<f64>();
        let raw_ids = obligations
            .iter()
            .filter(|item| item.lifecycle() == crate::ObligationLifecycle::Completed)
            .map(|item| item.id().clone())
            .collect::<BTreeSet<_>>();
        let evidence_supported = aggregate.obligations_with_connected_support();
        let verified = aggregate.obligations_with_connected_verification(false);
        let fresh = aggregate.obligations_with_connected_verification(true);
        let human_accepted = aggregate.obligations_with_human_acceptance();
        Self {
            universe_id: aggregate.universe().id().clone(),
            denominator,
            raw: measure(&obligations, &raw_ids, total_weight),
            evidence_supported: measure(&obligations, &evidence_supported, total_weight),
            verified: measure(&obligations, &verified, total_weight),
            fresh: measure(&obligations, &fresh, total_weight),
            human_accepted: measure(&obligations, &human_accepted, total_weight),
            exclusion_count: aggregate.universe().exclusions().len(),
            excluded_weight: aggregate.universe().excluded_weight(),
        }
    }
}

fn measure(
    obligations: &[&crate::Obligation],
    qualified: &BTreeSet<StableId>,
    total_weight: f64,
) -> CoverageMeasure {
    let raw_numerator = qualified.len() as f64;
    let weighted_numerator = obligations
        .iter()
        .filter(|item| qualified.contains(item.id()))
        .map(|item| item.weight())
        .sum::<f64>();
    CoverageMeasure {
        raw: Ratio::new(raw_numerator, obligations.len() as f64),
        weighted: Ratio::new(weighted_numerator, total_weight),
    }
}
