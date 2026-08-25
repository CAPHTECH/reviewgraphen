use reviewgraphen_core::{ContentHash, canonical_json};
use reviewgraphen_runtime::{
    diagnostics::{
        GENERIC_REVIEW_DIAGNOSTICS_SCHEMA, GENERIC_REVIEW_STAGE_OBSERVER_API,
        GenericReviewDiagnosticsCollector, GenericReviewStage, GenericReviewStageEvent,
        GenericReviewStageObserver, MonotonicMicrosecondClock,
    },
    generic::{
        GENERIC_REVIEW_REQUEST_V3_SCHEMA, GenericIngestRequestV2, GenericObserverRequestV2,
        GenericPlanRequest, GenericReviewRequestV3, run_generic_review_v3,
        run_generic_review_v3_with_observer,
    },
};
use std::{collections::VecDeque, fs, path::Path, process::Command};
use tempfile::tempdir;

struct FakeClock(VecDeque<u64>);
impl MonotonicMicrosecondClock for FakeClock {
    fn now_microseconds(&mut self) -> u64 {
        self.0.pop_front().expect("literal fake-clock schedule")
    }
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "Diagnostics Test")
        .env("GIT_AUTHOR_EMAIL", "diagnostics@example.invalid")
        .env("GIT_COMMITTER_NAME", "Diagnostics Test")
        .env("GIT_COMMITTER_EMAIL", "diagnostics@example.invalid")
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn fake_clock_diagnostic_is_separate_and_canonical_run_hash_is_unchanged() {
    let workspace = tempdir().unwrap();
    let repository = workspace.path().join("repository");
    fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "-q"]);
    fs::write(
        repository.join("lib.rs"),
        "pub fn caller() -> u64 { callee() }\npub fn callee() -> u64 { 1 }\n",
    )
    .unwrap();
    git(&repository, &["add", "lib.rs"]);
    git(&repository, &["commit", "-q", "-m", "base"]);
    fs::write(
        repository.join("lib.rs"),
        "pub fn caller() -> u64 { callee() }\npub fn callee() -> u64 { 2 }\n",
    )
    .unwrap();
    git(&repository, &["add", "lib.rs"]);
    git(&repository, &["commit", "-q", "-m", "target"]);
    let request = GenericReviewRequestV3 {
        schema: GENERIC_REVIEW_REQUEST_V3_SCHEMA.into(),
        workspace_admission_root: workspace.path().to_path_buf(),
        repository_admission_root: repository,
        repository_identity: "diagnostics-fixture".into(),
        base_revision: "HEAD~1".into(),
        target_revision: "HEAD".into(),
        ingest: GenericIngestRequestV2 {
            profile_id: "rust.production.v1".into(),
            profile_version: "1".into(),
            rule_set_hash: ContentHash::sha256(b"diagnostics-test-rules"),
            max_files: 8,
            max_file_bytes: 1024 * 1024,
            max_total_source_bytes: 1024 * 1024,
        },
        plan: GenericPlanRequest {
            max_waves: 8,
            max_obligations_per_wave: 32,
        },
        observer: GenericObserverRequestV2::DeterministicAbstain,
        verifier_descriptor_id: None,
        context_policy_id: "context.subject_windows@3".into(),
    };
    let before = run_generic_review_v3(&request).unwrap();
    let before_bytes = before.canonical_bytes().unwrap();
    let before_hash = ContentHash::sha256(&before_bytes);

    let ticks = [100, 103, 200, 207, 300, 311, 400, 413, 500, 517, 600, 619];
    let mut collector = GenericReviewDiagnosticsCollector::new(FakeClock(ticks.into()));
    let after = run_generic_review_v3_with_observer(&request, None, &mut collector).unwrap();
    // The enclosing coordinator owns these two real operations and continues
    // the same observer seam exported by runtime.
    collector.observe(GenericReviewStageEvent::Begin(GenericReviewStage::Report));
    let reported_bytes = after.canonical_bytes().unwrap();
    collector.observe(GenericReviewStageEvent::Completed(
        GenericReviewStage::Report,
    ));
    collector.observe(GenericReviewStageEvent::Begin(
        GenericReviewStage::ArtifactWrite,
    ));
    let diagnostic_artifact = workspace.path().join("separate-diagnostic-target");
    fs::write(&diagnostic_artifact, &reported_bytes).unwrap();
    collector.observe(GenericReviewStageEvent::Completed(
        GenericReviewStage::ArtifactWrite,
    ));
    let diagnostic = collector
        .finish(
            before.value()["request_id"].as_str().map(str::to_owned),
            before.value()["run_id"].as_str().map(str::to_owned),
            "success",
        )
        .unwrap();
    assert_eq!(diagnostic.schema, GENERIC_REVIEW_DIAGNOSTICS_SCHEMA);
    assert_eq!(diagnostic.observer_api, GENERIC_REVIEW_STAGE_OBSERVER_API);
    assert_eq!(
        diagnostic
            .stages
            .iter()
            .map(|row| row.stage)
            .collect::<Vec<_>>(),
        GenericReviewStage::ORDERED
    );
    assert_eq!(
        diagnostic
            .stages
            .iter()
            .map(|row| row.elapsed_microseconds)
            .collect::<Vec<_>>(),
        vec![3, 7, 11, 13, 17, 19]
    );
    let diagnostic_value: serde_json::Value =
        serde_json::from_slice(&diagnostic.canonical_bytes().unwrap()).unwrap();
    for forbidden in [
        "id",
        "stable_id",
        "hash",
        "authority",
        "evidence",
        "coverage",
    ] {
        assert!(diagnostic_value.get(forbidden).is_none());
    }

    assert_eq!(after.canonical_bytes().unwrap(), before_bytes);
    assert_eq!(
        ContentHash::sha256(&after.canonical_bytes().unwrap()),
        before_hash
    );
    assert_eq!(
        canonical_json(&after).unwrap(),
        canonical_json(&before).unwrap()
    );
}
