//! Closed M5 double-submit gluing orchestration.
//!
//! This module deliberately does not construct program facts, M4 authority,
//! assignments, descriptors, or trust roots.  Its caller must receive the
//! two one-shot [`TrustedGluingInputSourceV4`] values from a source-bound
//! Core/Store profile seam.  Runtime then supplies the only legal ordering:
//! payment descriptor, UI-event descriptor, and one atomic gluing bundle.
//! In particular, reviewer prose and arbitrary workspace input never enter
//! this API.

use reviewgraphen_core::{
    AssignmentCompatibilityV4, AuthorityReplayBasisV4, AuthorityTrustRootsV4,
    GluingInputDescriptorV4, GluingResultV4, M5CompletedGluingProfileV4,
    M5DoubleSubmitAssignmentsV4, M5GluingProfileInputV4, TrustedGluingInputSourceV4,
};
use reviewgraphen_store::{
    EventJournal, GluingInputPublicationV4, JournalError, M5GluingProfileRecoveryV4,
    M5GluingProfileSessionV4, RecoveryKeyV4, RecoveryProvenanceV4, RecoveryReceiptV4,
    ReplayedV4RunSession, V4GluingBundleAppendReceipt,
};
use thiserror::Error;

/// The two closed profile inputs.  The fields are intentionally private so
/// callers cannot reorder a descriptor after it has been paired with its
/// one-shot trusted source.
pub struct DoubleSubmitGluingInputsV4 {
    payment: Option<GluingInputInputV4>,
    ui_event: Option<GluingInputInputV4>,
}

struct GluingInputInputV4 {
    source: TrustedGluingInputSourceV4,
    descriptor: GluingInputDescriptorV4,
}

impl DoubleSubmitGluingInputsV4 {
    /// Accepts only the two fixed context slots.  Core and Store repeat the
    /// complete source/binding/descriptor validation at admission time.
    pub fn new(
        payment_source: TrustedGluingInputSourceV4,
        payment_descriptor: GluingInputDescriptorV4,
        ui_event_source: TrustedGluingInputSourceV4,
        ui_event_descriptor: GluingInputDescriptorV4,
    ) -> Result<Self, M5RuntimeError> {
        if payment_descriptor.context_id().as_str()
            != reviewgraphen_core::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID
            || payment_source.context_id().as_str()
                != reviewgraphen_core::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID
            || ui_event_descriptor.context_id().as_str()
                != reviewgraphen_core::DOUBLE_SUBMIT_UI_CONTEXT_ID
            || ui_event_source.context_id().as_str()
                != reviewgraphen_core::DOUBLE_SUBMIT_UI_CONTEXT_ID
        {
            return Err(M5RuntimeError::InputContext);
        }
        Ok(Self {
            payment: Some(GluingInputInputV4 {
                source: payment_source,
                descriptor: payment_descriptor,
            }),
            ui_event: Some(GluingInputInputV4 {
                source: ui_event_source,
                descriptor: ui_event_descriptor,
            }),
        })
    }

    /// Converts the exact remaining suffix issued by Core's read-only profile
    /// inspection. A durable 0/1/2 input prefix becomes a 2/1/0 source
    /// suffix respectively; Runtime never recreates a confirmed source.
    pub fn from_profile_inputs(
        inputs: Vec<M5GluingProfileInputV4>,
    ) -> Result<Self, M5RuntimeError> {
        if inputs.len() > 2 {
            return Err(M5RuntimeError::InputContext);
        }
        let mut payment = None;
        let mut ui_event = None;
        for input in inputs {
            let descriptor = GluingInputDescriptorV4::from_json_bytes(input.descriptor_bytes())?;
            let is_payment = descriptor.context_id().as_str()
                == reviewgraphen_core::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID;
            let is_ui_event =
                descriptor.context_id().as_str() == reviewgraphen_core::DOUBLE_SUBMIT_UI_CONTEXT_ID;
            let slot = GluingInputInputV4 {
                source: input.into_trusted_source(),
                descriptor,
            };
            match (is_payment, is_ui_event) {
                (true, false) if payment.is_none() => {
                    payment = Some(slot);
                }
                (false, true) if ui_event.is_none() => {
                    ui_event = Some(slot);
                }
                _ => return Err(M5RuntimeError::InputContext),
            }
        }
        // The only legal nonempty suffixes are payment→ui and ui. Core has
        // already proved the durable prefix; repeat the shape check here so a
        // future host material cannot reorder one-shot capabilities.
        if payment.is_some() && ui_event.is_none() {
            return Err(M5RuntimeError::InputContext);
        }
        Ok(Self { payment, ui_event })
    }

