//! Deliberately small, offline CLI surface for the committed double-submit
//! reference scenario. It contains no subprocess, network, arbitrary tool,
//! repository-write, or arbitrary M6 report path.

use jsonschema::{Resource, Validator};
use reviewgraphen_core::{
    ArtifactSensitivity, ArtifactSourceV4, AssignmentValueV4, AuthorityHarnessBindingV3Tuple,
    AuthorityHumanGrantV3Tuple, ContentHash, DecisionInputV3, DecisionOutcomeV3,
    FixedHumanDecisionV5, GluingInputDescriptorV4, HumanAuthorityCapabilityV3,
    M5GluingWorkRequestV5, M6HumanWorkRequestV5, M6ReviewerWorkRequestV5, StableId,
    StaticFactInputV1, V5GluingInputTrustInput, ValidatedFixedM6ReviewerResultV5,
    ValidatedM6StaticResultV5, canonical_json, evaluate_static_fact_result_from_input_v1,
};
use reviewgraphen_report::{ReportRequestV5, generate_v5};
use reviewgraphen_runtime::m5_gluing::run_double_submit_payment_profile_conflict_v4;
use reviewgraphen_store::{
    DerivedIndexV5, DerivedIndexV6, M5ReportAuthorityInspectionV4, StoreLimits, StoreRoot,
    V5AuthorityTrustInput,
};
#[cfg(target_os = "linux")]
use rustix::fs::{self, FileType, Mode, OFlags};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Read},
    path::Path,
};

const MAX_INPUT_BYTES: u64 = 128 * 1024 * 1024;
const V3_URI: &str = "https://capht.tech/schemas/reviewgraphen/review-report.v3.schema.json";
const V4_URI: &str = "https://capht.tech/schemas/reviewgraphen/review-report.v4.schema.json";

pub struct CommandOutcome {
    pub exit_code: u8,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl CommandOutcome {
    fn success(stdout: Vec<u8>, stderr: impl Into<String>) -> Self {
        Self {
            exit_code: 0,
            stdout,
            stderr: stderr.into(),
        }
    }

    fn failure(exit_code: u8, stderr: impl Into<String>) -> Self {
        Self {
            exit_code,
            stdout: Vec::new(),
            stderr: stderr.into(),
        }
    }
}

/// Executes only the documented fixed vertical slice. The parser is manual
/// and closed: unknown commands/arguments are rejected before any fixture,
/// filesystem, Store, or Report work starts.
pub fn run(arguments: Vec<String>) -> CommandOutcome {
    match arguments.as_slice() {
        [command, fixture, name]
            if command == "review" && fixture == "--fixture" && name == "double-submit" =>
        {
            run_double_submit()
        }
        [command, subcommand] if command == "schema" && subcommand == "list" => schema_list(),
        [command, subcommand, name] if command == "schema" && subcommand == "print" => {
            schema_print(name)
        }
        [command, subcommand, path] if command == "schema" && subcommand == "validate" => {
            schema_validate(Path::new(path))
        }
        _ => CommandOutcome::failure(2, usage()),
    }
}

fn usage() -> &'static str {
    "usage: reviewgraphen schema list|print <schema-id>|validate <report.json> | review --fixture double-submit"
}

fn schema_list() -> CommandOutcome {
    let values = json!([
        "reviewgraphen.review.report.v1",
        "reviewgraphen.review.report.v2",
        "reviewgraphen.review.report.v3",
        "reviewgraphen.review.report.v4",
        "reviewgraphen.review.report.v5"
    ]);
    CommandOutcome::success(canonical_json(&values).unwrap_or_default(), String::new())
}

fn schema_print(name: &str) -> CommandOutcome {
    match schema_source(name) {
        Some(source) => CommandOutcome::success(source.as_bytes().to_vec(), String::new()),
        None => CommandOutcome::failure(2, "unknown schema ID"),
    }
}

fn schema_validate(path: &Path) -> CommandOutcome {
    let bytes = match bounded_regular_file(path) {
        Ok(bytes) => bytes,
        Err(error) => return CommandOutcome::failure(3, error),
    };
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return validation_failure("invalid_json"),
    };
    let schema_name = match value.get("schema").and_then(Value::as_str) {
        Some(name) => name,
        None => return validation_failure("missing_schema"),
    };
    let validator = match validator_for(schema_name) {
        Ok(validator) => validator,
        Err(_) => return validation_failure("unsupported_schema"),
    };
    if validator.is_valid(&value) && semantic_validation(schema_name, &value).is_ok() {
        CommandOutcome::success(
            canonical_json(&json!({"schema":schema_name,"valid":true})).unwrap_or_default(),
            String::new(),
        )
    } else {
        validation_failure("schema_invalid")
    }
}

fn validation_failure(reason: &str) -> CommandOutcome {
    CommandOutcome {
        exit_code: 3,
        stdout: canonical_json(&json!({"valid":false,"reason":reason})).unwrap_or_default(),
        stderr: String::new(),
    }
}

fn run_double_submit() -> CommandOutcome {
    // The source-bound continuation owns independent source and target
    // replay/index images. Its Core-defined validation path is intentionally
    // stack-deep; execute the one closed fixture route on a bounded dedicated
    // worker stack so library callers and the binary have identical behavior.
    const FIXED_PIPELINE_STACK_BYTES: usize = 16 * 1024 * 1024;
    match std::thread::Builder::new()
        .name("reviewgraphen-fixed-offline".to_owned())
        .stack_size(FIXED_PIPELINE_STACK_BYTES)
        .spawn(run_double_submit_inner)
    {
        Ok(worker) => match worker.join() {
            Ok(outcome) => outcome,
            Err(_) => CommandOutcome::failure(20, "fixed offline pipeline panicked"),
        },
        Err(_) => CommandOutcome::failure(4, "unable to start fixed offline pipeline"),
    }
}

