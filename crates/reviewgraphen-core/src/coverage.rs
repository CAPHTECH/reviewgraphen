use crate::{ReviewAggregate, StableId};
use serde::Serialize;
use std::collections::BTreeSet;

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