    fn compatibility(&self) -> Option<AssignmentCompatibilityV4> {
        Some(
            self.payment
                .as_ref()?
                .descriptor
                .assignment_value()
                .compatibility(self.ui_event.as_ref()?.descriptor.assignment_value()),
        )
    }
}

/// Descriptive records from a completed M5 operation.  None of these values
/// are append authority; retries must obtain fresh host-issued inputs and
/// reopen/recover the Store session.
pub struct DoubleSubmitGluingReceiptV4 {
    pub payment: Option<GluingInputPublicationV4>,
    pub ui_event: Option<GluingInputPublicationV4>,
    pub bundle: V4GluingBundleAppendReceipt,
}

/// Receipt from the restart-safe Store profile seam. The input publication
/// count is 0, 1, or 2, matching the legal durable descriptor prefix; it is
/// not coverage or verification progress.
pub struct DoubleSubmitGluingProfileReceiptV4 {
    pub published_input_count: usize,
    pub bundle: V4GluingBundleAppendReceipt,
}

/// Closed resolution of profile-specific recovery. The completed branch is
/// descriptive proof of the one recovered bundle and carries no append
/// authority.
pub enum DoubleSubmitGluingRecoveryV4 {
    Continued {
        recovery: RecoveryReceiptV4,
        receipt: DoubleSubmitGluingProfileReceiptV4,
    },
    AlreadyComplete {
        recovery: RecoveryReceiptV4,
        completed: M5CompletedGluingProfileV4,
    },
}

fn complete_profile_session_v4(
    profile: &mut M5GluingProfileSessionV4<'_, '_, '_>,
) -> Result<DoubleSubmitGluingProfileReceiptV4, JournalError> {
    let mut published_input_count = 0_usize;
    while profile.publish_next_gluing_input()?.is_some() {
        published_input_count =
            published_input_count
                .checked_add(1)
                .ok_or(JournalError::Identity(
                    "M5 input publication count overflow",
                ))?;
    }
    let bundle = profile.append_gluing_bundle()?;
    Ok(DoubleSubmitGluingProfileReceiptV4 {
        published_input_count,
        bundle,
    })
}

fn require_payment_conflict_v4(
    receipt: DoubleSubmitGluingProfileReceiptV4,
) -> Result<DoubleSubmitGluingProfileReceiptV4, M5RuntimeError> {
    let result = receipt.bundle.core().result();
    if result != GluingResultV4::Failed {
        return Err(M5RuntimeError::ExpectedConflict(result));
    }
    Ok(receipt)
}

