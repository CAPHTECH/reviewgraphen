//! Narrow, process-free M4 verification orchestration.
//!
//! The runtime owns ordering only. Core owns record construction and
//! transition validation, Store owns FD-relative CAS and durable event
//! append, and `reviewgraphen-verifier` owns the closed deterministic
//! descriptor implementations. There is intentionally no command, path,
//! argument, environment, network, or caller-supplied runner surface here.

use reviewgraphen_core::{
    AuthorityReplayBasisV3, DecisionInputV3, StableId, StaticFactEvaluationV1,
    StaticVerificationAttemptInspectionV3, VerificationAttemptStageV3,
    VerificationBundleResumeAuthorityV3, VerifierArtifactRoleV3,
};
use reviewgraphen_store::{
    CasHash, CasReceipt, CasStore, JournalAppendReceipt, JournalError,
    RecoveredVerificationBundleV3Session, ReplayedV3RunSession, StoreError, StoreRoot,
    V3VerificationBundleAppendReceipt,
};
use reviewgraphen_verifier::{Descriptor, VerifierError, descriptor_by_id};
use std::io::Cursor;
use thiserror::Error;

const MAX_RUNTIME_VERIFIER_ARTIFACT_BYTES: u64 = 16_777_216;

#[cfg(test)]
std::thread_local! {
    static FIXTURE_DESCRIPTOR_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static STATIC_DESCRIPTOR_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static CAS_RACE_ON_PUT: std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(None) };
}

fn mark_fixture_descriptor_call() {
    #[cfg(test)]
    FIXTURE_DESCRIPTOR_CALLS.with(|calls| calls.set(calls.get() + 1));
}

fn mark_static_descriptor_call() {
    #[cfg(test)]
    STATIC_DESCRIPTOR_CALLS.with(|calls| calls.set(calls.get() + 1));
}