// Keep the short command dispatch frame separate from the opaque, owning
// source/target continuation. Every large typestate value below is boxed as
// it crosses a phase boundary, so normal command and test-thread stacks use
// the same bounded layout without changing Core authority semantics.
#[inline(never)]
fn run_double_submit_inner() -> CommandOutcome {
    let workspace = match tempfile::tempdir() {
        Ok(workspace) => workspace,
        Err(_) => return CommandOutcome::failure(4, "unable to create isolated fixture workspace"),
    };
    let root = match StoreRoot::open(workspace.path(), StoreLimits::default()) {
        Ok(root) => root,
        Err(_) => return CommandOutcome::failure(4, "unable to open isolated fixture store"),
    };
    let fixture =
        match reviewgraphen_runtime::fixed_offline::materialize_double_submit_m4_prefix_v4(&root) {
            Ok(fixture) => fixture,
            Err(_) => return CommandOutcome::failure(20, "fixed fixture materialization failed"),
        };
    let (journal, base_roots, assignments) = fixture.into_parts();
    if run_double_submit_payment_profile_conflict_v4(
        &journal,
        match base_roots.build() {
            Ok(value) => value,
            Err(_) => return CommandOutcome::failure(20, "fixture roots are invalid"),
        },
        match assignments.build() {
            Ok(value) => value,
            Err(_) => return CommandOutcome::failure(20, "fixture assignments are invalid"),
        },
    )
    .is_err()
    {
        return CommandOutcome::failure(20, "fixed gluing runtime failed");
    }
    let source_authority = match journal.inspect_m5_report_authority_v4(
        match base_roots.build() {
            Ok(value) => value,
            Err(_) => return CommandOutcome::failure(20, "fixture roots are invalid"),
        },
        match assignments.build() {
            Ok(value) => value,
            Err(_) => return CommandOutcome::failure(20, "fixture assignments are invalid"),
        },
    ) {
        Ok(M5ReportAuthorityInspectionV4::Complete(authority)) => authority,
        Ok(M5ReportAuthorityInspectionV4::Incomplete { .. }) => {
            return CommandOutcome::failure(20, "fixed runtime left M5 authority incomplete");
        }
        Err(_) => {
            return CommandOutcome::failure(4, "unable to inspect completed fixture authority");
        }
    };
    let source_index = match DerivedIndexV5::open(&root) {
        Ok(index) => index,
        Err(_) => return CommandOutcome::failure(4, "unable to open fixture index"),
    };
    if source_authority
        .rebuild_v5(&source_index, &journal)
        .is_err()
    {
        return CommandOutcome::failure(4, "unable to rebuild completed fixture index");
    }
    let target = match reviewgraphen_runtime::fixed_offline::publish_double_submit_target_v5(&root)
    {
        Ok(target) => target,
        Err(_) => return CommandOutcome::failure(20, "fixed target materialization failed"),
    };
    let (target_journal, plan_id, obligation_id) = target.into_parts();
    let target_identity = match target_journal.reader() {
        Ok(reader) => reader.identity().clone(),
        Err(_) => return CommandOutcome::failure(4, "unable to inspect target fixture journal"),
    };
    let target_index = match DerivedIndexV6::open(&root) {
        Ok(index) => index,
        Err(_) => return CommandOutcome::failure(4, "unable to open target fixture index"),
    };
    if target_index
        .rebuild_pre_incremental_v6(&target_journal)
        .is_err()
    {
        return CommandOutcome::failure(4, "unable to rebuild target fixture index");
    }
    let target_snapshot = match target_index.validated_snapshot_current_v6(&target_journal) {
        Ok(snapshot) => snapshot.snapshot().clone(),
        Err(_) => return CommandOutcome::failure(4, "unable to validate target fixture index"),
    };
    let Some(git) = target_snapshot
        .program_space
        .accepted_git_revision_closure()
    else {
        return CommandOutcome::failure(20, "fixed target has no accepted Git closure");
    };
    let accepted = match source_authority.derive_incremental_mapping_v5(
        &journal,
        &source_index,
        &target_journal,
        &target_index,
    ) {
        Ok(value) => Box::new(value),
        Err(_) => return CommandOutcome::failure(20, "fixed incremental mapping failed"),
    };
    let terminal = match accepted.recompute_terminal_m6_no_gluing(
        V5AuthorityTrustInput::new_without_gluing(
            target_snapshot.marker.policy_revision_hash.clone(),
            target_snapshot.repository_id.clone(),
            git.target_tree_hash().clone(),
            Vec::new(),
            Vec::new(),
        ),
        "2026-08-13T00:00:00Z",
    ) {
        Ok(value) => Box::new(value),
        Err(_) => return CommandOutcome::failure(20, "fixed M6 authority recomputation failed"),
    };
    let selected_obligation_ids = terminal
        .partial_actions()
        .iter()
        .flat_map(|action| action.subject_ids().iter().cloned())
        .collect::<BTreeSet<_>>();
    if !terminal.partial_actions().iter().any(|action| {
        action.action() == reviewgraphen_core::PartialRerunActionKindV5::RerunHumanDecision
    }) {
        return CommandOutcome::failure(20, "fixed M6 plan omitted the stale human-decision rerun");
    }
    if selected_obligation_ids.is_empty() || !selected_obligation_ids.contains(&obligation_id) {
        return CommandOutcome::failure(
            20,
            "fixed M6 plan does not cover selected target obligation",
        );
    }
    let partial_members = terminal.partial_actions().len().saturating_add(1);
    let mut incremental = match (*terminal).append_pre_d2_m6() {
        Ok(value) => Box::new(value),
        Err(_) => return CommandOutcome::failure(20, "fixed M6 prefix append failed"),
    };
    for _ in 0..partial_members {
        incremental = match (*incremental).append_next_partial() {
            Ok(value) => Box::new(value),
            Err(_) => return CommandOutcome::failure(20, "fixed M6 partial rerun failed"),
        };
    }
    let mut reviewer = match (*incremental).begin_post_partial() {
        Ok(value) => Box::new(value),
        Err(_) => return CommandOutcome::failure(20, "fixed reviewer phase failed"),
    };
    let mut raw = None;
    let reviewer_step_limit = partial_members.saturating_mul(7).saturating_add(1);
    for _ in 0..reviewer_step_limit {
        match reviewer.next_reviewer_work() {
            Ok(M6ReviewerWorkRequestV5::Automatic) => match (*reviewer).append_reviewer_automatic()
            {
                Ok(value) => *reviewer = value,
                Err(_) => return CommandOutcome::failure(20, "fixed reviewer automation failed"),
            },
            Ok(M6ReviewerWorkRequestV5::RawReviewerResponse(work)) => {
                let bytes = match canonical_reviewer_response(&work) {
                    Ok(value) => value,
                    Err(_) => {
                        return CommandOutcome::failure(20, "fixed reviewer response is invalid");
                    }
                };
                raw = Some(bytes.clone());
                reviewer = match (*reviewer).append_reviewer_raw(&bytes) {
                    Ok(value) => Box::new(value),
                    Err(_) => {
                        return CommandOutcome::failure(20, "fixed reviewer raw admission failed");
                    }
                };
            }
            Ok(M6ReviewerWorkRequestV5::StructuredReviewerResult(_)) => {
                let Some(bytes) = raw.take() else {
                    return CommandOutcome::failure(
                        20,
                        "fixed reviewer execution lacks raw response",
                    );
                };
                let parsed = match ValidatedFixedM6ReviewerResultV5::from_canonical_bytes(&bytes) {
                    Ok(value) => value,
                    Err(_) => {
                        return CommandOutcome::failure(20, "fixed reviewer result is invalid");
                    }
                };
                reviewer = match (*reviewer).append_fixed_reviewer_execution(parsed) {
                    Ok(value) => Box::new(value),
                    Err(_) => {
                        return CommandOutcome::failure(
                            20,
                            "fixed reviewer execution admission failed",
                        );
                    }
                };
            }
            Ok(M6ReviewerWorkRequestV5::Complete) => break,
            Err(_) => return CommandOutcome::failure(20, "fixed reviewer work resolution failed"),
        }
    }
    if !matches!(
        reviewer.next_reviewer_work(),
        Ok(M6ReviewerWorkRequestV5::Complete)
    ) {
        return CommandOutcome::failure(20, "fixed reviewer phase did not terminate");
    }
    let mut gluing = match (*reviewer).begin_gluing() {
        Ok(value) => Box::new(value),
        Err(error) => {
            return CommandOutcome::failure(20, format!("fixed gluing phase failed: {error}"));
        }
    };
    // Core derives every member; Runtime only persists that exact sequence
    // before the first descriptor coordinate becomes observable.
    for _ in 0..gluing.gluing_member_count() {
        gluing = match (*gluing).append_next() {
            Ok(value) => Box::new(value),
            Err(_) => return CommandOutcome::failure(20, "fixed target gluing rerun failed"),
        };
    }
    // The host may only bind the exact, Core-derived verifier contracts.  Do
    // this before opening native verification so every selected fixture root
    // is present, without guessing a claim ID or body hash.
    let harness_requests = match gluing.harness_work_requests() {
        Ok(requests) if !requests.is_empty() => requests,
        _ => return CommandOutcome::failure(20, "fixed native verifier has no fixture request"),
    };
    for harness_request in &harness_requests {
        let witness = reviewgraphen_verifier::FIXTURE_WITNESS_BYTES;
        let witness_hash = ContentHash::sha256(witness);
        if witness_hash.as_str() != reviewgraphen_core::FIXTURE_WITNESS_HASH {
            return CommandOutcome::failure(20, "fixed fixture witness constant is invalid");
        }
        let witness_cas = match reviewgraphen_store::CasStore::open(&root) {
            Ok(value) => value,
            Err(_) => return CommandOutcome::failure(4, "unable to open fixed fixture CAS"),
        };
        if witness_cas
            .put(
                &match reviewgraphen_store::CasHash::parse(witness_hash.to_string()) {
                    Ok(value) => value,
                    Err(_) => return CommandOutcome::failure(20, "fixed fixture hash is invalid"),
                },
                Some(u64::try_from(witness.len()).expect("fixed witness size")),
                std::io::Cursor::new(witness),
            )
            .is_err()
        {
            return CommandOutcome::failure(4, "unable to publish fixed fixture witness");
        }
        if ContentHash::sha256(harness_request.output_bytes()) != *harness_request.output_hash() {
            return CommandOutcome::failure(20, "fixed fixture output contract is invalid");
        }
        if witness_cas
            .put(
                &match reviewgraphen_store::CasHash::parse(
                    harness_request.output_hash().to_string(),
                ) {
                    Ok(value) => value,
                    Err(_) => {
                        return CommandOutcome::failure(20, "fixed fixture output hash is invalid");
                    }
                },
                Some(
                    u64::try_from(harness_request.output_bytes().len())
                        .expect("bounded fixed fixture output"),
                ),
                std::io::Cursor::new(harness_request.output_bytes()),
            )
            .is_err()
        {
            return CommandOutcome::failure(4, "unable to publish fixed fixture output");
        }
        gluing = match (*gluing).with_harness(fixed_harness(
            &target_identity,
            &target_snapshot,
            harness_request,
        )) {
            Ok(value) => Box::new(value),
            Err(_) => return CommandOutcome::failure(20, "fixed harness root is invalid"),
        };
    }
    let mut native = match (*gluing).begin_native() {
        Ok(value) => Box::new(value),
        Err(_) => return CommandOutcome::failure(20, "fixed native verifier phase failed"),
    };
    let native_step_limit = partial_members.saturating_mul(5);
    for _ in 0..native_step_limit {
        if let Ok(Some(request)) = native.next_static_work_request() {
            let input = match StaticFactInputV1::from_json_bytes(request.input_bytes()) {
                Ok(value) => value,
                Err(_) => {
                    return CommandOutcome::failure(20, "fixed static verifier input is invalid");
                }
            };
            let output = match evaluate_static_fact_result_from_input_v1(&input)
                .and_then(|value| value.canonical_bytes())
            {
                Ok(value) => value,
                Err(_) => {
                    return CommandOutcome::failure(20, "fixed static verifier evaluation failed");
                }
            };
            let result = match ValidatedM6StaticResultV5::from_canonical_bytes(
                request.input_bytes(),
                &output,
            ) {
                Ok(value) => value,
                Err(_) => {
                    return CommandOutcome::failure(20, "fixed static verifier result is invalid");
                }
            };
            native = match (*native).append_static_result(result) {
                Ok(value) => Box::new(value),
                Err(_) => return CommandOutcome::failure(20, "fixed static verification failed"),
            };
        } else if matches!(native.next_harness_work_request(), Ok(Some(_))) {
            native = match (*native).append_next() {
                Ok(value) => Box::new(value),
                Err(error) => {
                    return CommandOutcome::failure(
                        20,
                        format!("fixed native verification failed: {error}"),
                    );
                }
            };
        } else {
            break;
        }
    }
    if !matches!(native.next_static_work_request(), Ok(None))
        || !matches!(native.next_harness_work_request(), Ok(None))
    {
        return CommandOutcome::failure(20, "fixed native verification did not terminate");
    }
    let grant = fixed_human_grant(&target_identity, &target_snapshot, &harness_requests);
    let mut human = match (*native)
        .begin_human()
        .and_then(|value| value.with_human_grant(grant.clone()))
    {
        Ok(value) => Box::new(value),
        Err(error) => {
            return CommandOutcome::failure(
                20,
                format!("fixed human resolution phase failed: {error}"),
            );
        }
    };
    let human_step_limit = partial_members.saturating_mul(2).saturating_add(1);
    for _ in 0..human_step_limit {
        match human.next_human_work() {
            M6HumanWorkRequestV5::Decision => {
                human = match (*human).append_decision(FixedHumanDecisionV5::new(
                    grant.clone(),
                    DecisionInputV3::new(
                        DecisionOutcomeV3::Accept,
                        "human:fixed-offline",
                        "fixed-offline-review-board",
                        "fixed offline fixture accepts reproduced issue",
                        "2026-08-13T00:00:00Z",
                        None,
                    ),
                )) {
                    Ok(value) => Box::new(value),
                    Err(_) => return CommandOutcome::failure(20, "fixed human decision failed"),
                };
            }
            M6HumanWorkRequestV5::DerivedFinding => match (*human).append_derived_finding() {
                Ok(value) => *human = value,
                Err(_) => return CommandOutcome::failure(20, "fixed finding derivation failed"),
            },
            M6HumanWorkRequestV5::Complete => break,
        }
    }
    if human.next_human_work() != M6HumanWorkRequestV5::Complete {
        return CommandOutcome::failure(20, "fixed human resolution did not terminate");
    }
    let mut m5 = match (*human).begin_m5() {
        Ok(value) => Box::new(value),
        Err(_) => return CommandOutcome::failure(20, "fixed terminal M5 phase failed"),
    };
    let first_request = match m5.next_gluing_work_request() {
        Ok(Some(value)) => value,
        _ => return CommandOutcome::failure(20, "fixed first target gluing request failed"),
    };
    m5 = match (*m5).with_gluing_input(fixed_gluing_input(
        &root,
        &first_request,
        target_snapshot.marker.policy_revision_hash.clone(),
        target_snapshot.repository_id.clone(),
        git.target_tree_hash().clone(),
    )) {
        Ok(value) => Box::new(value),
        Err(_) => return CommandOutcome::failure(20, "fixed first target gluing input failed"),
    };
    match m5.append_next() {
        Ok(true) => {}
        Ok(false) => {
            return CommandOutcome::failure(
                20,
                "fixed first target gluing registration was absent",
            );
        }
        Err(error) => {
            return CommandOutcome::failure(
                20,
                format!("fixed first target gluing registration failed: {error}"),
            );
        }
    }
    let second_request = match m5.next_gluing_work_request() {
        Ok(Some(value)) => value,
        _ => return CommandOutcome::failure(20, "fixed second target gluing request failed"),
    };
    m5 = match (*m5).with_gluing_input(fixed_gluing_input(
        &root,
        &second_request,
        target_snapshot.marker.policy_revision_hash.clone(),
        target_snapshot.repository_id.clone(),
        git.target_tree_hash().clone(),
    )) {
        Ok(value) => Box::new(value),
        Err(_) => return CommandOutcome::failure(20, "fixed second target gluing input failed"),
    };
    if !matches!(m5.append_next(), Ok(true))
        || !matches!(m5.next_gluing_work_request(), Ok(None))
        || !matches!(m5.append_next(), Ok(true))
        || !matches!(m5.append_next(), Ok(false))
    {
        return CommandOutcome::failure(20, "fixed terminal M5 route is not complete");
    }
    // The terminal proof must outlive, but not retain, the mutable
    // incremental continuation. `append_terminal_marker` consumes the M5
    // owner; this scope makes that target replay/session release explicit
    // before the dual-lock terminal report admission below.
    let persisted = {
        let terminal_m5 = m5;
        match (*terminal_m5).append_terminal_marker() {
            Ok(value) => value,
            Err(error) => {
                return CommandOutcome::failure(
                    20,
                    format!("fixed terminal marker failed: {error}"),
                );
            }
        }
    };
    // A V6 index handle is stateless, but reopening it here documents and
    // enforces the terminal lifecycle boundary: authority receives a fresh
    // Store-root-bound index handle after the mutable continuation is gone.
    let target_index = match DerivedIndexV6::open(&root) {
        Ok(index) => index,
        Err(_) => return CommandOutcome::failure(4, "unable to reopen terminal fixture index"),
    };
    let authority = match source_authority
        .terminal_report_authority_from_persisted_with_terminal_index_v5(
            &journal,
            &source_index,
            &target_journal,
            &target_index,
            &persisted,
        ) {
        Ok(value) => value,
        Err(error) => {
            return CommandOutcome::failure(
                20,
                format!("fixed terminal report authority failed: {error}"),
            );
        }
    };
    #[cfg(test)]
    let terminal_index_coordinates = match authority.terminal_index_rebuild_receipt() {
        Ok(receipt) => (
            receipt.rebuild.confirmed_offset,
            receipt.rebuild.event_count,
        ),
        Err(error) => {
            return CommandOutcome::failure(
                20,
                format!("fixed terminal index receipt failed: {error}"),
            );
        }
    };
    let request = ReportRequestV5 {
        report_id: StableId::parse("report:cli-double-submit-v5").expect("static report ID"),
        target_plan_id: plan_id,
        selected_obligation_ids,
        tool_versions: BTreeMap::from([("reviewgraphen.cli".to_owned(), "0.1.0".to_owned())]),
    };
    match generate_v5(authority, &request) {
        Ok(report) => {
            #[cfg(test)]
            assert_fixed_terminal_offset_v5(&report.canonical_bytes, terminal_index_coordinates);
            CommandOutcome::success(
                report.canonical_bytes,
                "running fixed offline double-submit V5 pipeline",
            )
        }
        Err(error) => CommandOutcome::failure(
            20,
            format!("source-bound fixed report generation failed: {error}"),
        ),
    }
}