#[derive(Debug, Error)]
pub enum M5RuntimeError {
    #[error(transparent)]
    Core(#[from] reviewgraphen_core::DomainError),
    #[error(transparent)]
    M5(#[from] reviewgraphen_core::M5Error),
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error("M5 inputs must occupy the fixed payment and UI-event context slots")]
    InputContext,
    #[error("the closed double-submit fixture must produce a failed gluing result, got {0:?}")]
    ExpectedConflict(GluingResultV4),
    #[error("the closed payment-conflict operation requires conflicting assignments")]
    AssignmentConflictRequired,
}

/// Publishes source-bound inputs in the ADR 0022 context order and appends
/// exactly one atomic bundle.  Store owns CAS publication, orphan adoption,
/// replay confirmation, and the post-sync uncertainty boundary; Core owns
/// all descriptor and gluing validation.
pub fn run_double_submit_gluing_v4(
    session: &mut ReplayedV4RunSession<'_, '_>,
    basis: &mut AuthorityReplayBasisV4,
    inputs: DoubleSubmitGluingInputsV4,
) -> Result<DoubleSubmitGluingReceiptV4, M5RuntimeError> {
    let payment = match inputs.payment {
        Some(input) => Some(session.publish_gluing_input(input.source, input.descriptor, basis)?),
        None => None,
    };
    let ui_event = match inputs.ui_event {
        Some(input) => Some(session.publish_gluing_input(input.source, input.descriptor, basis)?),
        None => None,
    };
    let bundle = session.mint_gluing_bundle(basis)?;
    let bundle = session.append_gluing_bundle(bundle, basis)?;
    Ok(DoubleSubmitGluingReceiptV4 {
        payment,
        ui_event,
        bundle,
    })
}

/// Runs the fixed reference scenario.  The expected `Failed` result is an
/// assertion on an already source-bound bundle, never an assignment derived
/// from model text or an error-string substitute for an obstruction.
pub fn run_double_submit_payment_conflict_v4(
    session: &mut ReplayedV4RunSession<'_, '_>,
    basis: &mut AuthorityReplayBasisV4,
    inputs: DoubleSubmitGluingInputsV4,
) -> Result<DoubleSubmitGluingReceiptV4, M5RuntimeError> {
    if inputs.compatibility() != Some(AssignmentCompatibilityV4::Conflict) {
        return Err(M5RuntimeError::AssignmentConflictRequired);
    }
    let receipt = run_double_submit_gluing_v4(session, basis, inputs)?;
    let result = receipt.bundle.core().result();
    if result != GluingResultV4::Failed {
        return Err(M5RuntimeError::ExpectedConflict(result));
    }
    Ok(receipt)
}

/// The normal M5 Runtime entry point. Store keeps the full two-pass replay,
/// augmented roots, input capabilities, and exclusive locks inside its
/// callback. This permits an idempotent 0/1/2 descriptor prefix without
/// retaining host material across a process restart.
pub fn run_double_submit_payment_profile_conflict_v4(
    journal: &EventJournal<'_>,
    base_roots: AuthorityTrustRootsV4,
    assignments: M5DoubleSubmitAssignmentsV4,
) -> Result<DoubleSubmitGluingProfileReceiptV4, M5RuntimeError> {
    if assignments.compatibility() != AssignmentCompatibilityV4::Conflict {
        return Err(M5RuntimeError::AssignmentConflictRequired);
    }
    let receipt = journal.with_m5_gluing_profile_session(
        base_roots,
        assignments,
        complete_profile_session_v4,
    )?;
    require_payment_conflict_v4(receipt)
}

/// Resolves the same closed operation after a Store-attributed canonical-tail
/// recovery. The supplied key must come from `inspect_recovery_v4`; Store
/// consumes it, repairs the tail, derives fresh augmented roots from the base
/// roots, and keeps the complete sequence under one lock interval. If the
/// uncertain bundle was already durable, full replay returns a descriptive
/// completed proof and the append callback is not run.
pub fn recover_double_submit_payment_profile_conflict_v4(
    journal: &EventJournal<'_>,
    base_roots: AuthorityTrustRootsV4,
    assignments: M5DoubleSubmitAssignmentsV4,
    key: RecoveryKeyV4,
    provenance: RecoveryProvenanceV4,
) -> Result<DoubleSubmitGluingRecoveryV4, M5RuntimeError> {
    if assignments.compatibility() != AssignmentCompatibilityV4::Conflict {
        return Err(M5RuntimeError::AssignmentConflictRequired);
    }
    let recovered = journal.recover_with_m5_gluing_profile_session(
        base_roots,
        assignments,
        key,
        provenance,
        complete_profile_session_v4,
    )?;
    match recovered {
        M5GluingProfileRecoveryV4::Continued { recovery, value } => {
            Ok(DoubleSubmitGluingRecoveryV4::Continued {
                recovery,
                receipt: require_payment_conflict_v4(value)?,
            })
        }
        M5GluingProfileRecoveryV4::AlreadyComplete {
            recovery,
            completed,
        } => {
            if completed.result() != GluingResultV4::Failed {
                return Err(M5RuntimeError::ExpectedConflict(completed.result()));
            }
            Ok(DoubleSubmitGluingRecoveryV4::AlreadyComplete {
                recovery,
                completed,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reviewgraphen_core::{ContentHash, StableId};
    use reviewgraphen_store::test_support::{
        FixtureAssignmentValueV4, M5M4PrefixFixtureManifestV4,
        materialize_foreign_profile_m5_m4_prefix_v4, materialize_m5_m4_prefix_v4,
    };
    use reviewgraphen_store::{
        JournalError, RecoveryInspectionV4, RecoveryKindV4, RecoveryOutcomeV4, StoreLimits,
        StoreRoot,
    };
    use serde_json::Value;

    fn recovery_provenance(operation: &'static str) -> RecoveryProvenanceV4 {
        RecoveryProvenanceV4::new("runtime-m5-e2e", operation).unwrap()
    }

    #[test]
    fn profile_runtime_restarts_from_zero_one_and_two_descriptor_prefixes() {
        for durable_prefix in 0..=2 {
            let workspace = tempfile::tempdir().unwrap();
            let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
            let fixture = materialize_m5_m4_prefix_v4(&root).unwrap();
            let (journal, base, assignments) = fixture.into_parts();
            if durable_prefix > 0 {
                journal
                    .with_m5_gluing_profile_session(
                        base.build().unwrap(),
                        assignments.build().unwrap(),
                        |profile| {
                            for _ in 0..durable_prefix {
                                assert!(profile.publish_next_gluing_input()?.is_some());
                            }
                            Ok(())
                        },
                    )
                    .unwrap();
            }

            let receipt = run_double_submit_payment_profile_conflict_v4(
                &journal,
                base.build().unwrap(),
                assignments.build().unwrap(),
            )
            .unwrap();
            assert_eq!(receipt.published_input_count, 2 - durable_prefix);
            assert_eq!(receipt.bundle.core().result(), GluingResultV4::Failed);
            let completed = journal
                .inspect_completed_m5_gluing_profile_v4(
                    base.build().unwrap(),
                    assignments.build().unwrap(),
                )
                .unwrap();
            assert_eq!(completed.result(), GluingResultV4::Failed);
            assert!(completed.obstruction_id().is_some());
            assert!(!completed.obstruction_source_ids().is_empty());
            assert!(matches!(
                run_double_submit_payment_profile_conflict_v4(
                    &journal,
                    base.build().unwrap(),
                    assignments.build().unwrap(),
                ),
                Err(M5RuntimeError::Journal(JournalError::Domain(
                    reviewgraphen_core::DomainError::AlreadyComplete
                )))
            ));
        }
    }

    #[test]
    fn registration_post_sync_uncertainty_requires_keyed_profile_recovery() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let fixture = materialize_m5_m4_prefix_v4(&root).unwrap();
        let (journal, base, assignments) = fixture.into_parts();
        let identity = journal.reader().unwrap().identity().clone();
        assert!(matches!(
            journal.with_m5_gluing_profile_session(
                base.build().unwrap(),
                assignments.build().unwrap(),
                |profile| {
                    profile.inject_next_append_post_sync_uncertainty_for_test_support();
                    profile.publish_next_gluing_input().map(|_| ())
                },
            ),
            Err(JournalError::SessionUncertain)
        ));
        assert!(journal.reader().is_err());

        let key = EventJournal::inspect_recovery_v4(
            &root,
            RecoveryInspectionV4::new(
                identity.run_id.clone(),
                identity.genesis_hash(),
                RecoveryKindV4::CanonicalTail,
            ),
        )
        .unwrap();
        let recovered = recover_double_submit_payment_profile_conflict_v4(
            &journal,
            base.build().unwrap(),
            assignments.build().unwrap(),
            key,
            recovery_provenance("registration-post-sync"),
        )
        .unwrap();
        let DoubleSubmitGluingRecoveryV4::Continued { recovery, receipt } = recovered else {
            panic!("one durable registration must continue the exact suffix")
        };
        assert!(matches!(
            recovery.outcome(),
            RecoveryOutcomeV4::TailRecovered { .. }
        ));
        assert_eq!(receipt.published_input_count, 1);
        assert_eq!(receipt.bundle.core().result(), GluingResultV4::Failed);
    }

    #[test]
    fn bundle_post_sync_uncertainty_returns_verified_complete_and_refuses_wrong_key() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let fixture = materialize_m5_m4_prefix_v4(&root).unwrap();
        let (journal, base, assignments) = fixture.into_parts();
        let identity = journal.reader().unwrap().identity().clone();
        assert!(matches!(
            journal.with_m5_gluing_profile_session(
                base.build().unwrap(),
                assignments.build().unwrap(),
                |profile| {
                    assert!(profile.publish_next_gluing_input()?.is_some());
                    assert!(profile.publish_next_gluing_input()?.is_some());
                    profile.inject_next_append_post_sync_uncertainty_for_test_support();
                    profile.append_gluing_bundle().map(|_| ())
                },
            ),
            Err(JournalError::SessionUncertain)
        ));

        let foreign_key = EventJournal::inspect_recovery_v4(
            &root,
            RecoveryInspectionV4::new(
                StableId::parse("run:foreign-m5-recovery").unwrap(),
                ContentHash::sha256(b"foreign-m5-genesis"),
                RecoveryKindV4::GenesisBootstrap,
            ),
        )
        .unwrap();
        assert!(matches!(
            recover_double_submit_payment_profile_conflict_v4(
                &journal,
                base.build().unwrap(),
                assignments.build().unwrap(),
                foreign_key,
                recovery_provenance("foreign-key"),
            ),
            Err(M5RuntimeError::Journal(JournalError::RecoveryKeyMismatchV4))
        ));
        let key = EventJournal::inspect_recovery_v4(
            &root,
            RecoveryInspectionV4::new(
                identity.run_id.clone(),
                identity.genesis_hash(),
                RecoveryKindV4::CanonicalTail,
            ),
        )
        .unwrap();
        let recovered = recover_double_submit_payment_profile_conflict_v4(
            &journal,
            base.build().unwrap(),
            assignments.build().unwrap(),
            key,
            recovery_provenance("bundle-post-sync"),
        )
        .unwrap();
        let DoubleSubmitGluingRecoveryV4::AlreadyComplete {
            recovery,
            completed,
        } = recovered
        else {
            panic!("the line-synced atomic bundle must not be appended twice")
        };
        assert!(matches!(
            recovery.outcome(),
            RecoveryOutcomeV4::TailRecovered { .. }
        ));
        assert_eq!(completed.result(), GluingResultV4::Failed);
        assert!(completed.obstruction_id().is_some());
        assert!(!completed.obstruction_source_ids().is_empty());
    }

    #[test]
    fn registration_recovery_refuses_mismatched_roots_and_conflicting_assignments() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let fixture = materialize_m5_m4_prefix_v4(&root).unwrap();
        let (journal, mut base, assignments) = fixture.into_parts();
        let identity = journal.reader().unwrap().identity().clone();
        assert!(matches!(
            journal.with_m5_gluing_profile_session(
                base.build().unwrap(),
                assignments.build().unwrap(),
                |profile| {
                    profile.inject_next_append_post_sync_uncertainty_for_test_support();
                    profile.publish_next_gluing_input().map(|_| ())
                },
            ),
            Err(JournalError::SessionUncertain)
        ));
        let key = EventJournal::inspect_recovery_v4(
            &root,
            RecoveryInspectionV4::new(
                identity.run_id.clone(),
                identity.genesis_hash(),
                RecoveryKindV4::CanonicalTail,
            ),
        )
        .unwrap();
        base.repository_source_hash = ContentHash::sha256(b"mismatched-recovery-root");
        let error = match recover_double_submit_payment_profile_conflict_v4(
            &journal,
            base.build().unwrap(),
            assignments.build().unwrap(),
            key,
            recovery_provenance("mismatched-roots"),
        ) {
            Ok(_) => panic!("mismatched roots must not yield a recovery outcome"),
            Err(error) => error,
        };
        assert!(!matches!(error, M5RuntimeError::AssignmentConflictRequired));

        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let fixture = materialize_m5_m4_prefix_v4(&root).unwrap();
        let (journal, base, mut assignments) = fixture.into_parts();
        let identity = journal.reader().unwrap().identity().clone();
        assert!(matches!(
            journal.with_m5_gluing_profile_session(
                base.build().unwrap(),
                assignments.build().unwrap(),
                |profile| {
                    profile.inject_next_append_post_sync_uncertainty_for_test_support();
                    profile.publish_next_gluing_input().map(|_| ())
                },
            ),
            Err(JournalError::SessionUncertain)
        ));
        let key = EventJournal::inspect_recovery_v4(
            &root,
            RecoveryInspectionV4::new(
                identity.run_id.clone(),
                identity.genesis_hash(),
                RecoveryKindV4::CanonicalTail,
            ),
        )
        .unwrap();
        assignments.payment = FixtureAssignmentValueV4::Satisfied;
        assignments.ui_event = FixtureAssignmentValueV4::Required;
        let error = match recover_double_submit_payment_profile_conflict_v4(
            &journal,
            base.build().unwrap(),
            assignments.build().unwrap(),
            key,
            recovery_provenance("mismatched-assignments"),
        ) {
            Ok(_) => panic!("mismatched assignments must not yield a recovery outcome"),
            Err(error) => error,
        };
        assert!(!matches!(error, M5RuntimeError::AssignmentConflictRequired));
    }

    #[test]
    fn committed_fixture_negatives_and_independent_v5_outputs_are_deterministic() {
        let committed = M5M4PrefixFixtureManifestV4::committed().unwrap();
        let regenerated =
            reviewgraphen_store::test_support::regenerate_m5_m4_prefix_manifest_v4().unwrap();
        assert_eq!(
            committed.canonical_bytes().unwrap(),
            regenerated.canonical_bytes().unwrap()
        );
        let mut wrong_profile: Value =
            serde_json::from_slice(&committed.canonical_bytes().unwrap()).unwrap();
        let mut unknown_field = wrong_profile.clone();
        unknown_field["accepted"] = Value::Bool(true);
        assert!(serde_json::from_value::<M5M4PrefixFixtureManifestV4>(unknown_field).is_err());
        wrong_profile["canonical_genesis_base64"] = Value::String("AA".to_owned());
        let wrong_profile: M5M4PrefixFixtureManifestV4 =
            serde_json::from_value(wrong_profile).unwrap();
        assert!(wrong_profile.validate().is_err());

        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let foreign = materialize_foreign_profile_m5_m4_prefix_v4(&root).unwrap();
        let (foreign_journal, foreign_base, foreign_assignments) = foreign.into_parts();
        let event_count = foreign_journal.reader().unwrap().events().len();
        assert!(
            run_double_submit_payment_profile_conflict_v4(
                &foreign_journal,
                foreign_base.build().unwrap(),
                foreign_assignments.build().unwrap(),
            )
            .is_err()
        );
        assert_eq!(
            foreign_journal.reader().unwrap().events().len(),
            event_count
        );

        let mut snapshots = Vec::new();
        for _ in 0..2 {
            let workspace = tempfile::tempdir().unwrap();
            let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
            let fixture = materialize_m5_m4_prefix_v4(&root).unwrap();
            let (journal, base, mut assignments) = fixture.into_parts();
            let mut wrong_base = base.clone();
            wrong_base.repository_source_hash = ContentHash::sha256(b"wrong-root");
            assert!(
                run_double_submit_payment_profile_conflict_v4(
                    &journal,
                    wrong_base.build().unwrap(),
                    assignments.build().unwrap(),
                )
                .is_err()
            );
            journal
                .with_m5_gluing_profile_session(
                    base.build().unwrap(),
                    assignments.build().unwrap(),
                    |profile| {
                        assert!(profile.publish_next_gluing_input()?.is_some());
                        Ok(())
                    },
                )
                .unwrap();
            let event_count = journal.reader().unwrap().events().len();
            assignments.payment = FixtureAssignmentValueV4::Satisfied;
            assignments.ui_event = FixtureAssignmentValueV4::Required;
            assert!(
                run_double_submit_payment_profile_conflict_v4(
                    &journal,
                    base.build().unwrap(),
                    assignments.build().unwrap(),
                )
                .is_err()
            );
            assert_eq!(journal.reader().unwrap().events().len(), event_count);
            assignments.payment = FixtureAssignmentValueV4::Required;
            assignments.ui_event = FixtureAssignmentValueV4::Satisfied;
            run_double_submit_payment_profile_conflict_v4(
                &journal,
                base.build().unwrap(),
                assignments.build().unwrap(),
            )
            .unwrap();
            let (completed, snapshot) = journal
                .completed_m5_v5_snapshot_for_test_support(
                    base.build().unwrap(),
                    assignments.build().unwrap(),
                )
                .unwrap();
            assert_eq!(snapshot.gluing_obstructions.len(), 1);
            let obstruction = &snapshot.gluing_obstructions[0].obstruction;
            assert_eq!(
                obstruction["id"].as_str(),
                completed.obstruction_id().map(StableId::as_str)
            );
            let indexed_sources = obstruction["source_ids"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| StableId::parse(value.as_str().unwrap()).unwrap())
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(&indexed_sources, completed.obstruction_source_ids());
            snapshots.push(snapshot);
        }
        assert_eq!(snapshots[0], snapshots[1]);
    }
}