#[derive(Debug, Error)]
pub enum M4RuntimeError {
    #[error(transparent)]
    Core(#[from] reviewgraphen_core::DomainError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error(transparent)]
    Verifier(#[from] VerifierError),
    #[error("runtime store root does not match the replayed V3 session root")]
    StoreRootMismatch,
    #[error("M4 claim does not exist in the replayed V3 aggregate")]
    ClaimNotFound,
    #[error("M4 claim does not have the exact one-obligation closure")]
    ClaimObligationClosure,
    #[error("fixed verifier output differs from the Store-sealed fixture execution")]
    FixtureOutputMismatch,
    #[error("the exact M4 verification attempt is already complete")]
    AlreadyComplete,
    #[error("the M4 verification bundle is partially durable and must use the resume API")]
    ResumeRequired,
}

#[derive(Debug)]
pub struct DurableVerifierArtifact {
    pub cas: CasReceipt,
    pub registration_id: StableId,
    pub registration: JournalAppendReceipt,
}

#[derive(Debug)]
pub struct FixtureVerificationReceipt {
    pub witness: Option<DurableVerifierArtifact>,
    pub result: Option<DurableVerifierArtifact>,
    pub bundle: V3VerificationBundleAppendReceipt,
}

#[derive(Debug)]
pub struct StaticVerificationReceipt {
    pub input: Option<DurableVerifierArtifact>,
    pub result: Option<DurableVerifierArtifact>,
    pub bundle: V3VerificationBundleAppendReceipt,
}

#[derive(Debug)]
pub enum M4VerificationReceipt {
    Fixture(FixtureVerificationReceipt),
    Static(StaticVerificationReceipt),
}

/// Runs only a compiled-in descriptor. An unknown descriptor is rejected by
/// the closed verifier registry before CAS is opened or an event is appended.
pub fn verify_claim(
    session: &mut ReplayedV3RunSession<'_, '_>,
    root: &StoreRoot,
    claim_id: &StableId,
    descriptor_id: &str,
    basis: &mut AuthorityReplayBasisV3,
) -> Result<M4VerificationReceipt, M4RuntimeError> {
    require_store_root(session, root)?;
    match descriptor_by_id(descriptor_id)? {
        Descriptor::FixtureTest => Ok(M4VerificationReceipt::Fixture(verify_fixed_fixture(
            session, root, claim_id, basis,
        )?)),
        Descriptor::StaticFact => Ok(M4VerificationReceipt::Static(verify_static_fact(
            session, root, claim_id, basis,
        )?)),
    }
}

/// Executes the one checked-in fixture in-process and durably appends its
/// exact witness/result registrations followed by E→B→V. A successful return
/// supports the claim but never records a human decision or finding.
pub fn verify_fixed_fixture(
    session: &mut ReplayedV3RunSession<'_, '_>,
    root: &StoreRoot,
    claim_id: &StableId,
    basis: &mut AuthorityReplayBasisV3,
) -> Result<FixtureVerificationReceipt, M4RuntimeError> {
    require_store_root(session, root)?;
    session.validate_v3_operation_basis(basis)?;
    let expected = session.expect_fixture_verification_attempt_v3(claim_id, basis)?;
    let stage = session.inspect_m4_verification_attempt_v3(&expected, basis)?;
    match stage {
        VerificationAttemptStageV3::Ready => {
            // Descriptor execution occurs only in Ready. Core independently
            // seals the same fixed execution into fresh one-shot authority.
            mark_fixture_descriptor_call();
            let verifier_output = reviewgraphen_verifier::execute_fixed_fixture_harness()?;
            let mut execution = session.execute_fixture_harness(claim_id, basis)?;
            if verifier_output.witness_bytes() != expected.input_bytes()
                || execution.witness_bytes() != expected.input_bytes()
                || execution.fixture_result_bytes() != expected.output_bytes()
                || verifier_output.witness_hash()
                    != &reviewgraphen_core::ContentHash::sha256(execution.witness_bytes())
                || verifier_output.media_type()
                    != reviewgraphen_verifier::FIXTURE_WITNESS_MEDIA_TYPE
                || verifier_output.property_id() != reviewgraphen_core::M4_PROPERTY_ID
                || verifier_output.test_artifact_id().as_str()
                    != reviewgraphen_core::FIXTURE_TEST_ARTIFACT_ID
            {
                return Err(M4RuntimeError::FixtureOutputMismatch);
            }
            preflight_artifacts(root, [expected.input_bytes(), expected.output_bytes()])?;
            let cas = CasStore::open(root)?;
            let witness_cas = put_absent(session, &cas, expected.input_bytes())?;
            let witness_registration =
                session.prepare_external_fixture_witness_registration(&mut execution, basis)?;
            let witness_registration_id = witness_registration.registration_id().clone();
            let witness_journal =
                session.append_authority_registration(witness_registration, basis)?;
            let result_cas = put_absent(session, &cas, expected.output_bytes())?;
            let result_registration =
                session.prepare_fixture_verifier_output_registration(&mut execution, basis)?;
            let result_registration_id = result_registration.registration_id().clone();
            let result_journal =
                session.append_authority_registration(result_registration, basis)?;
            let admission = session.admit_external_fixture_witness(
                &mut execution,
                &witness_registration_id,
                basis,
            )?;
            let bundle = session.mint_fixture_verification_bundle(
                admission,
                &result_registration_id,
                basis,
            )?;
            let bundle = session.append_verification_bundle(bundle, basis)?;
            Ok(FixtureVerificationReceipt {
                witness: Some(DurableVerifierArtifact {
                    cas: witness_cas,
                    registration_id: witness_registration_id,
                    registration: witness_journal,
                }),
                result: Some(DurableVerifierArtifact {
                    cas: result_cas,
                    registration_id: result_registration_id,
                    registration: result_journal,
                }),
                bundle,
            })
        }
        VerificationAttemptStageV3::WitnessRegistered => {
            preflight_artifacts(root, [expected.output_bytes()])?;
            let mut resume =
                session.recover_fixture_registration_resume_authority(claim_id, basis)?;
            let cas = CasStore::open(root)?;
            let result_cas = put_absent(session, &cas, resume.fixture_result_bytes())?;
            let result_registration =
                session.prepare_fixture_output_from_resume(&mut resume, basis)?;
            let result_registration_id = result_registration.registration_id().clone();
            let result_journal =
                session.append_authority_registration(result_registration, basis)?;
            let bundle = session.mint_fixture_verification_bundle_from_resume(resume, basis)?;
            let bundle = session.append_verification_bundle(bundle, basis)?;
            Ok(FixtureVerificationReceipt {
                witness: None,
                result: Some(DurableVerifierArtifact {
                    cas: result_cas,
                    registration_id: result_registration_id,
                    registration: result_journal,
                }),
                bundle,
            })
        }
        VerificationAttemptStageV3::OutputRegistered => {
            let resume = session.recover_fixture_registration_resume_authority(claim_id, basis)?;
            let bundle = session.mint_fixture_verification_bundle_from_resume(resume, basis)?;
            let bundle = session.append_verification_bundle(bundle, basis)?;
            Ok(FixtureVerificationReceipt {
                witness: None,
                result: None,
                bundle,
            })
        }
        VerificationAttemptStageV3::Complete => Err(M4RuntimeError::AlreadyComplete),
        VerificationAttemptStageV3::BundlePartial => Err(M4RuntimeError::ResumeRequired),
        VerificationAttemptStageV3::InputRegistered => Err(M4RuntimeError::FixtureOutputMismatch),
    }
}

/// Evaluates the closed static descriptor, stores its canonical input/result,
/// and appends the sealed static bundle. Static verification never passes and
/// therefore cannot support or accept a claim.
pub fn verify_static_fact(
    session: &mut ReplayedV3RunSession<'_, '_>,
    root: &StoreRoot,
    claim_id: &StableId,
    basis: &mut AuthorityReplayBasisV3,
) -> Result<StaticVerificationReceipt, M4RuntimeError> {
    require_store_root(session, root)?;
    session.validate_v3_operation_basis(basis)?;
    {
        let aggregate = session.aggregate()?;
        let claim = aggregate
            .execution_claims()
            .find(|claim| claim.id() == claim_id)
            .ok_or(M4RuntimeError::ClaimNotFound)?;
        if claim.obligation_ids().len() != 1 {
            return Err(M4RuntimeError::ClaimObligationClosure);
        }
    }
    let (expected, stage) = match session.inspect_static_verification_attempt_v3(claim_id, basis)? {
        StaticVerificationAttemptInspectionV3::Ready => {
            let evaluation = evaluate_static_descriptor(session, claim_id)?;
            let expected =
                session.seal_static_verification_attempt_v3(claim_id, &evaluation, basis)?;
            let stage = session.inspect_m4_verification_attempt_v3(&expected, basis)?;
            (expected, stage)
        }
        StaticVerificationAttemptInspectionV3::Existing { expected, stage } => (expected, stage),
    };
    match stage {
        VerificationAttemptStageV3::Ready => {
            let input_bytes = expected.input_bytes();
            let result_bytes = expected.output_bytes();
            preflight_artifacts(root, [input_bytes, result_bytes])?;
            let cas = CasStore::open(root)?;
            let input_cas = put_absent(session, &cas, input_bytes)?;
            let input_registration = session.prepare_static_verifier_artifact_registration(
                claim_id.clone(),
                VerifierArtifactRoleV3::Input,
                expected.input_hash().clone(),
                expected.input_size(),
                basis,
            )?;
            let input_registration_id = input_registration.registration_id().clone();
            let input_journal = session.append_authority_registration(input_registration, basis)?;
            let result_cas = put_absent(session, &cas, result_bytes)?;
            let result_registration = session.prepare_static_verifier_artifact_registration(
                claim_id.clone(),
                VerifierArtifactRoleV3::Output,
                expected.output_hash().clone(),
                expected.output_size(),
                basis,
            )?;
            let result_registration_id = result_registration.registration_id().clone();
            let result_journal =
                session.append_authority_registration(result_registration, basis)?;
            let bundle = session.mint_static_verification_bundle(
                claim_id,
                &input_registration_id,
                &result_registration_id,
                basis,
            )?;
            let bundle = session.append_verification_bundle(bundle, basis)?;
            Ok(StaticVerificationReceipt {
                input: Some(DurableVerifierArtifact {
                    cas: input_cas,
                    registration_id: input_registration_id,
                    registration: input_journal,
                }),
                result: Some(DurableVerifierArtifact {
                    cas: result_cas,
                    registration_id: result_registration_id,
                    registration: result_journal,
                }),
                bundle,
            })
        }
        VerificationAttemptStageV3::InputRegistered => {
            preflight_artifacts(root, [expected.output_bytes()])?;
            let cas = CasStore::open(root)?;
            let result_cas = put_absent(session, &cas, expected.output_bytes())?;
            let result_registration = session.prepare_static_verifier_artifact_registration(
                claim_id.clone(),
                VerifierArtifactRoleV3::Output,
                expected.output_hash().clone(),
                expected.output_size(),
                basis,
            )?;
            let result_registration_id = result_registration.registration_id().clone();
            let result_journal =
                session.append_authority_registration(result_registration, basis)?;
            let bundle = session.mint_static_verification_bundle(
                claim_id,
                expected.input_registration_id(),
                &result_registration_id,
                basis,
            )?;
            let bundle = session.append_verification_bundle(bundle, basis)?;
            Ok(StaticVerificationReceipt {
                input: None,
                result: Some(DurableVerifierArtifact {
                    cas: result_cas,
                    registration_id: result_registration_id,
                    registration: result_journal,
                }),
                bundle,
            })
        }
        VerificationAttemptStageV3::OutputRegistered => {
            let bundle = session.mint_static_verification_bundle(
                claim_id,
                expected.input_registration_id(),
                expected.output_registration_id(),
                basis,
            )?;
            let bundle = session.append_verification_bundle(bundle, basis)?;
            Ok(StaticVerificationReceipt {
                input: None,
                result: None,
                bundle,
            })
        }
        VerificationAttemptStageV3::Complete => Err(M4RuntimeError::AlreadyComplete),
        VerificationAttemptStageV3::BundlePartial => Err(M4RuntimeError::ResumeRequired),
        VerificationAttemptStageV3::WitnessRegistered => {
            Err(M4RuntimeError::ClaimObligationClosure)
        }
    }
}

/// Explicitly records one host-authorized human decision. Verification never
/// calls this function automatically.
pub fn record_human_decision(
    session: &mut ReplayedV3RunSession<'_, '_>,
    claim_id: &StableId,
    input: DecisionInputV3,
    basis: &mut AuthorityReplayBasisV3,
) -> Result<JournalAppendReceipt, M4RuntimeError> {
    let decision = session.mint_decision(claim_id, input, basis)?;
    Ok(session.append_decision(decision, basis)?)
}

/// Explicitly derives and records the current finding projection. Decisions
/// and verification do not create findings as a side effect.
pub fn record_current_finding(
    session: &mut ReplayedV3RunSession<'_, '_>,
    claim_id: &StableId,
    projection_descriptor_id: &str,
    basis: &mut AuthorityReplayBasisV3,
) -> Result<JournalAppendReceipt, M4RuntimeError> {
    let finding = session.mint_finding(claim_id, projection_descriptor_id, basis)?;
    Ok(session.append_finding(finding, basis)?)
}

/// Completes only the missing suffix of a Store-recovered partial bundle.
/// Ordinary verification entry points intentionally refuse this state.
pub fn resume_partial_verification_bundle<'root, 'roots>(
    recovered: RecoveredVerificationBundleV3Session<'root, 'roots>,
    authority: VerificationBundleResumeAuthorityV3,
    basis: &mut AuthorityReplayBasisV3,
) -> Result<
    (
        ReplayedV3RunSession<'root, 'roots>,
        V3VerificationBundleAppendReceipt,
    ),
    M4RuntimeError,
> {
    Ok(recovered.resume_verification_bundle(authority, basis)?)
}

fn evaluate_static_descriptor(
    session: &ReplayedV3RunSession<'_, '_>,
    claim_id: &StableId,
) -> Result<StaticFactEvaluationV1, M4RuntimeError> {
    mark_static_descriptor_call();
    let aggregate = session.aggregate()?;
    let claim = aggregate
        .execution_claims()
        .find(|claim| claim.id() == claim_id)
        .ok_or(M4RuntimeError::ClaimNotFound)?;
    let obligation_id = claim
        .obligation_ids()
        .iter()
        .next()
        .ok_or(M4RuntimeError::ClaimObligationClosure)?;
    if claim.obligation_ids().len() != 1 {
        return Err(M4RuntimeError::ClaimObligationClosure);
    }
    let obligation = aggregate
        .obligations()
        .find(|obligation| obligation.id() == obligation_id)
        .ok_or(M4RuntimeError::ClaimObligationClosure)?;
    Ok(reviewgraphen_verifier::evaluate_static(
        aggregate.program(),
        obligation,
        claim,
    )?)
}

fn require_store_root(
    session: &ReplayedV3RunSession<'_, '_>,
    root: &StoreRoot,
) -> Result<(), M4RuntimeError> {
    if !session.matches_store_root(root)? {
        return Err(M4RuntimeError::StoreRootMismatch);
    }
    Ok(())
}

fn preflight_artifacts<'a>(
    root: &StoreRoot,
    artifacts: impl IntoIterator<Item = &'a [u8]>,
) -> Result<(), M4RuntimeError> {
    let mut total = 0_u64;
    for bytes in artifacts {
        let observed = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if observed > root.limits().max_object_bytes {
            return Err(StoreError::ObjectTooLarge {
                limit: root.limits().max_object_bytes,
                observed,
            }
            .into());
        }
        total = total.checked_add(observed).ok_or({
            reviewgraphen_core::DomainError::Incomplete {
                operation: "M4 runtime verifier artifact working bytes",
                limit: MAX_RUNTIME_VERIFIER_ARTIFACT_BYTES as usize,
                observed: usize::MAX,
            }
        })?;
    }
    if total > MAX_RUNTIME_VERIFIER_ARTIFACT_BYTES {
        return Err(reviewgraphen_core::DomainError::Incomplete {
            operation: "M4 runtime verifier artifact working bytes",
            limit: MAX_RUNTIME_VERIFIER_ARTIFACT_BYTES as usize,
            observed: usize::try_from(total).unwrap_or(usize::MAX),
        }
        .into());
    }
    Ok(())
}

fn put_exact(cas: &CasStore<'_>, bytes: &[u8]) -> Result<CasReceipt, M4RuntimeError> {
    let hash = reviewgraphen_core::ContentHash::sha256(bytes);
    let hash = CasHash::parse(hash.to_string())?;
    Ok(cas.put(
        &hash,
        Some(u64::try_from(bytes.len()).unwrap_or(u64::MAX)),
        Cursor::new(bytes),
    )?)
}

fn put_absent(
    session: &ReplayedV3RunSession<'_, '_>,
    cas: &CasStore<'_>,
    bytes: &[u8],
) -> Result<CasReceipt, M4RuntimeError> {
    let hash = reviewgraphen_core::ContentHash::sha256(bytes);
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if session.cas_contains_exact_v3(&hash, size)? {
        return Err(JournalError::OrphanCasObjectV3 { hash }.into());
    }
    #[cfg(test)]
    CAS_RACE_ON_PUT.with(|target| {
        if let Some(remaining) = target.get() {
            if remaining == 1 {
                target.set(None);
                put_exact(cas, bytes).expect("test CAS race publication");
            } else {
                target.set(Some(remaining - 1));
            }
        }
    });
    let receipt = put_exact(cas, bytes)?;
    if receipt.existed {
        return Err(JournalError::OrphanCasObjectV3 { hash }.into());
    }
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reviewgraphen_core::{
        ArtifactRegisteredV3, ArtifactSensitivity, ArtifactSourceV3, AssessmentDispositionV3,
        AssessmentReviewStatusV3, AuthorityTrustRootsV3, ClaimPolarity, ContentHash,
        DecisionOutcomeV3, EventCommand, EventLog, ExecutionClaimInputV2, ExecutionOutcome,
        ExecutionRecordInput, FINDING_PROJECTION_ID, FIXTURE_DESCRIPTOR_ID, FIXTURE_HARNESS_ID,
        FIXTURE_HARNESS_REVISION, FIXTURE_HARNESS_SOURCE_HASH, FIXTURE_MEDIA_TYPE,
        FIXTURE_PROCEDURE_ID, FIXTURE_TEST_ARTIFACT_ID, FIXTURE_WITNESS_HASH,
        HarnessTrustRootInputV3, HumanAuthorityCapabilityV3, HumanTrustGrantInputV3,
        M4_PROPERTY_ID, MvpRulePack, ObligationLifecycle, PlanBudget, ProgramSpace,
        ReviewAggregate, SnapshotSourceRecordEntry, SnapshotSourcesRecorded,
        ValidatedExecutionBundle, plan, prepare_context,
    };
    use reviewgraphen_store::{EventJournal, JournalGenesis, JournalIdentity, StoreLimits};
    use serde_json::Value;
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    fn id(value: &str) -> StableId {
        StableId::parse(value).unwrap()
    }

    fn put_fixture_cas(root: &StoreRoot, bytes: &[u8]) {
        put_exact(&CasStore::open(root).unwrap(), bytes).unwrap();
    }

    fn cas_inventory(root: &StoreRoot) -> Vec<(PathBuf, u64)> {
        fn visit(base: &Path, path: &Path, files: &mut Vec<(PathBuf, u64)>) {
            let Ok(entries) = std::fs::read_dir(path) else {
                return;
            };
            for entry in entries {
                let entry = entry.unwrap();
                let metadata = entry.metadata().unwrap();
                if metadata.is_dir() {
                    visit(base, &entry.path(), files);
                } else if metadata.is_file() {
                    files.push((
                        entry.path().strip_prefix(base).unwrap().to_path_buf(),
                        metadata.len(),
                    ));
                }
            }
        }
        let base = root.path().join("artifacts");
        let mut files = Vec::new();
        visit(&base, &base, &mut files);
        files.sort_unstable();
        files
    }

    fn append_unrelated_gap(
        session: &mut ReplayedV3RunSession<'_, '_>,
        root: &StoreRoot,
        basis: &mut AuthorityReplayBasisV3,
        label: &str,
    ) {
        let bytes = format!("unrelated gap {label}").into_bytes();
        put_fixture_cas(root, &bytes);
        let registration = ArtifactRegisteredV3::new(
            session.run_id().unwrap().clone(),
            ContentHash::sha256(&bytes),
            "text/plain",
            bytes.len() as u64,
            ArtifactSensitivity::WorkspaceSource,
            ArtifactSourceV3::SnapshotIngest {
                adapter_id: format!("runtime-gap-{label}"),
                run_id: session.run_id().unwrap().clone(),
                snapshot_id: session.aggregate().unwrap().program().snapshot_id().clone(),
            },
        )
        .unwrap();
        session
            .append_nonauthority_registration_v3(registration, basis)
            .unwrap();
    }

    fn public_v3_fixture_journal<'a>(
        root: &'a StoreRoot,
        run: &str,
        static_candidates: usize,
    ) -> (EventJournal<'a>, AuthorityTrustRootsV3, StableId) {
        let mut input: Value = serde_json::from_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        let mut contains = input["relations"][0].clone();
        contains["id"] = Value::String("relation:file-contains-payment-charge-runtime".into());
        contains["kind"] = Value::String("contains".into());
        contains["source_id"] = Value::String("file:payment-repository".into());
        contains["target_ids"] = serde_json::json!(["function:payment-charge"]);
        contains["directed"] = Value::Bool(true);
        input["relations"].as_array_mut().unwrap().push(contains);
        let test = input["artifacts"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|artifact| artifact["id"] == FIXTURE_TEST_ARTIFACT_ID)
            .unwrap();
        test["location"]["start_line"] = Value::Null;
        test["location"]["end_line"] = Value::Null;
        let invariant = input["invariants"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|invariant| invariant["property_id"] == M4_PROPERTY_ID)
            .unwrap();
        invariant["scope_ids"] = if static_candidates == 0 {
            serde_json::json!(["requirement:payment-at-most-once"])
        } else {
            serde_json::json!(["context:payment", "context:ui-event"])
        };
        if static_candidates == 2 {
            let mut second = invariant.clone();
            second["id"] = Value::String("invariant:payment-at-most-once-runtime-second".into());
            second["description"] = Value::String("second applicable runtime invariant".into());
            input["invariants"].as_array_mut().unwrap().push(second);
        }
        let bytes_by_path = BTreeMap::from([
            ("src/checkout_controller.rs", b"checkout\n".repeat(40)),
            ("src/payment_repository.rs", b"repository\n".repeat(40)),
        ]);
        for artifact in input["artifacts"].as_array_mut().unwrap() {
            if artifact["kind"] == "file" {
                let path = artifact["location"]["path"].as_str().unwrap();
                artifact["content_hash"] =
                    Value::String(ContentHash::sha256(&bytes_by_path[path]).to_string());
            }
        }
        let repository_source_hash =
            ContentHash::parse(input["source"]["content_hash"].as_str().unwrap()).unwrap();
        let program = ProgramSpace::from_json_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
        let repository_id = program.repository_id().clone();
        let (universe, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        let mut log = EventLog::new_v3(
            id(run),
            ReviewAggregate::new(program, universe, obligations).unwrap(),
        )
        .unwrap();
        let run_id = log.run_id().clone();
        let snapshot_id = log.aggregate().program().snapshot_id().clone();
        let files = log
            .aggregate()
            .program()
            .artifacts()
            .iter()
            .filter(|artifact| artifact.kind == "file")
            .cloned()
            .collect::<Vec<_>>();
        let mut entries = Vec::new();
        let mut source_by_id = BTreeMap::new();
        for artifact in files {
            let path = artifact.location.as_ref().unwrap().path.clone();
            let bytes = bytes_by_path[path.as_str()].clone();
            put_fixture_cas(root, &bytes);
            let hash = ContentHash::sha256(&bytes);
            let registration = ArtifactRegisteredV3::new(
                run_id.clone(),
                hash.clone(),
                "text/plain",
                bytes.len() as u64,
                ArtifactSensitivity::WorkspaceSource,
                ArtifactSourceV3::SnapshotIngest {
                    adapter_id: "runtime-v3-e2e-fixture".into(),
                    run_id: run_id.clone(),
                    snapshot_id: snapshot_id.clone(),
                },
            )
            .unwrap();
            entries.push(
                SnapshotSourceRecordEntry::new(
                    artifact.id.clone(),
                    path,
                    hash.clone(),
                    registration.registration_id().clone(),
                    hash,
                    bytes.iter().filter(|byte| **byte == b'\n').count() as u64 + 1,
                )
                .unwrap(),
            );
            source_by_id.insert(artifact.id, bytes);
            log.append(EventCommand::artifact_registered_v3(registration))
                .unwrap();
        }
        entries.sort_by(|left, right| left.path().cmp(right.path()));
        log.append(EventCommand::snapshot_sources_recorded(
            SnapshotSourcesRecorded::new(snapshot_id, entries).unwrap(),
        ))
        .unwrap();
        let review_plan = plan(log.aggregate(), PlanBudget::new(16, 16).unwrap()).unwrap();
        log.append(EventCommand::review_plan_recorded(review_plan.clone()))
            .unwrap();
        let (obligation_id, built) = review_plan
            .waves()
            .iter()
            .flat_map(|wave| wave.obligation_ids())
            .find_map(|candidate| {
                let obligation = log
                    .aggregate()
                    .obligations()
                    .find(|obligation| obligation.id() == candidate)?;
                if obligation.property_id() != M4_PROPERTY_ID {
                    return None;
                }
                let mut context = prepare_context(log.aggregate(), candidate.clone()).ok()?;
                while let Some(request) = context.next_source_request().ok()? {
                    context
                        .submit_source(&request, &source_by_id[request.artifact_id()])
                        .ok()?;
                }
                let built = context.finish().ok()?;
                (!built.envelope().normalized_included_source_ids().is_empty())
                    .then(|| (candidate.clone(), built))
            })
            .unwrap();
        log.append(EventCommand::obligation_transition(
            obligation_id.clone(),
            ObligationLifecycle::Planned,
        ))
        .unwrap();
        log.append(EventCommand::obligation_transition(
            obligation_id.clone(),
            ObligationLifecycle::InProgress,
        ))
        .unwrap();
        let envelope = built.envelope().clone();
        log.append(EventCommand::context_envelope_projected(built))
            .unwrap();

        let wave = review_plan
            .waves()
            .iter()
            .find(|wave| wave.obligation_ids().contains(&obligation_id))
            .unwrap();
        let execution_input = ExecutionRecordInput::fake(
            review_plan.id().clone(),
            wave.id().clone(),
            obligation_id.clone(),
            envelope.id().clone(),
            envelope.snapshot_id().clone(),
            1,
        )
        .unwrap();
        let execution_id = execution_input.execution_id().unwrap();
        let raw = br#"{"attempt":1,"fixture":true,"version":3}"#.to_vec();
        put_fixture_cas(root, &raw);
        let raw_registration = ArtifactRegisteredV3::new(
            run_id.clone(),
            ContentHash::sha256(&raw),
            "application/json",
            raw.len() as u64,
            ArtifactSensitivity::Sensitive,
            ArtifactSourceV3::ReviewerExecution {
                execution_id,
                reviewer_id: reviewgraphen_core::FAKE_REVIEWER_ID.into(),
                run_id: run_id.clone(),
            },
        )
        .unwrap();
        log.append(EventCommand::artifact_registered_v3(
            raw_registration.clone(),
        ))
        .unwrap();
        let obligation = log
            .aggregate()
            .obligations()
            .find(|obligation| obligation.id() == &obligation_id)
            .unwrap();
        let claim = ExecutionClaimInputV2::new(
            obligation.property_id(),
            obligation.normalized_target_refs().clone(),
            ClaimPolarity::IssuePresent,
            "runtime public V3 fixture demonstrates duplicate submit",
            envelope.normalized_included_source_ids().clone(),
            BTreeSet::new(),
            BTreeSet::new(),
            Some(1.0),
        )
        .unwrap();
        let source_buffers = source_by_id.values().collect::<Vec<_>>();
        let execution = ValidatedExecutionBundle::fake_v3(
            execution_input,
            &raw_registration,
            raw,
            source_buffers,
            vec![claim],
            ExecutionOutcome::Structured,
        )
        .unwrap();
        let claim_id = execution.claims()[0].id().clone();
        log.append(EventCommand::review_execution_recorded(execution))
            .unwrap();

        let policy = ContentHash::sha256(b"runtime-public-v3-fixture-policy");
        let claim_record = log
            .aggregate()
            .execution_claims()
            .find(|claim| claim.id() == &claim_id)
            .unwrap();
        let roots = AuthorityTrustRootsV3::new(
            policy.clone(),
            repository_id.clone(),
            repository_source_hash.clone(),
            vec![HarnessTrustRootInputV3 {
                policy_revision_hash: policy.clone(),
                repository_id,
                repository_source_hash,
                harness_id: FIXTURE_HARNESS_ID.into(),
                harness_revision: FIXTURE_HARNESS_REVISION.into(),
                harness_source_hash: ContentHash::parse(FIXTURE_HARNESS_SOURCE_HASH).unwrap(),
                test_artifact_id: id(FIXTURE_TEST_ARTIFACT_ID),
                descriptor_id: FIXTURE_DESCRIPTOR_ID.into(),
                procedure_version: FIXTURE_PROCEDURE_ID.into(),
                result_hash: ContentHash::parse(FIXTURE_WITNESS_HASH).unwrap(),
                result_size: 145,
                result_media_type: FIXTURE_MEDIA_TYPE.into(),
                result_sensitivity: ArtifactSensitivity::CanonicalState,
                run_id: run_id.clone(),
                genesis_hash: log.genesis_hash().clone(),
                snapshot_id: log.aggregate().program().snapshot_id().clone(),
                universe_id: log.aggregate().universe().id().clone(),
                property_id: M4_PROPERTY_ID.into(),
                claim_id: claim_id.clone(),
                claim_body_hash: claim_record.body_hash().unwrap(),
            }],
            vec![HumanTrustGrantInputV3 {
                policy_revision_hash: policy,
                actor: "human:runtime-reviewer".into(),
                authority_id: "runtime-review-board".into(),
                capabilities: BTreeSet::from([HumanAuthorityCapabilityV3::AcceptFinding]),
                run_id: run_id.clone(),
                snapshot_id: log.aggregate().program().snapshot_id().clone(),
                universe_id: log.aggregate().universe().id().clone(),
                property_ids: BTreeSet::from([M4_PROPERTY_ID.into()]),
                claim_ids: BTreeSet::from([claim_id.clone()]),
                valid_from: "2026-01-01T00:00:00Z".into(),
                valid_until: "2027-01-01T00:00:00Z".into(),
            }],
        )
        .unwrap();
        let genesis = log
            .run_genesis_snapshot()
            .unwrap()
            .canonical_bytes()
            .unwrap();
        put_fixture_cas(root, &genesis);
        let identity = JournalIdentity::new(run_id.clone(), JournalGenesis::V3(genesis)).unwrap();
        let journal =
            EventJournal::initialize_v3(root, identity, log.events()[0].envelope().clone())
                .unwrap();
        let (mut session, mut basis) = journal.replayed_v3_session(&roots).unwrap();
        let snapshot_id = session.aggregate().unwrap().program().snapshot_id().clone();
        let files = session
            .aggregate()
            .unwrap()
            .program()
            .artifacts()
            .iter()
            .filter(|artifact| artifact.kind == "file")
            .cloned()
            .collect::<Vec<_>>();
        let mut entries = Vec::new();
        for artifact in files {
            let path = artifact.location.as_ref().unwrap().path.clone();
            let bytes = &source_by_id[&artifact.id];
            let hash = ContentHash::sha256(bytes);
            let registration = ArtifactRegisteredV3::new(
                run_id.clone(),
                hash.clone(),
                "text/plain",
                bytes.len() as u64,
                ArtifactSensitivity::WorkspaceSource,
                ArtifactSourceV3::SnapshotIngest {
                    adapter_id: "runtime-v3-e2e-fixture".into(),
                    run_id: run_id.clone(),
                    snapshot_id: snapshot_id.clone(),
                },
            )
            .unwrap();
            entries.push(
                SnapshotSourceRecordEntry::new(
                    artifact.id,
                    path,
                    hash.clone(),
                    registration.registration_id().clone(),
                    hash,
                    bytes.iter().filter(|byte| **byte == b'\n').count() as u64 + 1,
                )
                .unwrap(),
            );
            session
                .append_nonauthority_registration_v3(registration, &mut basis)
                .unwrap();
        }
        entries.sort_by(|left, right| left.path().cmp(right.path()));
        session
            .append_snapshot_sources_v3(
                SnapshotSourcesRecorded::new(snapshot_id, entries).unwrap(),
                &mut basis,
            )
            .unwrap();
        let persisted_plan = plan(
            session.aggregate().unwrap(),
            PlanBudget::new(16, 16).unwrap(),
        )
        .unwrap();
        session
            .append_review_plan_v3(persisted_plan.clone(), &mut basis)
            .unwrap();
        session
            .append_obligation_transition_v3(
                obligation_id.clone(),
                ObligationLifecycle::Planned,
                &mut basis,
            )
            .unwrap();
        session
            .append_obligation_transition_v3(
                obligation_id.clone(),
                ObligationLifecycle::InProgress,
                &mut basis,
            )
            .unwrap();
        let mut context =
            prepare_context(session.aggregate().unwrap(), obligation_id.clone()).unwrap();
        while let Some(request) = context.next_source_request().unwrap() {
            context
                .submit_source(&request, &source_by_id[request.artifact_id()])
                .unwrap();
        }
        let built = context.finish().unwrap();
        let envelope = built.envelope().clone();
        session
            .append_context_projection_v3(built, &mut basis)
            .unwrap();
        let wave = persisted_plan
            .waves()
            .iter()
            .find(|wave| wave.obligation_ids().contains(&obligation_id))
            .unwrap();
        let execution_input = ExecutionRecordInput::fake(
            persisted_plan.id().clone(),
            wave.id().clone(),
            obligation_id.clone(),
            envelope.id().clone(),
            envelope.snapshot_id().clone(),
            1,
        )
        .unwrap();
        let execution_id = execution_input.execution_id().unwrap();
        let raw = br#"{"attempt":1,"fixture":true,"version":3}"#.to_vec();
        let raw_registration = ArtifactRegisteredV3::new(
            run_id,
            ContentHash::sha256(&raw),
            "application/json",
            raw.len() as u64,
            ArtifactSensitivity::Sensitive,
            ArtifactSourceV3::ReviewerExecution {
                execution_id,
                reviewer_id: reviewgraphen_core::FAKE_REVIEWER_ID.into(),
                run_id: session.run_id().unwrap().clone(),
            },
        )
        .unwrap();
        session
            .append_nonauthority_registration_v3(raw_registration.clone(), &mut basis)
            .unwrap();
        let obligation = session
            .aggregate()
            .unwrap()
            .obligations()
            .find(|obligation| obligation.id() == &obligation_id)
            .unwrap();
        let claim = ExecutionClaimInputV2::new(
            obligation.property_id(),
            obligation.normalized_target_refs().clone(),
            ClaimPolarity::IssuePresent,
            "runtime public V3 fixture demonstrates duplicate submit",
            envelope.normalized_included_source_ids().clone(),
            BTreeSet::new(),
            BTreeSet::new(),
            Some(1.0),
        )
        .unwrap();
        let source_buffers = source_by_id.values().collect::<Vec<_>>();
        let execution = ValidatedExecutionBundle::fake_v3(
            execution_input,
            &raw_registration,
            raw,
            source_buffers,
            vec![claim],
            ExecutionOutcome::Structured,
        )
        .unwrap();
        assert_eq!(execution.claims()[0].id(), &claim_id);
        session
            .append_review_execution_v3(execution, &mut basis)
            .unwrap();
        drop(session);
        (journal, roots, claim_id)
    }

    #[test]
    fn artifact_preflight_is_inclusive_and_has_no_store_side_effect() {
        let workspace = tempfile::tempdir().unwrap();
        let limits = reviewgraphen_store::StoreLimits {
            max_object_bytes: 3,
            ..reviewgraphen_store::StoreLimits::default()
        };
        let root = StoreRoot::open(workspace.path(), limits).unwrap();

        preflight_artifacts(&root, [&b"abc"[..]]).unwrap();
        assert!(matches!(
            preflight_artifacts(&root, [&b"abcd"[..]]),
            Err(M4RuntimeError::Store(StoreError::ObjectTooLarge {
                limit: 3,
                observed: 4
            }))
        ));
        assert!(!root.path().join("artifacts").exists());
    }

    #[test]
    fn aggregate_working_preflight_is_exact_and_refuses_plus_one_before_store_open() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let exact = vec![0_u8; MAX_RUNTIME_VERIFIER_ARTIFACT_BYTES as usize];

        preflight_artifacts(&root, [exact.as_slice()]).unwrap();
        assert!(matches!(
            preflight_artifacts(&root, [exact.as_slice(), &[0_u8][..]]),
            Err(M4RuntimeError::Core(
                reviewgraphen_core::DomainError::Incomplete {
                    operation: "M4 runtime verifier artifact working bytes",
                    limit,
                    observed,
                }
            )) if limit == MAX_RUNTIME_VERIFIER_ARTIFACT_BYTES as usize
                && observed == MAX_RUNTIME_VERIFIER_ARTIFACT_BYTES as usize + 1
        ));
        assert!(!root.path().join("artifacts").exists());
    }