#[cfg(test)]
fn assert_fixed_terminal_offset_v5(bytes: &[u8], expected: (u64, u64)) {
    let report: Value = serde_json::from_slice(bytes).expect("fixed V5 report JSON");
    let offset = report
        .pointer("/metadata/confirmed_offset")
        .and_then(Value::as_u64)
        .expect("fixed V5 confirmed byte offset");
    let event_count = report
        .pointer("/metadata/confirmed_event_count")
        .and_then(Value::as_u64)
        .expect("fixed V5 confirmed event count");
    assert_eq!(offset, expected.0, "report offset differs from V6 receipt");
    assert_eq!(
        event_count, expected.1,
        "report count differs from V6 receipt"
    );
    assert_ne!(
        offset, event_count,
        "fixture must distinguish LF-inclusive byte offset from event count"
    );
}

fn fixed_gluing_input(
    root: &StoreRoot,
    request: &M5GluingWorkRequestV5,
    policy_revision_hash: ContentHash,
    repository_id: StableId,
    repository_source_hash: ContentHash,
) -> V5GluingInputTrustInput {
    let descriptor = GluingInputDescriptorV4::new(
        request.run_id().clone(),
        request.snapshot_id().clone(),
        request.universe_id().clone(),
        request.plan_id().clone(),
        request.context_id().clone(),
        AssignmentValueV4::Satisfied,
        BTreeSet::new(),
    )
    .expect("fixed target gluing descriptor");
    let bytes = canonical_json(&descriptor).expect("canonical fixed gluing descriptor");
    let descriptor_hash = ContentHash::sha256(&bytes);
    let descriptor_size = u64::try_from(bytes.len()).expect("fixed descriptor size");
    reviewgraphen_store::CasStore::open(root)
        .expect("fixed target CAS")
        .put(
            &reviewgraphen_store::CasHash::parse(descriptor_hash.to_string())
                .expect("fixed descriptor CAS hash"),
            Some(descriptor_size),
            std::io::Cursor::new(bytes),
        )
        .expect("fixed target descriptor CAS write");
    let source = ArtifactSourceV4::GluingInput {
        context_id: request.context_id().clone(),
        descriptor_hash: descriptor_hash.clone(),
        descriptor_id: descriptor.id().clone(),
        descriptor_media_type: request.descriptor_media_type().to_owned(),
        descriptor_sensitivity: request.descriptor_sensitivity(),
        descriptor_size,
        genesis_hash: request.genesis_hash().clone(),
        plan_id: request.plan_id().clone(),
        policy_revision_hash: policy_revision_hash.clone(),
        profile_descriptor_id: request.profile_descriptor_id().to_owned(),
        repository_id: repository_id.clone(),
        repository_source_hash: repository_source_hash.clone(),
        run_id: request.run_id().clone(),
        snapshot_id: request.snapshot_id().clone(),
        universe_id: request.universe_id().clone(),
    };
    V5GluingInputTrustInput::new(
        policy_revision_hash,
        repository_id,
        repository_source_hash,
        request.run_id().clone(),
        request.genesis_hash().clone(),
        request.snapshot_id().clone(),
        request.universe_id().clone(),
        request.plan_id().clone(),
        request.profile_descriptor_id(),
        request.context_id().clone(),
        descriptor.id().clone(),
        descriptor_hash,
        descriptor_size,
        request.descriptor_media_type(),
        request.descriptor_sensitivity(),
        source,
        request.predecessor_event_hash().clone(),
        request.event_sequence(),
    )
    .expect("fixed V5 positioned gluing trust input")
}

