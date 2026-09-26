//! G7-R1 acceptance SUPPLEMENT-1, Q5–Q7 and Q10 (Acc-R1, frozen before the
//! seam exists).
//!
//! Seam interface `[R]` (implementer adds it to `crates/reviewgraphen-cli/src/lib.rs`,
//! `#[cfg(test)]` only, absent from non-test builds, thread-local, identity
//! when unset, each guard unsets its hook on drop):
//!
//! ```ignore
//! #[cfg(test)]
//! pub(crate) mod g7_r1_seam {
//!     /// Q5–Q7. Checked in each of `generic_review_v2`, `_v3`, `_v4`
//!     /// immediately after `generate_generic_human_report*` returned `Ok`
//!     /// (i.e. after the pre-G7-R1 early admission point, before the manifest,
//!     /// `Begin(ArtifactWrite)` and the late admission). When set, the route
//!     /// emits `Failed(Report)` and returns `CommandOutcome::failure(20, message)`.
//!     pub(crate) fn install_report_stage_fault(message: &'static str) -> impl Drop;
//!     /// Q10. Called once with the admitted root after
//!     /// `admit_fresh_generic_review_artifact_root_v2` succeeded and before the
//!     /// first entry is created by `write_admitted_artifacts_or_unwind`.
//!     pub(crate) fn install_post_admission_hook(hook: Box<dyn FnMut(&Path)>) -> impl Drop;
//! }
//! ```
//!
//! Q10 expected behaviour (`create_new`
//! plus unwind of exactly what this call created, non-recursive, a foreign
//! entry is never removed): a foreign file that appears inside the admitted
//! root under an artifact name is NOT overwritten; that write fails; the route
//! exits 20; every entry this call created is removed; the foreign file keeps
//! its bytes; the root therefore stays (its `remove_dir` fails, not forced),
//! containing exactly the foreign file.

use crate::g7_r1_seam::{install_post_admission_hook, install_report_stage_fault};
use crate::run;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
};
use tempfile::TempDir;

const RULE_SET_HASH: &str =
    "sha256:8f6bfbfb2dbf2f0eaf916b152ddba1e422db8b6de95f931c0c78c9ee4d050b47";
const FAULT: &str = "g7-r1 seam: injected report-stage failure";
const OUT: &str = ".g7-r1-sup1-out";
const FOREIGN: &[u8] = b"foreign bytes that must survive\n";

pub(crate) fn cwd_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "ReviewGraphen Test")
        .env("GIT_AUTHOR_EMAIL", "reviewgraphen@example.invalid")
        .env("GIT_COMMITTER_NAME", "ReviewGraphen Test")
        .env("GIT_COMMITTER_EMAIL", "reviewgraphen@example.invalid")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git is available");
    assert!(output.status.success(), "git failed: {arguments:?}");
    String::from_utf8(output.stdout)
        .expect("git stdout UTF-8")
        .trim()
        .to_owned()
}

struct Fixture {
    temporary: TempDir,
    repository: PathBuf,
    base: String,
    target: String,
}

fn fixture() -> Fixture {
    let temporary = TempDir::new().expect("temporary directory");
    let repository = temporary.path().join("repository");
    fs::create_dir(&repository).expect("repository");
    git(&repository, &["init", "-q"]);
    fs::write(
        repository.join("Cargo.toml"),
        "[package]\nname = \"g7r1sup1\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"lib.rs\"\n",
    )
    .expect("Cargo.toml");
    fs::write(
        repository.join("lib.rs"),
        "pub fn callee() -> u64 { 1 }\npub fn caller() -> u64 { callee() }\n",
    )
    .expect("base");
    git(&repository, &["add", "-A"]);
    git(&repository, &["commit", "-q", "-m", "base"]);
    let base = git(&repository, &["rev-parse", "HEAD"]);
    fs::write(
        repository.join("lib.rs"),
        "pub fn callee() -> u64 { 2 }\npub fn caller() -> u64 { callee() }\n",
    )
    .expect("target");
    git(&repository, &["add", "-A"]);
    git(&repository, &["commit", "-q", "-m", "target"]);
    let target = git(&repository, &["rev-parse", "HEAD"]);
    Fixture {
        temporary,
        repository,
        base,
        target,
    }
}

fn request(version: u8, fixture: &Fixture) -> Vec<u8> {
    let value = if version == 4 {
        let mut request: Value = serde_json::from_slice(include_bytes!(
            "../../../schemas/reviewgraphen.generic_review_request.v4.example.json"
        ))
        .expect("request v4 example");
        request["workspace_admission_root"] = json!(".");
        request["repository_admission_root"] = json!(".");
        request["repository_identity"] = json!("g7-r1/sup1@1");
        request["base_revision"] = json!(fixture.base);
        request["target_revision"] = json!(fixture.target);
        request["ingest"]["max_files"] = json!(64);
        request["ingest"]["max_file_bytes"] = json!(1_048_576);
        request["ingest"]["max_total_source_bytes"] = json!(1_048_576);
        request["verifier_descriptor_id"] = Value::Null;
        request
    } else {
        let mut request = json!({
            "schema": "reviewgraphen.generic_review_request.v2",
            "workspace_admission_root": ".",
            "repository_admission_root": ".",
            "repository_identity": "g7-r1/sup1@1",
            "base_revision": fixture.base,
            "target_revision": fixture.target,
            "ingest": {"profile_id": "rust.production.v1", "profile_version": "1",
                "rule_set_hash": RULE_SET_HASH, "max_files": 32,
                "max_file_bytes": 1048576, "max_total_source_bytes": 1048576},
            "plan": {"max_waves": 8, "max_obligations_per_wave": 32},
            "observer": {"kind": "deterministic_abstain"},
            "verifier_descriptor_id": null
        });
        if version == 3 {
            request["schema"] = json!("reviewgraphen.generic_review_request.v3");
            request["context_policy_id"] = json!("context.subject_windows@3");
        }
        request
    };
    reviewgraphen_core::canonical_json(&value).expect("canonical request")
}

