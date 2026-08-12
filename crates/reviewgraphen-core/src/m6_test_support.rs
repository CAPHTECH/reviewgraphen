//! Test-only, roots-bound fixture for the M6 S0 -> distinct-S1 reducer path.
//!
//! This module deliberately owns no raw event JSON, authority capability, or
//! inventory constructor.  The event fixture supplies both history prefixes
//! through their normal CAS/replay admission paths; this module only joins
//! those opaque values at the reducer seam.

use crate::{
    ContentHash, M6Result, M6StalenessPhaseV5,
    event::{CompleteM5V4Fixture, NoM5V5Fixture},
    m6::{
        IncrementalStalenessInputV5, M6FixturePhases, m6_distinct_s0_s1_program_fixture,
        m6_fixture_phases_from_exact_prefixes,
    },
};

/// Fixed time prevents the assessment ID and every derived digest from being
/// accidentally non-deterministic in the end-to-end reducer test.
pub(crate) const DISTINCT_S0_S1_ASSESSMENT_TIME: &str = "2026-08-12T01:02:03Z";

/// Fully admitted historical source plus a separately admitted, terminal S1
/// predecessor.  Its fields are intentionally opaque event fixtures: callers
/// can neither replace their inventories nor mint a replay authority.
pub(crate) struct DistinctS0S1StalenessFixture {
    source: CompleteM5V4Fixture,
    target: NoM5V5Fixture,
    phases: M6FixturePhases,
}

impl DistinctS0S1StalenessFixture {
    /// Builds the exact E2E fixture through typed constructors, trusted CAS,
    /// roots-bound replay, and sealed mapping/correspondence derivation only.
    pub(crate) fn new() -> M6Result<Self> {
        let (source_program, target_program, source_bytes, target_bytes) =
            m6_distinct_s0_s1_program_fixture()?;
        let source = CompleteM5V4Fixture::from_program_and_sources(
            source_program,
            source_bytes,
            crate::StableId::parse("run:m6-distinct-s0-fixture")?,
        )?;
        let target = NoM5V5Fixture::from_program_and_sources(
            target_program,
            target_bytes,
            crate::StableId::parse("run:m6-distinct-s1-fixture")?,
        )?;
        let phases = target.with_terminal(|_, target_actual| {
            m6_fixture_phases_from_exact_prefixes(&source, &target, target_actual)
        })?;
        let value = Self {
            source,
            target,
            phases,
        };
        value.assert_admitted_topology();
        Ok(value)
    }

    /// The reducer receives the event-owned actual target inventory produced
    /// by the terminal selector.  In particular, no `StableId -> JSON` test
    /// map can stand in for the target predecessor.
    pub(crate) fn reduce(&self) -> M6Result<M6StalenessPhaseV5> {
        self.target.with_terminal(|target, target_actual| {
            let input = IncrementalStalenessInputV5::new(
                self.source.log(),
                self.phases.closure(),
                self.phases.mapping(),
                self.phases.correspondence(),
                target,
            )?;
            input.reduce_v5(target_actual, DISTINCT_S0_S1_ASSESSMENT_TIME)
        })
    }

    #[cfg(test)]
    fn reduce_with_working_limit(&self, working_limit: usize) -> M6Result<M6StalenessPhaseV5> {
        self.target.with_terminal(|target, target_actual| {
            let input = IncrementalStalenessInputV5::new(
                self.source.log(),
                self.phases.closure(),
                self.phases.mapping(),
                self.phases.correspondence(),
                target,
            )?;
            input.reduce_v5_with_working_limit_for_test(
                target_actual,
                DISTINCT_S0_S1_ASSESSMENT_TIME,
                working_limit,
            )
        })
    }

    #[cfg(test)]
    pub(crate) fn target_plan_json_for_test(&self) -> serde_json::Value {
        self.target
            .with_terminal(|target, _| {
                serde_json::to_value(target.plan())
                    .map_err(|error| crate::m6::M6Error::Canonical(error.to_string()))
            })
            .expect("roots-bound target plan serialization")
    }

    #[cfg(test)]
    fn external_component_sum(&self) -> M6Result<(usize, [usize; 5])> {
        self.target.with_terminal(|target, target_actual| {
            let input = IncrementalStalenessInputV5::new(
                self.source.log(),
                self.phases.closure(),
                self.phases.mapping(),
                self.phases.correspondence(),
                target,
            )?;
            input.external_component_sum_for_test(target_actual)
        })
    }