fn canonical_reviewer_response(
    request: &reviewgraphen_core::M6ReviewerRuntimeRequestV5,
) -> Result<Vec<u8>, reviewgraphen_core::DomainError> {
    // The two frozen M5 contexts keep their named targets. Other Core-derived
    // partial-rerun contexts use their first canonical allowed target/source
    // pair so every scheduled human-decision prerequisite has a real claim
    // and native verification. This remains a closed fake-reviewer fixture:
    // no target or source outside Core's request can enter the claim.
    let scoped_claim = if request
        .allowed_target_refs()
        .contains(&StableId::parse("relation:payment-calls-stripe")?)
    {
        Some((
            StableId::parse("relation:payment-calls-stripe")?,
            StableId::parse("file:payment-repository")?,
        ))
    } else if request
        .allowed_target_refs()
        .contains(&StableId::parse("relation:tap-handled-by-submit")?)
    {
        Some((
            StableId::parse("relation:tap-handled-by-submit")?,
            StableId::parse("file:checkout-controller")?,
        ))
    } else {
        request
            .allowed_target_refs()
            .iter()
            .next()
            .cloned()
            .zip(request.allowed_source_ids().iter().next().cloned())
    };
    let (abstention, claims) = if let Some((target_ref, source_id)) = scoped_claim {
        if !request.allowed_source_ids().contains(&source_id) {
            return Err(reviewgraphen_core::DomainError::Validation(
                "fixed reviewer context lacks its selected source".to_owned(),
            ));
        }
        (
            None,
            vec![json!({
                "assumptions": [],
                "candidate_confidence": 1.0,
                "polarity": "issue_present",
                "property_id": request.property_id(),
                "requested_evidence": [],
                "source_ids": [source_id],
                "summary": "fixed offline M6 reviewer fixture",
                "target_refs": [target_ref],
            })],
        )
    } else {
        (
            Some(json!({
                "detail": "fixed offline fixture does not select this local review action",
                "reason": "property_not_understood",
            })),
            Vec::<Value>::new(),
        )
    };
    canonical_json(&json!({
        "abstention": abstention,
        "claims": claims,
        "execution_id": request.execution_id(),
        "schema": "reviewgraphen.reviewer_output.v1",
    }))
}