    #[test]
    fn the_public_runtime_has_only_the_two_closed_descriptors() {
        assert!(matches!(
            descriptor_by_id("repository-requested-runner"),
            Err(VerifierError::UnsupportedDescriptor)
        ));
        for descriptor in reviewgraphen_verifier::descriptors() {
            let capabilities = descriptor.metadata().capabilities;
            assert!(!capabilities.process);
            assert!(!capabilities.network);
            assert!(!capabilities.workspace_write);
        }
    }

    #[test]
    fn public_fixture_verification_supports_but_never_accepts_until_explicit_human_steps() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, roots, claim_id) =
            public_v3_fixture_journal(&root, "run:runtime-m4-happy", 1);
        let (mut session, mut basis) = journal.replayed_v3_session(&roots).unwrap();

        let receipt = verify_fixed_fixture(&mut session, &root, &claim_id, &mut basis).unwrap();
        assert_eq!(receipt.bundle.authority().event_ids().len(), 3);
        let assessment = session.claim_assessment(&claim_id).unwrap().unwrap();
        assert_eq!(assessment.disposition(), AssessmentDispositionV3::Supported);
        assert_eq!(
            assessment.review_status(),
            AssessmentReviewStatusV3::Unreviewed
        );
        assert!(assessment.active_decision_id().is_none());
        assert!(assessment.current_finding_id().is_none());

        record_human_decision(
            &mut session,
            &claim_id,
            DecisionInputV3::new(
                DecisionOutcomeV3::Accept,
                "human:runtime-reviewer",
                "runtime-review-board",
                "explicit acceptance of the reproduced counterexample",
                "2026-08-10T00:00:00Z",
                None,
            ),
            &mut basis,
        )
        .unwrap();
        let assessment = session.claim_assessment(&claim_id).unwrap().unwrap();
        assert_eq!(assessment.disposition(), AssessmentDispositionV3::Accepted);
        assert_eq!(
            assessment.review_status(),
            AssessmentReviewStatusV3::Accepted
        );
        assert!(assessment.current_finding_id().is_none());

        record_current_finding(&mut session, &claim_id, FINDING_PROJECTION_ID, &mut basis).unwrap();
        assert!(
            session
                .claim_assessment(&claim_id)
                .unwrap()
                .unwrap()
                .current_finding_id()
                .is_some()
        );
    }

    #[test]
    fn tmp_probe_record_human_decision_edges() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, roots, claim_id) = public_v3_fixture_journal(&root, "run:tmp-probe-edges", 1);
        let (mut session, mut basis) = journal.replayed_v3_session(&roots).unwrap();
        verify_fixed_fixture(&mut session, &root, &claim_id, &mut basis).unwrap();

        // 1. unknown claim id must fail with a typed error, session stays usable
        let bogus = StableId::parse("claim:nonexistent-claim").unwrap();
        let err = record_human_decision(
            &mut session,
            &bogus,
            DecisionInputV3::new(
                DecisionOutcomeV3::Accept,
                "human:runtime-reviewer",
                "runtime-review-board",
                "should not apply",
                "2026-08-10T00:00:00Z",
                None,
            ),
            &mut basis,
        )
        .unwrap_err();
        println!("EDGE1 unknown claim => {err:?}");

        // 2. grant expired by issued_at must be refused
        let err = record_human_decision(
            &mut session,
            &claim_id,
            DecisionInputV3::new(
                DecisionOutcomeV3::Accept,
                "human:runtime-reviewer",
                "runtime-review-board",
                "expired window",
                "2028-08-10T00:00:00Z",
                None,
            ),
            &mut basis,
        )
        .unwrap_err();
        println!("EDGE2 expired grant => {err:?}");

        // 3. unknown actor must be refused
        let err = record_human_decision(
            &mut session,
            &claim_id,
            DecisionInputV3::new(
                DecisionOutcomeV3::Accept,
                "human:intruder",
                "runtime-review-board",
                "no grant",
                "2026-08-10T00:00:00Z",
                None,
            ),
            &mut basis,
        )
        .unwrap_err();
        println!("EDGE3 unknown actor => {err:?}");

        // 4. expires_at before issued_at
        let r = record_human_decision(
            &mut session,
            &claim_id,
            DecisionInputV3::new(
                DecisionOutcomeV3::Accept,
                "human:runtime-reviewer",
                "runtime-review-board",
                "backwards expiry",
                "2026-08-10T00:00:00Z",
                Some("2026-01-01T00:00:00Z".to_owned()),
            ),
            &mut basis,
        );
        println!(
            "EDGE4 expires_at<issued_at => {:?}",
            r.as_ref().map(|_| "ok").map_err(|e| format!("{e:?}"))
        );
        if r.is_ok() {
            let a = session.claim_assessment(&claim_id).unwrap().unwrap();
            println!(
                "EDGE4 disposition={:?} active={:?} decisions={}",
                a.disposition(),
                a.active_decision_id(),
                session.aggregate().unwrap().execution_claims().count()
            );
        }

        // 5. second decision on the same claim
        let r = record_human_decision(
            &mut session,
            &claim_id,
            DecisionInputV3::new(
                DecisionOutcomeV3::Reject,
                "human:runtime-reviewer",
                "runtime-review-board",
                "second decision",
                "2026-08-11T00:00:00Z",
                None,
            ),
            &mut basis,
        );
        println!(
            "EDGE5 second decision => {:?}",
            r.as_ref().map(|_| "ok").map_err(|e| format!("{e:?}"))
        );
    }

    #[test]
    fn stale_decision_basis_is_rejected_without_poisoning_fresh_session() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, roots, claim_id) = public_v3_fixture_journal(&root, "run:tmp-probe-stale", 1);
        // Capture and release a basis before opening the write session. A replay
        // session holds the journal lock, so opening both at once deadlocks.
        let (stale_session, mut stale) = journal.replayed_v3_session(&roots).unwrap();
        drop(stale_session);
        let (mut session, mut basis) = journal.replayed_v3_session(&roots).unwrap();

        // capture basis BEFORE the fixture verification bundle advances it
        verify_fixed_fixture(&mut session, &root, &claim_id, &mut basis).unwrap();

        let stale_result = record_human_decision(
            &mut session,
            &claim_id,
            DecisionInputV3::new(
                DecisionOutcomeV3::Accept,
                "human:runtime-reviewer",
                "runtime-review-board",
                "decision on stale basis",
                "2026-08-10T00:00:00Z",
                None,
            ),
            &mut stale,
        );
        assert!(matches!(
            stale_result,
            Err(M4RuntimeError::Journal(JournalError::Domain(
                reviewgraphen_core::DomainError::AuthorityReplayBasisMismatch
            )))
        ));

        // after refusal, a correct call must still work
        let fresh_result = record_human_decision(
            &mut session,
            &claim_id,
            DecisionInputV3::new(
                DecisionOutcomeV3::Accept,
                "human:runtime-reviewer",
                "runtime-review-board",
                "decision on fresh basis",
                "2026-08-10T00:00:00Z",
                None,
            ),
            &mut basis,
        );
        assert!(fresh_result.is_ok());

        // second Accept with a valid grant after an active Accept decision
        let second_result = record_human_decision(
            &mut session,
            &claim_id,
            DecisionInputV3::new(
                DecisionOutcomeV3::Accept,
                "human:runtime-reviewer",
                "runtime-review-board",
                "second accept",
                "2026-08-12T00:00:00Z",
                None,
            ),
            &mut basis,
        );
        assert!(second_result.is_ok());
        let a = session.claim_assessment(&claim_id).unwrap().unwrap();
        assert_eq!(a.disposition(), AssessmentDispositionV3::Accepted);
        assert_eq!(a.review_status(), AssessmentReviewStatusV3::Accepted);
        assert!(a.active_decision_id().is_some());
    }

    #[test]
    fn static_verification_is_durable_but_remains_proposed_and_unreviewed() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, roots, claim_id) =
            public_v3_fixture_journal(&root, "run:runtime-m4-static", 1);
        let (mut session, mut basis) = journal.replayed_v3_session(&roots).unwrap();

        STATIC_DESCRIPTOR_CALLS.with(|calls| calls.set(0));
        let receipt = verify_static_fact(&mut session, &root, &claim_id, &mut basis).unwrap();
        assert_eq!(STATIC_DESCRIPTOR_CALLS.with(|calls| calls.get()), 1);
        assert_eq!(receipt.bundle.authority().event_ids().len(), 3);
        let assessment = session.claim_assessment(&claim_id).unwrap().unwrap();
        assert_eq!(assessment.disposition(), AssessmentDispositionV3::Proposed);
        assert_eq!(
            assessment.review_status(),
            AssessmentReviewStatusV3::Unreviewed
        );
        assert_eq!(assessment.evidence_ids().len(), 1);
        assert_eq!(assessment.verification_ids().len(), 1);
        assert!(assessment.active_decision_id().is_none());
        assert!(assessment.current_finding_id().is_none());
    }

    #[test]
    fn static_absent_and_ambiguous_results_remain_evidence_free_and_inconclusive() {
        for (candidate_count, run) in [
            (0, "run:runtime-m4-static-absent"),
            (2, "run:runtime-m4-static-ambiguous"),
        ] {
            let workspace = tempfile::tempdir().unwrap();
            let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
            let (journal, roots, claim_id) = public_v3_fixture_journal(&root, run, candidate_count);
            let (mut session, mut basis) = journal.replayed_v3_session(&roots).unwrap();

            let receipt = verify_static_fact(&mut session, &root, &claim_id, &mut basis).unwrap();
            assert_eq!(receipt.bundle.authority().event_ids().len(), 1);
            let assessment = session.claim_assessment(&claim_id).unwrap().unwrap();
            assert_eq!(assessment.disposition(), AssessmentDispositionV3::Proposed);
            assert_eq!(
                assessment.review_status(),
                AssessmentReviewStatusV3::Unreviewed
            );
            assert!(assessment.evidence_ids().is_empty());
            assert_eq!(assessment.verification_ids().len(), 1);
        }
    }

    #[test]
    fn wrong_root_unknown_descriptor_and_unknown_claim_refuse_before_runtime_writes() {
        let workspace = tempfile::tempdir().unwrap();
        let wrong_workspace = tempfile::tempdir().unwrap();
        let stale_workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let wrong_root = StoreRoot::open(wrong_workspace.path(), StoreLimits::default()).unwrap();
        let stale_root = StoreRoot::open(stale_workspace.path(), StoreLimits::default()).unwrap();
        let (journal, roots, claim_id) =
            public_v3_fixture_journal(&root, "run:runtime-m4-refusal", 1);
        let (stale_journal, stale_roots, _) =
            public_v3_fixture_journal(&stale_root, "run:runtime-m4-stale-basis", 1);
        let (mut session, mut basis) = journal.replayed_v3_session(&roots).unwrap();
        let (_stale_session, mut stale_basis) =
            stale_journal.replayed_v3_session(&stale_roots).unwrap();
        let before = session.event_count().unwrap();

        assert!(matches!(
            verify_claim(
                &mut session,
                &wrong_root,
                &claim_id,
                FIXTURE_DESCRIPTOR_ID,
                &mut basis,
            ),
            Err(M4RuntimeError::StoreRootMismatch)
        ));
        assert!(matches!(
            verify_claim(
                &mut session,
                &root,
                &claim_id,
                "repository-requested-runner",
                &mut basis,
            ),
            Err(M4RuntimeError::Verifier(
                VerifierError::UnsupportedDescriptor
            ))
        ));
        assert!(matches!(
            verify_static_fact(
                &mut session,
                &root,
                &id("claim:missing-runtime"),
                &mut basis,
            ),
            Err(M4RuntimeError::ClaimNotFound)
        ));
        let cas_before_stale = cas_inventory(&root);
        assert!(matches!(
            verify_static_fact(&mut session, &root, &claim_id, &mut stale_basis),
            Err(M4RuntimeError::Journal(JournalError::Domain(
                reviewgraphen_core::DomainError::AuthorityReplayBasisMismatch
            )))
        ));
        assert_eq!(session.event_count().unwrap(), before);
        assert_eq!(cas_inventory(&root), cas_before_stale);
        session.validate_v3_operation_basis(&basis).unwrap();
        assert!(!wrong_root.path().join("artifacts").exists());
    }

    #[test]
    fn fixture_retry_from_witness_registration_does_not_rerun_or_duplicate() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, roots, claim_id) =
            public_v3_fixture_journal(&root, "run:runtime-fixture-witness-resume", 1);
        let (mut session, mut basis) = journal.replayed_v3_session(&roots).unwrap();
        let expected = session
            .expect_fixture_verification_attempt_v3(&claim_id, &basis)
            .unwrap();
        put_fixture_cas(&root, expected.input_bytes());
        let mut execution = session.execute_fixture_harness(&claim_id, &basis).unwrap();
        let witness = session
            .prepare_external_fixture_witness_registration(&mut execution, &basis)
            .unwrap();
        session
            .append_authority_registration(witness, &mut basis)
            .unwrap();
        let before = session.event_count().unwrap();
        FIXTURE_DESCRIPTOR_CALLS.with(|calls| calls.set(0));
        let receipt = verify_fixed_fixture(&mut session, &root, &claim_id, &mut basis).unwrap();
        assert!(receipt.witness.is_none());
        assert!(receipt.result.is_some());
        assert_eq!(session.event_count().unwrap(), before + 4);
        assert_eq!(session.verification_count().unwrap(), 1);
        assert_eq!(FIXTURE_DESCRIPTOR_CALLS.with(|calls| calls.get()), 0);
    }

    #[test]
    fn fixture_retry_from_output_registration_appends_bundle_only() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, roots, claim_id) =
            public_v3_fixture_journal(&root, "run:runtime-fixture-output-resume", 1);
        let (mut session, mut basis) = journal.replayed_v3_session(&roots).unwrap();
        let expected = session
            .expect_fixture_verification_attempt_v3(&claim_id, &basis)
            .unwrap();
        put_fixture_cas(&root, expected.input_bytes());
        put_fixture_cas(&root, expected.output_bytes());
        let mut execution = session.execute_fixture_harness(&claim_id, &basis).unwrap();
        let witness = session
            .prepare_external_fixture_witness_registration(&mut execution, &basis)
            .unwrap();
        session
            .append_authority_registration(witness, &mut basis)
            .unwrap();
        let output = session
            .prepare_fixture_verifier_output_registration(&mut execution, &basis)
            .unwrap();
        session
            .append_authority_registration(output, &mut basis)
            .unwrap();
        let before = session.event_count().unwrap();
        FIXTURE_DESCRIPTOR_CALLS.with(|calls| calls.set(0));
        let receipt = verify_fixed_fixture(&mut session, &root, &claim_id, &mut basis).unwrap();
        assert!(receipt.witness.is_none());
        assert!(receipt.result.is_none());
        assert_eq!(session.event_count().unwrap(), before + 3);
        assert_eq!(FIXTURE_DESCRIPTOR_CALLS.with(|calls| calls.get()), 0);
    }

    #[test]
    fn static_retry_from_input_registration_uses_deterministic_expected_output() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, roots, claim_id) =
            public_v3_fixture_journal(&root, "run:runtime-static-input-resume", 1);
        let (mut session, mut basis) = journal.replayed_v3_session(&roots).unwrap();
        assert!(matches!(
            session
                .inspect_static_verification_attempt_v3(&claim_id, &basis)
                .unwrap(),
            StaticVerificationAttemptInspectionV3::Ready
        ));
        let evaluation = evaluate_static_descriptor(&session, &claim_id).unwrap();
        let expected = session
            .seal_static_verification_attempt_v3(&claim_id, &evaluation, &basis)
            .unwrap();
        put_fixture_cas(&root, expected.input_bytes());
        let input = session
            .prepare_static_verifier_artifact_registration(
                claim_id.clone(),
                VerifierArtifactRoleV3::Input,
                expected.input_hash().clone(),
                expected.input_size(),
                &basis,
            )
            .unwrap();
        session
            .append_authority_registration(input, &mut basis)
            .unwrap();
        let before = session.event_count().unwrap();
        STATIC_DESCRIPTOR_CALLS.with(|calls| calls.set(0));
        let receipt = verify_static_fact(&mut session, &root, &claim_id, &mut basis).unwrap();
        assert!(receipt.input.is_none());
        assert!(receipt.result.is_some());
        assert_eq!(session.event_count().unwrap(), before + 4);
        assert_eq!(STATIC_DESCRIPTOR_CALLS.with(|calls| calls.get()), 0);
    }

    #[test]
    fn static_retry_from_output_registration_appends_bundle_only() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, roots, claim_id) =
            public_v3_fixture_journal(&root, "run:runtime-static-output-resume", 1);
        let (mut session, mut basis) = journal.replayed_v3_session(&roots).unwrap();
        assert!(matches!(
            session
                .inspect_static_verification_attempt_v3(&claim_id, &basis)
                .unwrap(),
            StaticVerificationAttemptInspectionV3::Ready
        ));
        let evaluation = evaluate_static_descriptor(&session, &claim_id).unwrap();
        let expected = session
            .seal_static_verification_attempt_v3(&claim_id, &evaluation, &basis)
            .unwrap();
        put_fixture_cas(&root, expected.input_bytes());
        put_fixture_cas(&root, expected.output_bytes());
        for (role, hash, size) in [
            (
                VerifierArtifactRoleV3::Input,
                expected.input_hash().clone(),
                expected.input_size(),
            ),
            (
                VerifierArtifactRoleV3::Output,
                expected.output_hash().clone(),
                expected.output_size(),
            ),
        ] {
            let registration = session
                .prepare_static_verifier_artifact_registration(
                    claim_id.clone(),
                    role,
                    hash,
                    size,
                    &basis,
                )
                .unwrap();
            session
                .append_authority_registration(registration, &mut basis)
                .unwrap();
        }
        let before = session.event_count().unwrap();
        STATIC_DESCRIPTOR_CALLS.with(|calls| calls.set(0));
        let receipt = verify_static_fact(&mut session, &root, &claim_id, &mut basis).unwrap();
        assert!(receipt.input.is_none());
        assert!(receipt.result.is_none());
        assert_eq!(session.event_count().unwrap(), before + 3);
        assert_eq!(STATIC_DESCRIPTOR_CALLS.with(|calls| calls.get()), 0);
    }

    #[test]
    fn orphan_cas_and_complete_retry_are_typed_and_write_nothing() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, roots, claim_id) =
            public_v3_fixture_journal(&root, "run:runtime-orphan-refusal", 1);
        let (mut session, mut basis) = journal.replayed_v3_session(&roots).unwrap();
        let expected = session
            .expect_fixture_verification_attempt_v3(&claim_id, &basis)
            .unwrap();
        put_fixture_cas(&root, expected.input_bytes());
        let before = session.event_count().unwrap();
        let cas_before_orphan = cas_inventory(&root);
        FIXTURE_DESCRIPTOR_CALLS.with(|calls| calls.set(0));
        assert!(matches!(
            verify_fixed_fixture(&mut session, &root, &claim_id, &mut basis),
            Err(M4RuntimeError::Journal(
                JournalError::OrphanCasObjectV3 { .. }
            ))
        ));
        assert_eq!(session.event_count().unwrap(), before);
        assert_eq!(cas_inventory(&root), cas_before_orphan);
        session.validate_v3_operation_basis(&basis).unwrap();
        assert_eq!(FIXTURE_DESCRIPTOR_CALLS.with(|calls| calls.get()), 0);

        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, roots, claim_id) =
            public_v3_fixture_journal(&root, "run:runtime-complete-refusal", 1);
        let (mut session, mut basis) = journal.replayed_v3_session(&roots).unwrap();
        verify_fixed_fixture(&mut session, &root, &claim_id, &mut basis).unwrap();
        let before = session.event_count().unwrap();
        let cas_before_complete = cas_inventory(&root);
        FIXTURE_DESCRIPTOR_CALLS.with(|calls| calls.set(0));
        assert!(matches!(
            verify_fixed_fixture(&mut session, &root, &claim_id, &mut basis),
            Err(M4RuntimeError::AlreadyComplete)
        ));
        assert_eq!(session.event_count().unwrap(), before);
        assert_eq!(cas_inventory(&root), cas_before_complete);
        session.validate_v3_operation_basis(&basis).unwrap();
        assert_eq!(FIXTURE_DESCRIPTOR_CALLS.with(|calls| calls.get()), 0);
    }

    #[test]
    fn atomic_cas_existed_race_refuses_and_never_leaves_an_unregistered_prior_artifact() {
        for (race_put, run, expected_events) in [
            (1, "run:runtime-cas-race-input", 0),
            (2, "run:runtime-cas-race-output", 1),
        ] {
            let workspace = tempfile::tempdir().unwrap();
            let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
            let (journal, roots, claim_id) = public_v3_fixture_journal(&root, run, 1);
            let (mut session, mut basis) = journal.replayed_v3_session(&roots).unwrap();
            let before_events = session.event_count().unwrap();
            let before_inventory = cas_inventory(&root);
            CAS_RACE_ON_PUT.with(|target| target.set(Some(race_put)));
            assert!(matches!(
                verify_fixed_fixture(&mut session, &root, &claim_id, &mut basis),
                Err(M4RuntimeError::Journal(
                    JournalError::OrphanCasObjectV3 { .. }
                ))
            ));
            assert_eq!(
                session.event_count().unwrap(),
                before_events + expected_events
            );
            assert_eq!(
                cas_inventory(&root).len(),
                before_inventory.len() + usize::from(race_put)
            );
            session.validate_v3_operation_basis(&basis).unwrap();
            if race_put == 2 {
                assert_eq!(session.verification_count().unwrap(), 0);
            }
        }
    }

    #[test]
    fn missing_fixture_trust_root_refuses_before_harness_cas_or_state_changes() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, good_roots, claim_id) =
            public_v3_fixture_journal(&root, "run:runtime-missing-fixture-root", 1);
        let (probe, _) = journal.replayed_v3_session(&good_roots).unwrap();
        let repository_id = probe.aggregate().unwrap().program().repository_id().clone();
        let fixture_json: Value = serde_json::from_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        let repository_source_hash =
            ContentHash::parse(fixture_json["source"]["content_hash"].as_str().unwrap()).unwrap();
        drop(probe);
        let missing_roots = AuthorityTrustRootsV3::new(
            ContentHash::sha256(b"runtime-public-v3-fixture-policy"),
            repository_id,
            repository_source_hash,
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
        let (mut session, mut basis) = journal.replayed_v3_session(&missing_roots).unwrap();
        let before_events = session.event_count().unwrap();
        let before_inventory = cas_inventory(&root);
        let before_assessment = session
            .claim_assessment(&claim_id)
            .unwrap()
            .unwrap()
            .clone();
        FIXTURE_DESCRIPTOR_CALLS.with(|calls| calls.set(0));
        assert!(matches!(
            verify_fixed_fixture(&mut session, &root, &claim_id, &mut basis),
            Err(M4RuntimeError::Journal(JournalError::Domain(
                reviewgraphen_core::DomainError::HarnessTrustRootMissing
            )))
        ));
        assert_eq!(FIXTURE_DESCRIPTOR_CALLS.with(|calls| calls.get()), 0);
        assert_eq!(session.event_count().unwrap(), before_events);
        assert_eq!(cas_inventory(&root), before_inventory);
        assert_eq!(
            session.claim_assessment(&claim_id).unwrap().unwrap(),
            &before_assessment
        );
        session.validate_v3_operation_basis(&basis).unwrap();
    }

    #[test]
    fn unrelated_event_gap_blocks_fixture_and_static_registration_resume() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, roots, claim_id) =
            public_v3_fixture_journal(&root, "run:runtime-fixture-gap", 1);
        let (mut session, mut basis) = journal.replayed_v3_session(&roots).unwrap();
        let expected = session
            .expect_fixture_verification_attempt_v3(&claim_id, &basis)
            .unwrap();
        put_fixture_cas(&root, expected.input_bytes());
        let mut execution = session.execute_fixture_harness(&claim_id, &basis).unwrap();
        let witness = session
            .prepare_external_fixture_witness_registration(&mut execution, &basis)
            .unwrap();
        session
            .append_authority_registration(witness, &mut basis)
            .unwrap();
        append_unrelated_gap(&mut session, &root, &mut basis, "fixture");
        let before_events = session.event_count().unwrap();
        let before_inventory = cas_inventory(&root);
        FIXTURE_DESCRIPTOR_CALLS.with(|calls| calls.set(0));
        assert!(matches!(
            verify_fixed_fixture(&mut session, &root, &claim_id, &mut basis),
            Err(M4RuntimeError::Journal(JournalError::Domain(
                reviewgraphen_core::DomainError::VerificationBundleMismatch
            )))
        ));
        assert_eq!(FIXTURE_DESCRIPTOR_CALLS.with(|calls| calls.get()), 0);
        assert_eq!(session.event_count().unwrap(), before_events);
        assert_eq!(cas_inventory(&root), before_inventory);
        session.validate_v3_operation_basis(&basis).unwrap();

        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, roots, claim_id) =
            public_v3_fixture_journal(&root, "run:runtime-static-gap", 1);
        let (mut session, mut basis) = journal.replayed_v3_session(&roots).unwrap();
        let evaluation = evaluate_static_descriptor(&session, &claim_id).unwrap();
        let expected = session
            .seal_static_verification_attempt_v3(&claim_id, &evaluation, &basis)
            .unwrap();
        put_fixture_cas(&root, expected.input_bytes());
        let input = session
            .prepare_static_verifier_artifact_registration(
                claim_id.clone(),
                VerifierArtifactRoleV3::Input,
                expected.input_hash().clone(),
                expected.input_size(),
                &basis,
            )
            .unwrap();
        session
            .append_authority_registration(input, &mut basis)
            .unwrap();
        append_unrelated_gap(&mut session, &root, &mut basis, "static");
        let before_events = session.event_count().unwrap();
        let before_inventory = cas_inventory(&root);
        STATIC_DESCRIPTOR_CALLS.with(|calls| calls.set(0));
        assert!(matches!(
            verify_static_fact(&mut session, &root, &claim_id, &mut basis),
            Err(M4RuntimeError::Journal(JournalError::Domain(
                reviewgraphen_core::DomainError::VerificationBundleMismatch
            )))
        ));
        assert_eq!(STATIC_DESCRIPTOR_CALLS.with(|calls| calls.get()), 0);
        assert_eq!(session.event_count().unwrap(), before_events);
        assert_eq!(cas_inventory(&root), before_inventory);
        session.validate_v3_operation_basis(&basis).unwrap();
    }
}