/// In-process `run` (so the thread-local seam applies) from the repository
/// root; the request lives outside the repository.
fn review(fixture: &Fixture, version: u8, artifacts: &str) -> crate::CommandOutcome {
    let request_path = fixture
        .temporary
        .path()
        .join(format!("request.v{version}.{artifacts}.json"));
    fs::write(&request_path, request(version, fixture)).expect("request");
    let previous = env::current_dir().expect("cwd");
    env::set_current_dir(&fixture.repository).expect("enter repository");
    let outcome = run(vec![
        "review".to_owned(),
        "--request".to_owned(),
        request_path.display().to_string(),
        "--artifacts".to_owned(),
        artifacts.to_owned(),
    ]);
    env::set_current_dir(previous).expect("restore cwd");
    outcome
}

fn names(root: &Path) -> BTreeSet<String> {
    fs::read_dir(root)
        .expect("read root")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

// ------------------------------------------------------------ Q5–Q7

fn report_stage_failure_leaves_no_root(version: u8) {
    let _cwd = cwd_lock();
    let fixture = fixture();
    let control = review(&fixture, version, ".g7-r1-sup1-control");
    assert_eq!(
        control.exit_code, 0,
        "v{version} control: {}",
        control.stderr
    );
    {
        let _guard = install_report_stage_fault(FAULT);
        let outcome = review(&fixture, version, OUT);
        assert_eq!(outcome.exit_code, 20, "v{version}: injected report failure");
        assert_eq!(
            outcome.stderr, FAULT,
            "v{version}: the seam fired (escape check)"
        );
        assert!(outcome.stdout.is_empty(), "v{version}: no audit on stdout");
        assert!(
            fs::symlink_metadata(fixture.repository.join(OUT)).is_err(),
            "v{version}: artifact root must not exist after a report-stage failure \
             (it would if admission preceded the report)"
        );
    }
    let after = review(&fixture, version, ".g7-r1-sup1-after");
    assert_eq!(
        after.exit_code, 0,
        "v{version}: guard reset restores identity: {}",
        after.stderr
    );
}

#[test]
fn q5_v4_report_stage_failure_leaves_no_artifact_root() {
    report_stage_failure_leaves_no_root(4);
}

#[test]
fn q6_v2_report_stage_failure_leaves_no_artifact_root() {
    report_stage_failure_leaves_no_root(2);
}

#[test]
fn q7_v3_report_stage_failure_leaves_no_artifact_root() {
    report_stage_failure_leaves_no_root(3);
}

// ------------------------------------------------------------------ Q10

fn foreign_artifact_is_not_overwritten(version: u8) {
    let _cwd = cwd_lock();
    let fixture = fixture();
    let control = review(&fixture, version, ".g7-r1-sup1-control");
    assert_eq!(
        control.exit_code, 0,
        "v{version} control: {}",
        control.stderr
    );
    let control_names = names(&fixture.repository.join(".g7-r1-sup1-control"));
    assert!(control_names.contains("artifact-manifest.v1.json"));
    assert!(
        control_names.len() >= 4,
        "several entries are created before the manifest"
    );
    let calls = std::rc::Rc::new(std::cell::Cell::new(0));
    {
        let counter = std::rc::Rc::clone(&calls);
        // The manifest is written last, so every other entry is created by
        // this call first and must be unwound.
        let _guard = install_post_admission_hook(Box::new(move |root: &Path| {
            counter.set(counter.get() + 1);
            assert!(root.is_dir(), "hook runs after admission created the root");
            fs::write(root.join("artifact-manifest.v1.json"), FOREIGN).expect("foreign file");
        }));
        let outcome = review(&fixture, version, OUT);
        assert_eq!(
            calls.get(),
            1,
            "v{version}: hook ran once, between admission and write"
        );
        assert_eq!(
            outcome.exit_code, 20,
            "v{version}: write must refuse an existing entry"
        );
        assert_eq!(
            outcome.stderr, "unable to write generic review artifact manifest",
            "v{version}"
        );
        let root = fixture.repository.join(OUT);
        assert_eq!(
            fs::read(root.join("artifact-manifest.v1.json")).expect("foreign file kept"),
            FOREIGN,
            "v{version}: foreign bytes untouched (no overwrite)"
        );
        assert_eq!(
            names(&root),
            BTreeSet::from(["artifact-manifest.v1.json".to_owned()]),
            "v{version}: every entry this call created is unwound; only the foreign file remains"
        );
    }
}

#[test]
fn q10_v4_foreign_entry_is_not_overwritten_and_own_entries_unwind() {
    foreign_artifact_is_not_overwritten(4);
}

#[test]
fn q10_v2_foreign_entry_is_not_overwritten_and_own_entries_unwind() {
    foreign_artifact_is_not_overwritten(2);
}