fn fixed_harness(
    identity: &reviewgraphen_store::JournalIdentity,
    snapshot: &reviewgraphen_store::PreIncrementalIndexSnapshotV6,
    request: &reviewgraphen_core::M6NativeHarnessWorkRequestV5,
) -> AuthorityHarnessBindingV3Tuple {
    AuthorityHarnessBindingV3Tuple {
        policy_revision_hash: snapshot.marker.policy_revision_hash.clone(),
        repository_id: snapshot.repository_id.clone(),
        repository_source_hash: snapshot
            .program_space
            .accepted_git_revision_closure()
            .expect("fixed target has Git closure")
            .target_tree_hash()
            .clone(),
        harness_id: reviewgraphen_core::FIXTURE_HARNESS_ID.to_owned(),
        harness_revision: reviewgraphen_core::FIXTURE_HARNESS_REVISION.to_owned(),
        harness_source_hash: ContentHash::parse(reviewgraphen_core::FIXTURE_HARNESS_SOURCE_HASH)
            .expect("static harness hash"),
        test_artifact_id: StableId::parse(reviewgraphen_core::FIXTURE_TEST_ARTIFACT_ID)
            .expect("static test ID"),
        descriptor_id: reviewgraphen_core::FIXTURE_DESCRIPTOR_ID.to_owned(),
        procedure_version: reviewgraphen_core::FIXTURE_PROCEDURE_ID.to_owned(),
        result_hash: ContentHash::parse(reviewgraphen_core::FIXTURE_WITNESS_HASH)
            .expect("static witness hash"),
        result_size: 145,
        result_media_type: reviewgraphen_core::FIXTURE_MEDIA_TYPE.to_owned(),
        result_sensitivity: ArtifactSensitivity::CanonicalState,
        run_id: identity.run_id.clone(),
        genesis_hash: identity.genesis_hash().clone(),
        snapshot_id: snapshot.snapshot_id.clone(),
        universe_id: snapshot.universe_id.clone(),
        property_id: request.property_id().to_owned(),
        claim_id: request.claim_id().clone(),
        claim_body_hash: request.claim_body_hash().clone(),
    }
}

