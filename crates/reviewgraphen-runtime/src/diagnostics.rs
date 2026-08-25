//! Non-canonical operational stage observations for generic review.
//!
//! This API deliberately exposes events rather than time.  The CLI supplies
//! the monotonic clock and owns serialization of the separate diagnostic.

use reviewgraphen_core::{DomainError, canonical_json};
use serde::Serialize;

pub const GENERIC_REVIEW_DIAGNOSTICS_SCHEMA: &str = "reviewgraphen.generic_review_diagnostics.v1";
pub const GENERIC_REVIEW_STAGE_OBSERVER_API: &str =
    "reviewgraphen.runtime.generic_stage_observer@1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GenericReviewStage {
    Ingest,
    Synthesize,
    Context,
    Observer,
    Report,
    ArtifactWrite,
}

impl GenericReviewStage {
    pub const ORDERED: [Self; 6] = [
        Self::Ingest,
        Self::Synthesize,
        Self::Context,
        Self::Observer,
        Self::Report,
        Self::ArtifactWrite,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GenericReviewStageEvent {
    Begin(GenericReviewStage),
    Completed(GenericReviewStage),
    Failed(GenericReviewStage),
    Skipped(GenericReviewStage),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GenericReviewStageStatus {
    Completed,
    Failed,
    Skipped,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GenericReviewStageDiagnostic {
    pub stage: GenericReviewStage,
    pub status: GenericReviewStageStatus,
    pub elapsed_microseconds: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GenericReviewDiagnosticsV1 {
    pub schema: &'static str,
    pub observer_api: &'static str,
    pub request_id: Option<String>,
    pub run_id: Option<String>,
    pub terminal_code: String,
    pub stages: Vec<GenericReviewStageDiagnostic>,
}

impl GenericReviewDiagnosticsV1 {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, DomainError> {
        canonical_json(self)
    }
}

pub trait MonotonicMicrosecondClock {
    fn now_microseconds(&mut self) -> u64;
}

/// Converts effect events into the non-canonical diagnostic DTO.  A clock is
/// read only at event boundaries; absolute clock values are never retained.
pub struct GenericReviewDiagnosticsCollector<C> {
    clock: C,
    active: Option<(GenericReviewStage, u64)>,
    rows: Vec<GenericReviewStageDiagnostic>,
}

impl<C: MonotonicMicrosecondClock> GenericReviewDiagnosticsCollector<C> {
    pub fn new(clock: C) -> Self {
        Self {
            clock,
            active: None,
            rows: Vec::new(),
        }
    }

    pub fn finish(
        self,
        request_id: Option<String>,
        run_id: Option<String>,
        terminal_code: impl Into<String>,
    ) -> Result<GenericReviewDiagnosticsV1, &'static str> {
        if self.active.is_some()
            || self.rows.len() != GenericReviewStage::ORDERED.len()
            || self
                .rows
                .iter()
                .map(|row| row.stage)
                .ne(GenericReviewStage::ORDERED)
        {
            return Err("incomplete generic review diagnostic stage sequence");
        }
        Ok(GenericReviewDiagnosticsV1 {
            schema: GENERIC_REVIEW_DIAGNOSTICS_SCHEMA,
            observer_api: GENERIC_REVIEW_STAGE_OBSERVER_API,
            request_id,
            run_id,
            terminal_code: terminal_code.into(),
            stages: self.rows,
        })
    }
}

impl<C: MonotonicMicrosecondClock> GenericReviewStageObserver
    for GenericReviewDiagnosticsCollector<C>
{
    fn observe(&mut self, event: GenericReviewStageEvent) {
        match event {
            GenericReviewStageEvent::Begin(stage) => {
                assert!(self.active.is_none(), "diagnostic stage overlap");
                self.active = Some((stage, self.clock.now_microseconds()));
            }
            GenericReviewStageEvent::Completed(stage) | GenericReviewStageEvent::Failed(stage) => {
                let (active, began) = self.active.take().expect("terminal without stage begin");
                assert_eq!(active, stage, "diagnostic stage terminal mismatch");
                let ended = self.clock.now_microseconds();
                let status = if matches!(event, GenericReviewStageEvent::Completed(_)) {
                    GenericReviewStageStatus::Completed
                } else {
                    GenericReviewStageStatus::Failed
                };
                self.rows.push(GenericReviewStageDiagnostic {
                    stage,
                    status,
                    elapsed_microseconds: ended.checked_sub(began).expect("clock is monotonic"),
                });
            }
            GenericReviewStageEvent::Skipped(stage) => {
                assert!(self.active.is_none(), "skip while stage active");
                self.rows.push(GenericReviewStageDiagnostic {
                    stage,
                    status: GenericReviewStageStatus::Skipped,
                    elapsed_microseconds: 0,
                });
            }
        }
    }
}

/// Injected effect seam used by the runtime and by the CLI coordinator.
///
/// Implementations must not influence review behavior or canonical records.
pub trait GenericReviewStageObserver {
    fn observe(&mut self, event: GenericReviewStageEvent);
}

#[derive(Default)]
pub struct NoopGenericReviewStageObserver;

impl GenericReviewStageObserver for NoopGenericReviewStageObserver {
    fn observe(&mut self, _event: GenericReviewStageEvent) {}
}

/// Drives the closed six-stage failure/skipping protocol.  Production
/// coordinators emit the same events around their actual stage operations.
pub fn observe_stage_sequence<E>(
    observer: &mut impl GenericReviewStageObserver,
    mut operation: impl FnMut(GenericReviewStage) -> Result<(), E>,
) -> Result<(), E> {
    for stage in GenericReviewStage::ORDERED {
        observer.observe(GenericReviewStageEvent::Begin(stage));
        match operation(stage) {
            Ok(()) => observer.observe(GenericReviewStageEvent::Completed(stage)),
            Err(error) => {
                observer.observe(GenericReviewStageEvent::Failed(stage));
                // Finish emitting deterministic skipped rows before returning.
                for later in GenericReviewStage::ORDERED
                    .into_iter()
                    .skip_while(|candidate| *candidate != stage)
                    .skip(1)
                {
                    observer.observe(GenericReviewStageEvent::Skipped(later));
                }
                return Err(error);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Trace(Vec<GenericReviewStageEvent>);
    impl GenericReviewStageObserver for Trace {
        fn observe(&mut self, event: GenericReviewStageEvent) {
            self.0.push(event);
        }
    }

    struct FakeClock(std::collections::VecDeque<u64>);
    impl MonotonicMicrosecondClock for FakeClock {
        fn now_microseconds(&mut self) -> u64 {
            self.0.pop_front().expect("literal fake-clock schedule")
        }
    }

    #[test]
    fn success_has_exact_six_begin_terminal_pairs() {
        let mut trace = Trace::default();
        observe_stage_sequence(&mut trace, |_| Ok::<_, ()>(())).unwrap();
        assert_eq!(trace.0.len(), 12);
        for (index, stage) in GenericReviewStage::ORDERED.into_iter().enumerate() {
            assert_eq!(trace.0[index * 2], GenericReviewStageEvent::Begin(stage));
            assert_eq!(
                trace.0[index * 2 + 1],
                GenericReviewStageEvent::Completed(stage)
            );
        }
    }

    #[test]
    fn failure_has_one_failed_and_later_stages_are_skipped() {
        let mut trace = Trace::default();
        assert!(
            observe_stage_sequence(&mut trace, |stage| {
                if stage == GenericReviewStage::Context {
                    Err(())
                } else {
                    Ok(())
                }
            })
            .is_err()
        );
        assert_eq!(
            trace.0,
            vec![
                GenericReviewStageEvent::Begin(GenericReviewStage::Ingest),
                GenericReviewStageEvent::Completed(GenericReviewStage::Ingest),
                GenericReviewStageEvent::Begin(GenericReviewStage::Synthesize),
                GenericReviewStageEvent::Completed(GenericReviewStage::Synthesize),
                GenericReviewStageEvent::Begin(GenericReviewStage::Context),
                GenericReviewStageEvent::Failed(GenericReviewStage::Context),
                GenericReviewStageEvent::Skipped(GenericReviewStage::Observer),
                GenericReviewStageEvent::Skipped(GenericReviewStage::Report),
                GenericReviewStageEvent::Skipped(GenericReviewStage::ArtifactWrite),
            ]
        );
    }

    #[test]
    fn fake_clock_durations_are_separate_and_exact() {
        let schedule = (0..12).map(|tick| tick * 10).collect();
        let mut collector = GenericReviewDiagnosticsCollector::new(FakeClock(schedule));
        observe_stage_sequence(&mut collector, |_| Ok::<_, ()>(())).unwrap();
        let diagnostic = collector
            .finish(Some("request:x".into()), Some("run:y".into()), "success")
            .unwrap();
        assert!(
            diagnostic
                .stages
                .iter()
                .all(|row| row.elapsed_microseconds == 10)
        );
        let bytes = diagnostic.canonical_bytes().unwrap();
        assert!(
            String::from_utf8(bytes)
                .unwrap()
                .contains(GENERIC_REVIEW_STAGE_OBSERVER_API)
        );
    }
}
