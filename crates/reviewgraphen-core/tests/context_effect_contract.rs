use reviewgraphen_core::{ContextBuildEffect, ContextBuildProbe, ContextBuildTrace, StableId};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const FAIL_FAST_CHILD_TEST: &str =
    "context::tests::v2_effect_preflight_stops_on_the_4097th_candidate_without_downstream_effects";

fn prebuilt_core_unit_test_binary() -> PathBuf {
    let current = std::env::current_exe().expect("current integration-test binary");
    let deps = current.parent().expect("target deps directory");
    let mut candidates = fs::read_dir(deps)
        .expect("read target deps directory")
        .map(|entry| entry.expect("target deps entry").path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("reviewgraphen_core-"))
                && path.is_file()
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|path| {
        std::cmp::Reverse(
            fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .expect("unit-test binary modification time"),
        )
    });
    candidates
        .into_iter()
        .find(|candidate| {
            Command::new(candidate)
                .args(["--list", "--format", "terse"])
                .output()
                .is_ok_and(|output| {
                    output.status.success()
                        && String::from_utf8_lossy(&output.stdout).contains(FAIL_FAST_CHILD_TEST)
                })
        })
        .expect("prebuilt core unit-test binary containing the fail-fast fixture")
}

#[test]
fn exported_probe_is_ordered_read_only_and_byte_blind() {
    let trace = ContextBuildTrace::default();
    let id = StableId::parse("file:subject").unwrap();
    trace.observe(ContextBuildEffect::CandidateMaterialized {
        artifact_id: id.clone(),
    });
    trace.observe(ContextBuildEffect::SourceBytesRequested {
        artifact_id: id.clone(),
    });
    trace.observe(ContextBuildEffect::SourceSubmitted { artifact_id: id });
    assert!(matches!(
        trace.snapshot().as_slice(),
        [
            ContextBuildEffect::CandidateMaterialized { .. },
            ContextBuildEffect::SourceBytesRequested { .. },
            ContextBuildEffect::SourceSubmitted { .. }
        ]
    ));
}

#[test]
fn prebuilt_v2_fail_fast_child_is_reaped_before_the_ten_second_watchdog() {
    let binary = prebuilt_core_unit_test_binary();
    let mut child = Command::new(binary)
        .args([FAIL_FAST_CHILD_TEST, "--exact"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn prebuilt post-ingest child");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().expect("poll fail-fast child") {
            assert!(
                status.success(),
                "prebuilt post-ingest child failed: {status}"
            );
            break;
        }
        if Instant::now() >= deadline {
            child.kill().expect("kill runaway post-ingest child");
            let status = child.wait().expect("reap runaway post-ingest child");
            panic!("runaway: prebuilt post-ingest child exceeded 10s and was reaped as {status}");
        }
        thread::sleep(Duration::from_millis(10));
    }
}