fn fixed_human_grant(
    identity: &reviewgraphen_store::JournalIdentity,
    snapshot: &reviewgraphen_store::PreIncrementalIndexSnapshotV6,
    requests: &[reviewgraphen_core::M6NativeHarnessWorkRequestV5],
) -> AuthorityHumanGrantV3Tuple {
    AuthorityHumanGrantV3Tuple {
        policy_revision_hash: snapshot.marker.policy_revision_hash.clone(),
        actor: "human:fixed-offline".to_owned(),
        authority_id: "fixed-offline-review-board".to_owned(),
        capabilities: BTreeSet::from([HumanAuthorityCapabilityV3::AcceptFinding]),
        run_id: identity.run_id.clone(),
        snapshot_id: snapshot.snapshot_id.clone(),
        universe_id: snapshot.universe_id.clone(),
        property_ids: requests
            .iter()
            .map(|request| request.property_id().to_owned())
            .collect(),
        claim_ids: requests
            .iter()
            .map(|request| request.claim_id().clone())
            .collect(),
        valid_from: "2026-01-01T00:00:00Z".to_owned(),
        valid_until: "2027-01-01T00:00:00Z".to_owned(),
    }
}

fn validator_for(name: &str) -> Result<Validator, ()> {
    let source = schema_source(name).ok_or(())?;
    let schema: Value = serde_json::from_str(source).map_err(|_| ())?;
    let v3: Value =
        serde_json::from_str(schema_source("reviewgraphen.review.report.v3").ok_or(())?)
            .map_err(|_| ())?;
    let v4: Value =
        serde_json::from_str(schema_source("reviewgraphen.review.report.v4").ok_or(())?)
            .map_err(|_| ())?;
    jsonschema::options()
        .with_resource(V3_URI, Resource::from_contents(v3).map_err(|_| ())?)
        .with_resource(V4_URI, Resource::from_contents(v4).map_err(|_| ())?)
        .build(&schema)
        .map_err(|_| ())
}

