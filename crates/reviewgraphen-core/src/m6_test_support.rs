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
        m6_fixture_phases_from_exact_prefixes, preservation_distinct_s0_s1_program_fixture,
        successful_m5_distinct_s0_s1_program_fixture,
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
        let phases = target.with_terminal(|_, target_actual, _| {
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
        self.target.with_terminal(|target, target_actual, _| {
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
        self.target.with_terminal(|target, target_actual, _| {
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
            .with_terminal(|target, _, _| {
                serde_json::to_value(target.plan())
                    .map_err(|error| crate::m6::M6Error::Canonical(error.to_string()))
            })
            .expect("roots-bound target plan serialization")
    }

    #[cfg(test)]
    fn external_component_sum(&self) -> M6Result<(usize, [usize; 5])> {
        self.target.with_terminal(|target, target_actual, _| {
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

pub(crate) fn with_distinct_s0_s1_persistence_fixture<R>(
    callback: impl FnOnce(
        &CompleteM5V4Fixture,
        &NoM5V5Fixture,
        &M6FixturePhases,
        &M6StalenessPhaseV5,
    ) -> M6Result<R>,
) -> M6Result<R> {
    let fixture = DistinctS0S1StalenessFixture::new()?;
    let staleness = fixture.reduce()?;
    callback(
        &fixture.source,
        &fixture.target,
        &fixture.phases,
        &staleness,
    )
}

/// The compatible M5 branch over the same distinct S0/S1 topology.  It is
/// intentionally a second opaque source fixture rather than an in-place
/// mutation of the obstruction fixture, so an E2E reducer run can only obtain
/// its candidate through normal roots-bound replay.
pub(crate) struct SuccessfulM5DistinctS0S1StalenessFixture {
    source: CompleteM5V4Fixture,
    target: NoM5V5Fixture,
    phases: M6FixturePhases,
}

pub(crate) struct PreservationDistinctS0S1StalenessFixture {
    source: CompleteM5V4Fixture,
    target: NoM5V5Fixture,
    phases: M6FixturePhases,
}

impl PreservationDistinctS0S1StalenessFixture {
    pub(crate) fn new() -> M6Result<Self> {
        Self::new_with_human_source(false)
    }

    #[cfg(test)]
    fn new_with_human_source(include_human_state: bool) -> M6Result<Self> {
        Self::new_with_stages(
            include_human_state,
            crate::event::NoM5V5FixtureTerminalStage::ReviewerCompleted,
        )
    }

    #[cfg(test)]
    fn new_with_target_stage(
        target_stage: crate::event::NoM5V5FixtureTerminalStage,
    ) -> M6Result<Self> {
        Self::new_with_stages(false, target_stage)
    }

    #[cfg(test)]
    fn new_with_stages(
        include_human_source: bool,
        target_stage: crate::event::NoM5V5FixtureTerminalStage,
    ) -> M6Result<Self> {
        let (source_program, target_program, source_bytes, target_bytes) =
            preservation_distinct_s0_s1_program_fixture()?;
        let source_run_id = crate::StableId::parse("run:m6-preservation-s0-fixture")?;
        let source = if include_human_source {
            CompleteM5V4Fixture::from_program_and_sources(
                source_program,
                source_bytes,
                source_run_id,
            )?
        } else {
            CompleteM5V4Fixture::preservation_source_from_program_and_sources(
                source_program,
                source_bytes,
                source_run_id,
            )?
        };
        let target_program_copy = target_program.clone();
        let target_run_id = crate::StableId::parse("run:m6-preservation-s1-fixture")?;
        let target_native = match target_stage {
            crate::event::NoM5V5FixtureTerminalStage::HumanFinding => {
                CompleteM5V4Fixture::from_program_and_sources(
                    target_program,
                    target_bytes,
                    target_run_id,
                )?
            }
            _ => CompleteM5V4Fixture::preservation_source_from_program_and_sources(
                target_program,
                target_bytes,
                target_run_id,
            )?,
        };
        let target = NoM5V5Fixture::from_native_m4_fixture_through(
            target_native,
            target_program_copy,
            target_stage,
        )
        .map_err(|error| crate::M6Error::Canonical(format!("native target: {error}")))?;
        let phases = target
            .with_terminal(|_, target_actual, _| {
                m6_fixture_phases_from_exact_prefixes(&source, &target, target_actual)
            })
            .map_err(|error| crate::M6Error::Canonical(format!("native target phases: {error}")))?;
        Ok(Self {
            source,
            target,
            phases,
        })
    }

    pub(crate) fn reduce(&self) -> M6Result<M6StalenessPhaseV5> {
        self.target.with_terminal(|target, target_actual, _| {
            IncrementalStalenessInputV5::new(
                self.source.log(),
                self.phases.closure(),
                self.phases.mapping(),
                self.phases.correspondence(),
                target,
            )?
            .reduce_v5(target_actual, DISTINCT_S0_S1_ASSESSMENT_TIME)
        })
    }

    #[cfg(test)]
    fn plan_with_target_suppression(
        &self,
        staleness: &M6StalenessPhaseV5,
        preservation: &crate::M6PreservationPhaseV5,
    ) -> M6Result<(Vec<crate::PartialRerunActionV5>, crate::PartialRerunPlanV5)> {
        self.target
            .seal_partial_rerun_plan_v5(staleness, preservation)
    }
}

pub(crate) fn with_preservation_s0_s1_persistence_fixture<R>(
    callback: impl FnOnce(
        &CompleteM5V4Fixture,
        &NoM5V5Fixture,
        &M6FixturePhases,
        &M6StalenessPhaseV5,
    ) -> M6Result<R>,
) -> M6Result<R> {
    let fixture = PreservationDistinctS0S1StalenessFixture::new()?;
    let staleness = fixture.reduce()?;
    callback(
        &fixture.source,
        &fixture.target,
        &fixture.phases,
        &staleness,
    )
}

impl SuccessfulM5DistinctS0S1StalenessFixture {
    pub(crate) fn new() -> M6Result<Self> {
        let (source_program, target_program, source_bytes, target_bytes) =
            successful_m5_distinct_s0_s1_program_fixture()?;
        let source = CompleteM5V4Fixture::successful_from_program_and_sources(
            source_program,
            source_bytes,
            crate::StableId::parse("run:m6-successful-s0-fixture")?,
        )?;
        let target = NoM5V5Fixture::from_program_and_sources(
            target_program,
            target_bytes,
            crate::StableId::parse("run:m6-successful-s1-fixture")?,
        )?;
        let phases = target.with_terminal(|_, target_actual, _| {
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

    pub(crate) fn reduce(&self) -> M6Result<M6StalenessPhaseV5> {
        self.target.with_terminal(|target, target_actual, _| {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        HistoricalAssessmentStatusV5, HistoricalRecordKindV5, StaleReasonV5,
        event::HistoricalSourceRecordKindV4,
    };
    use std::collections::BTreeSet;

    fn suppression_case() -> (
        PreservationDistinctS0S1StalenessFixture,
        crate::M6StalenessPhaseV5,
        crate::M6PreservationPhaseV5,
        crate::StableId,
    ) {
        suppression_case_with_stage(crate::event::NoM5V5FixtureTerminalStage::ReviewerCompleted)
    }

    fn suppression_case_with_stage(
        stage: crate::event::NoM5V5FixtureTerminalStage,
    ) -> (
        PreservationDistinctS0S1StalenessFixture,
        crate::M6StalenessPhaseV5,
        crate::M6PreservationPhaseV5,
        crate::StableId,
    ) {
        let fixture = PreservationDistinctS0S1StalenessFixture::new_with_target_stage(stage)
            .expect("preservation fixture");
        let staleness = fixture.reduce().expect("sealed preservation staleness");
        let source_obligation_id = fixture
            .source
            .passed_fixture_obligation_id()
            .expect("passed source obligation");
        let target_obligation_id = staleness
            .records()
            .iter()
            .find(|record| {
                record.source_record_kind() == crate::HistoricalRecordKindV5::Obligation
                    && record.source_record_id() == source_obligation_id
            })
            .and_then(|record| record.successor_record_ids().first())
            .cloned()
            .expect("preserved target obligation");
        let bundle = fixture
            .source
            .preservation_bundle_v5(
                fixture.target.run_id().clone(),
                fixture.phases.closure(),
                fixture.phases.mapping(),
                fixture.phases.correspondence(),
                &staleness,
                ContentHash::sha256(b"policy"),
            )
            .expect("admitted preservation bundle");
        let preservation = crate::M6PreservationPhaseV5::from_admitted_bundles(
            &staleness,
            fixture.phases.correspondence(),
            std::slice::from_ref(&bundle),
        )
        .expect("opaque preservation phase");
        (fixture, staleness, preservation, target_obligation_id)
    }

    #[test]
    fn exact_native_and_human_predecessor_closures_suppress_only_their_completed_stages() {
        let (native_fixture, native_staleness, native_preservation, target_obligation_id) =
            suppression_case_with_stage(
                crate::event::NoM5V5FixtureTerminalStage::NativeVerification,
            );
        let (native_actions, _) = native_fixture
            .plan_with_target_suppression(&native_staleness, &native_preservation)
            .expect("native-verification predecessor plan");
        assert_eq!(
            native_actions
                .iter()
                .filter(|action| action.subject_ids().contains(&target_obligation_id))
                .map(|action| action.action())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([crate::PartialRerunActionKindV5::RerunHumanDecision])
        );

        let (human_fixture, human_staleness, human_preservation, target_obligation_id) =
            suppression_case_with_stage(crate::event::NoM5V5FixtureTerminalStage::HumanFinding);
        let (human_actions, _) = human_fixture
            .plan_with_target_suppression(&human_staleness, &human_preservation)
            .expect("human predecessor plan");
        assert!(
            human_actions
                .iter()
                .all(|action| !action.subject_ids().contains(&target_obligation_id)),
            "an exact current target human closure suppresses all four subject stages"
        );
    }

    #[test]
    fn target_predecessor_suppression_emits_exact_subject_action_set() {
        let (fixture, staleness, preservation, target_obligation_id) = suppression_case();
        let (unsuppressed, _) = staleness
            .plan_partial_rerun_without_target_suppression_v5(&preservation)
            .expect("no-suppression public plan");
        let unsuppressed_subject = unsuppressed
            .iter()
            .filter(|action| action.subject_ids().contains(&target_obligation_id))
            .map(|action| action.action())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            unsuppressed_subject,
            BTreeSet::from([
                crate::PartialRerunActionKindV5::ReprojectContext,
                crate::PartialRerunActionKindV5::RerunReviewer,
                crate::PartialRerunActionKindV5::RerunVerifier,
                crate::PartialRerunActionKindV5::RerunHumanDecision,
            ])
        );

        let (actions, plan) = fixture
            .plan_with_target_suppression(&staleness, &preservation)
            .expect("suppression-aware plan");
        let subject = actions
            .iter()
            .filter(|action| action.subject_ids().contains(&target_obligation_id))
            .collect::<Vec<_>>();
        assert_eq!(
            subject
                .iter()
                .map(|action| action.action())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                crate::PartialRerunActionKindV5::RerunVerifier,
                crate::PartialRerunActionKindV5::RerunHumanDecision,
            ])
        );
        let verifier = subject
            .iter()
            .find(|action| action.action() == crate::PartialRerunActionKindV5::RerunVerifier)
            .expect("remaining verifier");
        assert_eq!(verifier.prerequisites().len(), 2);
        assert!(verifier.prerequisites().iter().all(|value| matches!(
            value,
            crate::ActionPrerequisiteV5::ExistingTargetRecord { .. }
        )));
        assert_eq!(
            plan.action_count(),
            u64::try_from(actions.len()).expect("bounded action count")
        );
    }

    #[test]
    fn target_predecessor_suppression_rejects_zero_and_two_claims_before_completed_mismatch() {
        let (fixture, staleness, preservation, target_obligation_id) = suppression_case();
        for observed in [0_usize, 2] {
            let error = fixture
                .target
                .with_terminal(|_, _, suppression| {
                    let mut mutated = suppression.clone();
                    mutated.set_reviewer_claim_count_for_test(&target_obligation_id, observed);
                    mutated.set_reviewer_closure_exact_for_test(&target_obligation_id, false);
                    staleness
                        .plan_partial_rerun_with_target_suppression_v5(&preservation, &mutated)
                        .map(|_| ())
                })
                .expect_err("unsupported claim cardinality");
            assert!(matches!(
                error,
                crate::M6Error::M6ClaimCardinalityUnsupported {
                    observed: actual,
                    ..
                } if actual == observed
            ));
        }
    }

    #[test]
    fn target_predecessor_suppression_refuses_completed_closure_mismatch_without_reopening() {
        let (fixture, staleness, preservation, target_obligation_id) = suppression_case();
        for duplicate_envelope in [false, true] {
            let error = fixture
                .target
                .with_terminal(|_, _, suppression| {
                    let mut mutated = suppression.clone();
                    if duplicate_envelope {
                        mutated.set_duplicate_envelope_for_test(&target_obligation_id, true);
                    } else {
                        mutated.set_reviewer_closure_exact_for_test(&target_obligation_id, false);
                    }
                    staleness
                        .plan_partial_rerun_with_target_suppression_v5(&preservation, &mutated)
                        .map(|_| ())
                })
                .expect_err("completed mismatch must refuse rather than reopen");
            assert_eq!(
                error,
                crate::M6Error::CompletedReviewerClosureMismatch {
                    obligation_id: target_obligation_id.clone(),
                }
            );
        }
    }

    #[test]
    fn target_predecessor_suppression_fails_closed_on_cas_bound_witness_mutation() {
        let (fixture, staleness, preservation, target_obligation_id) = suppression_case();
        let error = fixture
            .target
            .with_terminal(|_, _, suppression| {
                let mut mutated = suppression.clone();
                mutated.corrupt_witness_body_hash_for_test(&target_obligation_id);
                staleness
                    .plan_partial_rerun_with_target_suppression_v5(&preservation, &mutated)
                    .map(|_| ())
            })
            .expect_err("unsealed CAS witness mutation");
        assert!(matches!(
            error,
            crate::M6Error::Canonical(message)
                if message.contains("target suppression projection seal mismatch")
        ));
    }

    #[test]
    fn actual_reducer_keeps_shared_record_obligation_cones_isolated() {
        let fixture =
            PreservationDistinctS0S1StalenessFixture::new().expect("preservation fixture");
        let phase = fixture.reduce().expect("phase");
        let preserved_source_obligation_id = fixture
            .source
            .passed_fixture_obligation_id()
            .expect("preserved source obligation");
        let preserved_assessment = phase
            .records()
            .iter()
            .find(|record| {
                record.source_record_kind() == crate::HistoricalRecordKindV5::Obligation
                    && record.source_record_id() == preserved_source_obligation_id
            })
            .expect("preserved obligation assessment");
        assert_eq!(
            preserved_assessment.status(),
            crate::HistoricalAssessmentStatusV5::StructurallyPreserved
        );
        let preserved_target_obligation_id = preserved_assessment
            .successor_record_ids()
            .first()
            .expect("preserved target obligation");
        let bundle = fixture
            .source
            .preservation_bundle_v5(
                fixture.target.run_id().clone(),
                fixture.phases.closure(),
                fixture.phases.mapping(),
                fixture.phases.correspondence(),
                &phase,
                ContentHash::sha256(b"policy"),
            )
            .expect("admitted preservation bundle");
        let preservation = crate::M6PreservationPhaseV5::from_admitted_bundles(
            &phase,
            fixture.phases.correspondence(),
            std::slice::from_ref(&bundle),
        )
        .expect("opaque preservation phase");
        let (actions, _) = phase
            .plan_partial_rerun_without_target_suppression_v5(&preservation)
            .expect("plan");
        let contexts = actions
            .iter()
            .filter(|value| value.action() == crate::PartialRerunActionKindV5::ReprojectContext)
            .collect::<Vec<_>>();
        let preserved_o2 = contexts
            .iter()
            .copied()
            .find(|action| {
                action
                    .subject_ids()
                    .contains(preserved_target_obligation_id)
            })
            .expect("admitted preserved O2 cone");
        let (stale_o1, stale_o1_witness_id) = phase
            .records()
            .iter()
            .filter(|record| {
                record.source_record_kind() == crate::HistoricalRecordKindV5::Obligation
                    && record.status() == crate::HistoricalAssessmentStatusV5::Stale
            })
            .find_map(|record| {
                contexts
                    .iter()
                    .copied()
                    .find(|action| {
                        !action
                            .subject_ids()
                            .contains(preserved_target_obligation_id)
                            && action
                                .stale_source_record_ids()
                                .contains(record.source_record_id())
                    })
                    .filter(|_| {
                        !preserved_o2
                            .stale_source_record_ids()
                            .contains(record.source_record_id())
                    })
                    .map(|action| (action, record.source_record_id().clone()))
            })
            .expect("actual stale O1 witness outside preserved O2 cone");
        assert!(
            stale_o1
                .stale_source_record_ids()
                .contains(&stale_o1_witness_id)
        );
        assert!(
            !preserved_o2
                .stale_source_record_ids()
                .contains(&stale_o1_witness_id)
        );
        for action in &actions {
            if action.subject_ids() == preserved_o2.subject_ids() {
                assert!(
                    !action
                        .stale_source_record_ids()
                        .contains(&stale_o1_witness_id)
                );
            }
        }
    }

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

    #[test]
    fn successful_m5_candidate_reduces_from_roots_bound_distinct_s0_to_no_m5_s1() {
        let fixture = SuccessfulM5DistinctS0S1StalenessFixture::new()
            .expect("admitted successful-candidate E2E fixture");
        let first = fixture.reduce().expect("successful S0 -> S1 reducer");
        let second = fixture
            .reduce()
            .expect("repeat successful S0 -> S1 reducer");

        let mut source_kinds = BTreeSet::new();
        let mut attempt_id = None;
        let mut candidate_id = None;
        fixture
            .source
            .log()
            .historical_prefix_projection_v4()
            .expect("successful source historical projection")
            .try_visit_records(|record| {
                source_kinds.insert(record.kind());
                match record.kind() {
                    HistoricalSourceRecordKindV4::GluingAttempt => {
                        attempt_id = Some(record.id().clone());
                    }
                    HistoricalSourceRecordKindV4::GlobalCandidate => {
                        candidate_id = Some(record.id().clone());
                    }
                    _ => {}
                }
                Ok::<(), crate::DomainError>(())
            })
            .expect("successful source historical traversal");
        let attempt_id = attempt_id.expect("one source gluing attempt");
        assert!(matches!(
            fixture.source.completed().result(),
            crate::GluingResultV4::Candidate
                | crate::GluingResultV4::GluedWithQualification
                | crate::GluingResultV4::Glued
        ));
        let candidate_id = candidate_id.expect("one source global candidate");
        assert_eq!(source_kinds.len(), 20, "source kinds: {source_kinds:?}");
        assert!(source_kinds.contains(&HistoricalSourceRecordKindV4::GlobalCandidate));
        assert!(
            !source_kinds.contains(&HistoricalSourceRecordKindV4::GluingObstruction),
            "a successful candidate and obstruction are mutually exclusive"
        );

        let attempt = first
            .records()
            .iter()
            .find(|record| {
                record.source_record_kind() == HistoricalRecordKindV5::GluingAttempt
                    && record.source_record_id() == &attempt_id
            })
            .expect("assessed source gluing attempt");
        let candidate = first
            .records()
            .iter()
            .find(|record| {
                record.source_record_kind() == HistoricalRecordKindV5::GlobalCandidate
                    && record.source_record_id() == &candidate_id
            })
            .expect("assessed source global candidate");
        assert!(attempt.successor_record_ids().is_empty());
        assert!(
            attempt.dependency_source_ids().contains(&candidate_id),
            "the attempt's transitive audit closure must retain its candidate"
        );
        for record in [attempt, candidate] {
            assert!(record.status() == HistoricalAssessmentStatusV5::Stale);
            assert!(!record.mapping_ids().is_empty());
            assert!(!record.correspondence_entry_ids().is_empty());
            assert!(!record.dependency_source_ids().is_empty());
        }

        assert_eq!(first.gluing_freshness().len(), 1);
        let gluing = &first.gluing_freshness()[0];
        assert_eq!(gluing.status(), HistoricalAssessmentStatusV5::Stale);
        assert!(gluing.successor_target_attempt_ids().is_empty());
        assert!(
            gluing
                .reasons()
                .contains(&StaleReasonV5::GluingInputChanged)
        );
        assert_eq!(first.assessment().gluing_freshness_count(), 1);
        assert_eq!(
            crate::canonical_json(first.assessment()).expect("first successful seal"),
            crate::canonical_json(second.assessment()).expect("repeat successful seal")
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
    }

    #[test]
    fn preservation_eligibility_refuses_a_target_run_coordinate_mutation_typed() {
        let fixture = DistinctS0S1StalenessFixture::new().expect("admitted E2E fixture");
        let staleness = fixture.reduce().expect("sealed staleness");
        let result = fixture.source.preservation_bundle_v5(
            crate::StableId::parse("run:wrong-preservation-target").expect("test run ID"),
            fixture.phases.closure(),
            fixture.phases.mapping(),
            fixture.phases.correspondence(),
            &staleness,
            ContentHash::sha256(b"policy"),
        );
        assert!(matches!(
            result,
            Err(crate::M6Error::PreservationUnsupported(_))
        ));
    }

    #[test]
    fn preservation_eligibility_uses_real_passed_fixture_closure() {
        let fixture =
            PreservationDistinctS0S1StalenessFixture::new().expect("admitted preservation fixture");
        let staleness = fixture.reduce().expect("sealed preservation staleness");
        let source_obligation_id = fixture
            .source
            .passed_fixture_obligation_id()
            .expect("fixture obligation");
        let obligation_assessment = staleness
            .records()
            .iter()
            .find(|record| {
                record.source_record_kind() == crate::HistoricalRecordKindV5::Obligation
                    && record.source_record_id() == source_obligation_id
            })
            .expect("actual payment obligation assessment");
        assert_eq!(
            obligation_assessment.status(),
            crate::HistoricalAssessmentStatusV5::StructurallyPreserved
        );
        assert!(obligation_assessment.reasons().is_empty());
        assert_eq!(obligation_assessment.successor_record_ids().len(), 1);
        let target_obligation_id = obligation_assessment
            .successor_record_ids()
            .first()
            .expect("sole target obligation");
        assert!(
            staleness
                .preservation_candidate_obligation_ids()
                .contains(target_obligation_id)
        );
        assert!(
            staleness
                .is_sealed_preservation_candidate(target_obligation_id)
                .expect("candidate digest recheck")
        );
        let bundle = fixture
            .source
            .preservation_bundle_v5(
                fixture.target.run_id().clone(),
                fixture.phases.closure(),
                fixture.phases.mapping(),
                fixture.phases.correspondence(),
                &staleness,
                ContentHash::sha256(b"policy"),
            )
            .expect("reachable exact preservation eligibility");
        let source_verification = fixture
            .source
            .passed_fixture_verification()
            .expect("real passed source verification");
        assert_eq!(
            bundle.evidence().source_verification_id(),
            source_verification.id()
        );
        assert_eq!(
            bundle.evidence().source_claim_id(),
            source_verification.claim_id()
        );
        assert_eq!(
            bundle.evidence().source_evidence_ids(),
            &source_verification
                .evidence_ids()
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>()
        );
        let input = crate::PreservationInputV1::from_json_bytes(bundle.input_bytes())
            .expect("strict preservation input");
        assert_eq!(input.source_verification_id(), source_verification.id());

        let preservation = crate::M6PreservationPhaseV5::from_admitted_bundles(
            &staleness,
            fixture.phases.correspondence(),
            std::slice::from_ref(&bundle),
        )
        .expect("opaque admitted preservation phase");
        let (actions, plan) = staleness
            .plan_partial_rerun_without_target_suppression_v5(&preservation)
            .expect("phase-bound partial rerun plan");
        assert!(
            actions
                .iter()
                .any(|action| action.subject_ids().contains(target_obligation_id))
        );
        assert!(actions.iter().any(|action| {
            action.subject_ids().contains(target_obligation_id)
                && action.action() == crate::PartialRerunActionKindV5::RerunHumanDecision
        }));
        assert_eq!(plan.preservation_verification_count(), 1);
        let target_plan_id = fixture
            .target
            .with_terminal(|target, _, _| Ok(target.plan().id().clone()))
            .expect("target plan ID");
        assert_eq!(plan.target_plan_id(), &target_plan_id);

        let (suppressed_actions, suppressed_plan) = fixture
            .plan_with_target_suppression(&staleness, &preservation)
            .expect("target-predecessor suppression plan");
        let subject_actions = suppressed_actions
            .iter()
            .filter(|action| action.subject_ids().contains(target_obligation_id))
            .collect::<Vec<_>>();
        assert_eq!(subject_actions.len(), 2);
        assert_eq!(
            subject_actions
                .iter()
                .map(|action| action.action())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                crate::PartialRerunActionKindV5::RerunVerifier,
                crate::PartialRerunActionKindV5::RerunHumanDecision,
            ])
        );
        let verifier = subject_actions
            .iter()
            .find(|action| action.action() == crate::PartialRerunActionKindV5::RerunVerifier)
            .expect("remaining verifier");
        assert_eq!(verifier.prerequisites().len(), 2);
        assert!(verifier.prerequisites().iter().all(|value| matches!(
            value,
            crate::ActionPrerequisiteV5::ExistingTargetRecord { .. }
        )));
        assert!(verifier.prerequisites().iter().any(|value| matches!(
            value,
            crate::ActionPrerequisiteV5::ExistingTargetRecord { record_id, .. }
                if record_id.kind() == "execution"
        )));
        assert!(verifier.prerequisites().iter().any(|value| matches!(
            value,
            crate::ActionPrerequisiteV5::ExistingTargetRecord { record_id, .. }
                if record_id.kind() == "claim"
        )));
        let expected = actions
            .iter()
            .filter(|action| {
                let subject_has_existing_reviewer_witness =
                    suppressed_actions.iter().any(|candidate| {
                        candidate.subject_ids() == action.subject_ids()
                        && candidate.action() == crate::PartialRerunActionKindV5::RerunVerifier
                        && candidate.prerequisites().iter().any(|value| matches!(
                            value,
                            crate::ActionPrerequisiteV5::ExistingTargetRecord { record_id, .. }
                                if record_id.kind() == "execution"
                        ))
                    });
                !(subject_has_existing_reviewer_witness
                    && matches!(
                        action.action(),
                        crate::PartialRerunActionKindV5::ReprojectContext
                            | crate::PartialRerunActionKindV5::RerunReviewer
                    ))
            })
            .map(|action| (action.subject_ids().clone(), action.action()))
            .collect::<BTreeSet<_>>();
        let observed = suppressed_actions
            .iter()
            .map(|action| (action.subject_ids().clone(), action.action()))
            .collect::<BTreeSet<_>>();
        assert_eq!(observed, expected);
        assert_eq!(
            suppressed_plan.action_count(),
            u64::try_from(observed.len()).expect("action count")
        );
    }

    #[test]
    fn preservation_eligibility_rejects_human_and_every_fake_reviewer_tuple_mutation() {
        let mut human = PreservationDistinctS0S1StalenessFixture::new_with_human_source(true)
            .expect("human source fixture");
        let human_staleness = human.reduce().expect("human source staleness");
        assert!(matches!(
            human.source.preservation_bundle_v5(
                human.target.run_id().clone(),
                human.phases.closure(),
                human.phases.mapping(),
                human.phases.correspondence(),
                &human_staleness,
                ContentHash::sha256(b"policy"),
            ),
            Err(crate::M6Error::PreservationUnsupported(_))
        ));
        let historical_counts = human.source.fixture_human_history_counts_for_test();
        assert!(historical_counts.0 > 0 && historical_counts.1 > 0);
        for (active_decision, current_finding) in [(true, false), (false, true)] {
            human
                .source
                .configure_active_fixture_human_pointers_for_test(active_decision, current_finding);
            assert!(matches!(
                human.source.preservation_bundle_v5(
                    human.target.run_id().clone(),
                    human.phases.closure(),
                    human.phases.mapping(),
                    human.phases.correspondence(),
                    &human_staleness,
                    ContentHash::sha256(b"policy"),
                ),
                Err(crate::M6Error::PreservationUnsupported(_))
            ));
        }
        human.source.clear_active_fixture_human_pointers_for_test();
        human
            .source
            .preservation_bundle_v5(
                human.target.run_id().clone(),
                human.phases.closure(),
                human.phases.mapping(),
                human.phases.correspondence(),
                &human_staleness,
                ContentHash::sha256(b"policy"),
            )
            .expect("inactive historical human records do not block preservation");

        let mut reviewer =
            PreservationDistinctS0S1StalenessFixture::new().expect("reviewer mutation fixture");
        let reviewer_staleness = reviewer.reduce().expect("reviewer mutation staleness");
        for field in [
            "provider",
            "model",
            "model_revision",
            "reviewer_kind",
            "reviewer_id",
            "system_prompt",
            "prompt",
            "inference",
            "tool_policy",
            "tool_count",
            "outcome",
            "raw_registration",
        ] {
            let original = reviewer.source.corrupt_fixture_reviewer_for_test(field);
            assert!(matches!(
                reviewer.source.preservation_bundle_v5(
                    reviewer.target.run_id().clone(),
                    reviewer.phases.closure(),
                    reviewer.phases.mapping(),
                    reviewer.phases.correspondence(),
                    &reviewer_staleness,
                    ContentHash::sha256(b"policy"),
                ),
                Err(crate::M6Error::PreservationUnsupported(_))
            ));
            reviewer.source.restore_fixture_reviewer_for_test(original);
        }
    }

    #[test]
    fn preservation_eligibility_rejects_harness_anchor_and_dependency_mutations() {
        let mut harness =
            PreservationDistinctS0S1StalenessFixture::new().expect("harness mutation fixture");
        let harness_staleness = harness.reduce().expect("harness mutation staleness");
        for field in [
            "harness",
            "harness_revision",
            "harness_source_hash",
            "test",
            "repository",
            "repository_source_hash",
            "descriptor",
            "procedure",
            "witness_hash",
            "witness_media",
            "witness_size",
            "witness_sensitivity",
            "policy",
            "run",
            "genesis",
            "snapshot",
            "universe",
            "property",
            "claim",
            "claim_body",
            "cas",
        ] {
            let original = harness.source.corrupt_fixture_harness_for_test(field);
            assert!(matches!(
                harness.source.preservation_bundle_v5(
                    harness.target.run_id().clone(),
                    harness.phases.closure(),
                    harness.phases.mapping(),
                    harness.phases.correspondence(),
                    &harness_staleness,
                    ContentHash::sha256(b"policy"),
                ),
                Err(crate::M6Error::PreservationUnsupported(_))
            ));
            harness.source.restore_fixture_harness_for_test(original);
        }

        let mut fixture =
            PreservationDistinctS0S1StalenessFixture::new().expect("dependency mutation fixture");
        let staleness = fixture.reduce().expect("dependency mutation staleness");
        let source_obligation_id = fixture
            .source
            .passed_fixture_obligation_id()
            .expect("fixture obligation");
        let dependency_mapping_id = staleness
            .records()
            .iter()
            .find(|record| {
                record.source_record_kind() == crate::HistoricalRecordKindV5::Obligation
                    && record.source_record_id() == source_obligation_id
            })
            .expect("payment assessment")
            .mapping_ids()
            .iter()
            .find(|mapping_id| {
                fixture
                    .phases
                    .mapping()
                    .mappings()
                    .iter()
                    .find(|mapping| mapping.id() == *mapping_id)
                    .is_some_and(|mapping| {
                        mapping.object_kind() != crate::ProgramObjectKindV5::Snapshot
                    })
            })
            .expect("non-coordinate dependency mapping")
            .clone();
        for status in [
            crate::MappingStatusV5::Modified,
            crate::MappingStatusV5::Split,
            crate::MappingStatusV5::Merged,
            crate::MappingStatusV5::Unresolved,
        ] {
            fixture
                .phases
                .corrupt_mapping_status_for_test(&dependency_mapping_id, status);
            assert!(matches!(
                fixture.source.preservation_bundle_v5(
                    fixture.target.run_id().clone(),
                    fixture.phases.closure(),
                    fixture.phases.mapping(),
                    fixture.phases.correspondence(),
                    &staleness,
                    ContentHash::sha256(b"policy"),
                ),
                Err(crate::M6Error::PreservationUnsupported(_))
            ));
        }

        let stale_fixture = PreservationDistinctS0S1StalenessFixture::new()
            .expect("stale assessment mutation fixture");
        let mut stale_assessment = stale_fixture.reduce().expect("candidate staleness");
        let stale_source_obligation_id = stale_fixture
            .source
            .passed_fixture_obligation_id()
            .expect("fixture obligation")
            .clone();
        stale_assessment.corrupt_obligation_assessment_stale_for_test(&stale_source_obligation_id);
        assert!(matches!(
            stale_fixture.source.preservation_bundle_v5(
                stale_fixture.target.run_id().clone(),
                stale_fixture.phases.closure(),
                stale_fixture.phases.mapping(),
                stale_fixture.phases.correspondence(),
                &stale_assessment,
                ContentHash::sha256(b"policy"),
            ),
            Err(crate::M6Error::PreservationUnsupported(_))
        ));
    }
}