    /// Invariant assertions intentionally sit next to the builder, so every
    /// reducer test inherits the cross-run/snapshot and terminal-M5 boundary.
    fn assert_admitted_topology(&self) {
        assert_ne!(self.source.run_id(), self.target.run_id());
        assert_ne!(self.source.snapshot_id(), self.target.snapshot_id());
        assert!(self.source.has_m5_bundle());
        assert_eq!(self.source.m5_event_count(), 1);
        assert_eq!(self.target.m5_event_count(), 0);

        assert_eq!(
            self.phases.closure().source_snapshot_id(),
            self.source.snapshot_id()
        );
        assert_eq!(
            self.phases.closure().target_snapshot_id(),
            self.target.snapshot_id()
        );
        assert_eq!(
            self.phases.mapping().morphism().source_closure_id(),
            self.phases.closure().id()
        );
        assert_eq!(
            self.phases.correspondence().correspondence().morphism_id(),
            self.phases.mapping().morphism().id()
        );
    }

    #[cfg(test)]
    fn assessment_digest(phase: &M6StalenessPhaseV5) -> ContentHash {
        phase
            .assessment()
            .body_hash()
            .expect("sealed assessment hash")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HistoricalAssessmentStatusV5, StaleReasonV5, event::HistoricalSourceRecordKindV4};
    use std::collections::BTreeSet;

    #[test]
    fn distinct_s0_s1_reducer_uses_roots_bound_complete_and_no_m5_prefixes() {
        let fixture = DistinctS0S1StalenessFixture::new().expect("admitted E2E fixture");
        let first = fixture.reduce().expect("S0 -> S1 reducer");
        let second = fixture.reduce().expect("repeat S0 -> S1 reducer");

        // The complete V4 source uses 20 actual entries from the closed
        // 21-kind vocabulary.  An M5 attempt can have a GlobalCandidate or a
        // GluingObstruction, never both; this failed source correctly retains
        // the latter and excludes the former.  The enum's all-21-kind table
        // coverage belongs to the focused M6 unit fixtures.
        let mut source_kinds = BTreeSet::new();
        fixture
            .source
            .log()
            .historical_prefix_projection_v4()
            .expect("complete source historical projection")
            .try_visit_records(|record| {
                source_kinds.insert(record.kind());
                Ok::<(), crate::DomainError>(())
            })
            .expect("source historical traversal");
        assert_eq!(source_kinds.len(), 20, "source kinds: {source_kinds:?}");
        assert!(source_kinds.contains(&HistoricalSourceRecordKindV4::GluingObstruction));
        assert!(!source_kinds.contains(&HistoricalSourceRecordKindV4::GlobalCandidate));
        assert_eq!(
            first.records().len() as u64,
            first.assessment().record_count()
        );
        assert!(
            first
                .records()
                .iter()
                .any(|record| record.status() == HistoricalAssessmentStatusV5::Stale)
        );

        // The sealed result—not raw reducer prose—must be byte-stable under
        // identical fixed prefixes and assessment time.
        assert_eq!(
            crate::canonical_json(first.assessment()).expect("first seal"),
            crate::canonical_json(second.assessment()).expect("second seal")
        );
        assert_eq!(
            DistinctS0S1StalenessFixture::assessment_digest(&first),
            DistinctS0S1StalenessFixture::assessment_digest(&second)
        );
        assert_eq!(
            first.assessment().record_set_digest(),
            second.assessment().record_set_digest()
        );
        assert_eq!(
            first.assessment().gluing_freshness_set_digest(),
            second.assessment().gluing_freshness_set_digest()
        );
        assert_eq!(
            first.assessment().stale_source_digest(),
            second.assessment().stale_source_digest()
        );

        // A target with no M5 attempt cannot synthesize an actual gluing
        // successor.  The stale source gluing attempt remains explicit.
        assert_eq!(first.gluing_freshness().len(), 1);
        let gluing = &first.gluing_freshness()[0];
        assert_eq!(gluing.status(), HistoricalAssessmentStatusV5::Stale);
        assert!(gluing.successor_target_attempt_ids().is_empty());
        assert!(
            gluing
                .reasons()
                .contains(&StaleReasonV5::GluingInputChanged),
            "a terminal NoM5 target must not preserve source gluing input"
        );
        assert_eq!(first.assessment().gluing_freshness_count(), 1);
        assert!(first.assessment().stale_source_count() > 0);
        assert_eq!(
            first.assessment().m5_dependent_successor_count() as usize,
            first.m5_dependent_successor_obligation_ids().len()
        );

        let exact = first.working_bytes();
        fixture
            .reduce_with_working_limit(exact)
            .expect("exact reducer working boundary");
        assert!(fixture.reduce_with_working_limit(exact - 1).is_err());
        let (external, components) = fixture
            .external_component_sum()
            .expect("external components");
        assert_eq!(
            external,
            components.into_iter().sum::<usize>(),
            "the terminal target reservation is one component; structural replay is not charged again"
        );
        assert!(
            components[4] > 0,
            "terminal+basis+inventory reservation is live"
        );
        let after_rejected_limit = fixture.reduce().expect("reducer remains output-free at -1");
        assert_eq!(
            crate::canonical_json(first.assessment()).expect("first seal after -1"),
            crate::canonical_json(after_rejected_limit.assessment()).expect("repeat seal after -1")
        );
    }
}