fn semantic_validation(name: &str, report: &Value) -> Result<(), ()> {
    match name {
        "reviewgraphen.review.report.v4" => {
            reviewgraphen_report::validate_v4_semantics(report).map_err(|_| ())
        }
        "reviewgraphen.review.report.v5" => {
            reviewgraphen_report::validate_v5_semantics(report).map_err(|_| ())
        }
        _ => Ok(()),
    }
}

#[cfg(target_os = "linux")]
fn bounded_regular_file(path: &Path) -> Result<Vec<u8>, &'static str> {
    // The descriptor is opened with NOFOLLOW, then checked after opening. This
    // binds the file type and byte limit to the object actually read, rather
    // than to an earlier pathname lookup.
    let descriptor = fs::open(
        path,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| "unable to read input file")?;
    let metadata = fs::fstat(&descriptor).map_err(|_| "unable to read input file")?;
    if FileType::from_raw_mode(metadata.st_mode) != FileType::RegularFile
        || metadata.st_size < 0
        || u64::try_from(metadata.st_size)
            .ok()
            .is_none_or(|size| size > MAX_INPUT_BYTES)
    {
        return Err("input must be a bounded regular file");
    }
    let mut file = std::fs::File::from(descriptor);
    let declared_size = usize::try_from(metadata.st_size).map_err(|_| "input is too large")?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(declared_size)
        .map_err(|_| "input is too large")?;
    file.by_ref()
        .take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => "unable to read input file",
            _ => "unable to read input file",
        })?;
    if bytes.len() > usize::try_from(MAX_INPUT_BYTES).unwrap_or(usize::MAX) {
        return Err("input must be a bounded regular file");
    }
    Ok(bytes)
}

#[cfg(not(target_os = "linux"))]
fn bounded_regular_file(_path: &Path) -> Result<Vec<u8>, &'static str> {
    // The documented CLI contract requires descriptor-safe path handling.
    Err("descriptor-safe file reads are unsupported on this platform")
}

fn schema_source(name: &str) -> Option<&'static str> {
    match name {
        "reviewgraphen.review.report.v1" => Some(include_str!(
            "../../../schemas/reviewgraphen.report.schema.json"
        )),
        "reviewgraphen.review.report.v2" => Some(include_str!(
            "../../../schemas/reviewgraphen.report.v2.schema.json"
        )),
        "reviewgraphen.review.report.v3" => Some(include_str!(
            "../../../schemas/reviewgraphen.report.v3.schema.json"
        )),
        "reviewgraphen.review.report.v4" => Some(include_str!(
            "../../../schemas/reviewgraphen.report.v4.schema.json"
        )),
        "reviewgraphen.review.report.v5" => Some(include_str!(
            "../../../schemas/reviewgraphen.report.v5.schema.json"
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_surface_is_closed_and_canonical() {
        let list = run(vec!["schema".into(), "list".into()]);
        assert_eq!(list.exit_code, 0);
        assert_eq!(
            serde_json::from_slice::<Value>(&list.stdout).unwrap(),
            json!([
                "reviewgraphen.review.report.v1",
                "reviewgraphen.review.report.v2",
                "reviewgraphen.review.report.v3",
                "reviewgraphen.review.report.v4",
                "reviewgraphen.review.report.v5"
            ])
        );
        let printed = run(vec![
            "schema".into(),
            "print".into(),
            "reviewgraphen.review.report.v5".into(),
        ]);
        assert_eq!(printed.exit_code, 0);
        assert_eq!(
            serde_json::from_slice::<Value>(&printed.stdout)
                .unwrap()
                .get("$id")
                .and_then(Value::as_str),
            Some("https://capht.tech/schemas/reviewgraphen/review-report.v5.schema.json")
        );
        assert_eq!(run(vec!["review".into()]).exit_code, 2);
    }

    #[test]
    fn detached_report_gate_command_is_not_supported() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            file.path(),
            br#"{"schema":"reviewgraphen.review.report.v5","gate":{"status":"pass"}}"#,
        )
        .unwrap();
        let result = run(vec!["gate".into(), file.path().display().to_string()]);
        assert_eq!(result.exit_code, 2);
        assert!(result.stdout.is_empty());
        assert!(!result.stderr.contains("gate <report.json>"));

        let nonexistent = run(vec![
            "gate".into(),
            "/path/that/must/not/be/read.json".into(),
        ]);
        assert_eq!(nonexistent.exit_code, 2);
        assert_eq!(nonexistent.stderr, result.stderr);
    }

    #[test]
    fn fixed_terminal_v5_report_is_schema_semantic_and_byte_deterministic() {
        let first = run(vec![
            "review".into(),
            "--fixture".into(),
            "double-submit".into(),
        ]);
        assert_eq!(first.exit_code, 0, "{}", first.stderr);
        let second = run(vec![
            "review".into(),
            "--fixture".into(),
            "double-submit".into(),
        ]);
        assert_eq!(second.exit_code, 0, "{}", second.stderr);
        assert_eq!(
            first.stdout, second.stdout,
            "fixed terminal report bytes drift"
        );

        let report: Value = serde_json::from_slice(&first.stdout).unwrap();
        let validator = validator_for("reviewgraphen.review.report.v5").unwrap();
        assert!(validator.is_valid(&report));
        reviewgraphen_report::validate_v5_semantics(&report).unwrap();
        let result = report.get("result").and_then(Value::as_object).unwrap();
        for family in [
            "partial_rerun_actions",
            "context_covers",
            "sections",
            "gluing_attempts",
        ] {
            assert!(
                result
                    .get(family)
                    .and_then(Value::as_array)
                    .is_some_and(|rows| !rows.is_empty()),
                "fixed required-gluing terminal report omitted {family}"
            );
        }
        let gate = report.get("gate").and_then(Value::as_object).unwrap();
        assert_eq!(gate.get("status").and_then(Value::as_str), Some("blocked"));
        assert!(
            gate.get("blocking_ids")
                .and_then(Value::as_array)
                .is_some_and(|ids| !ids.is_empty()),
            "accepted current finding did not block the gate"
        );
        assert!(
            gate.get("reasons")
                .and_then(Value::as_array)
                .is_some_and(|reasons| !reasons.is_empty()),
            "blocked gate omitted its reason"
        );
        let coverage = report.get("coverage").and_then(Value::as_object).unwrap();
        for axis in [
            "evidence_supported_obligation_ids",
            "verified_obligation_ids",
            "accepted_obligation_ids",
        ] {
            assert!(
                coverage.get(axis).is_some_and(Value::is_array),
                "coverage omitted distinct {axis} axis"
            );
        }

        let forge_pass = |value: &mut Value| {
            let gate = value
                .get_mut("gate")
                .and_then(Value::as_object_mut)
                .unwrap();
            gate.insert("status".to_owned(), Value::String("pass".to_owned()));
            for key in ["blocking_ids", "incomplete_ids", "reasons"] {
                gate.insert(key.to_owned(), Value::Array(Vec::new()));
            }
        };
        let mut forged_gate = report.clone();
        forge_pass(&mut forged_gate);
        assert!(
            reviewgraphen_report::validate_v5_semantics(&forged_gate).is_err(),
            "a locally coherent pass rewrite retained the old reduction identity"
        );

        let mut forged_freshness = report.clone();
        let forged_coverage = forged_freshness
            .get_mut("coverage")
            .and_then(Value::as_object_mut)
            .unwrap();
        let denominator = forged_coverage["denominator_obligation_ids"].clone();
        let count = denominator.as_array().unwrap().len() as u64;
        for (ids_key, count_key) in [
            ("native_verified_obligation_ids", "native_verified"),
            ("verified_obligation_ids", "verified"),
            ("fresh_verified_obligation_ids", "fresh_verified"),
        ] {
            forged_coverage.insert(ids_key.to_owned(), denominator.clone());
            forged_coverage.insert(count_key.to_owned(), Value::from(count));
        }
        forge_pass(&mut forged_freshness);
        assert!(
            reviewgraphen_report::validate_v5_semantics(&forged_freshness).is_err(),
            "coverage-local freshness could be promoted without native evidence closure"
        );
    }
}
