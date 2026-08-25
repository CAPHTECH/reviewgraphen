use reviewgraphen_cli::run;
use serde_json::json;
use std::{
    collections::BTreeMap,
    env, fs,
    path::Path,
    process::Command,
    sync::{Mutex, OnceLock},
    time::SystemTime,
};
use tempfile::tempdir;

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "ReviewGraphen Test")
        .env("GIT_AUTHOR_EMAIL", "reviewgraphen@example.invalid")
        .env("GIT_COMMITTER_NAME", "ReviewGraphen Test")
        .env("GIT_COMMITTER_EMAIL", "reviewgraphen@example.invalid")
        .output()
        .expect("git is available");
    assert!(output.status.success(), "git failed: {arguments:?}");
    String::from_utf8(output.stdout)
        .expect("git stdout UTF-8")
        .trim()
        .to_owned()
}

fn request(base_revision: &str, target_revision: &str) -> Vec<u8> {
    reviewgraphen_core::canonical_json(&json!({
        "schema": "reviewgraphen.generic_review_request.v2",
        "workspace_admission_root": ".",
        "repository_admission_root": ".",
        "repository_identity": "reviewgraphen/cli-quickstart-test@1",
        "base_revision": base_revision,
        "target_revision": target_revision,
        "ingest": {
            "profile_id": "rust.production.v1",
            "profile_version": "1",
            "rule_set_hash": "sha256:8f6bfbfb2dbf2f0eaf916b152ddba1e422db8b6de95f931c0c78c9ee4d050b47",
            "max_files": 32,
            "max_file_bytes": 1048576,
            "max_total_source_bytes": 1048576
        },
        "plan": {"max_waves": 8, "max_obligations_per_wave": 32},
        "observer": {"kind": "deterministic_abstain"},
        "verifier_descriptor_id": null
    }))
    .expect("canonical request")
}

fn request_v3(base_revision: &str, target_revision: &str) -> Vec<u8> {
    let mut value: serde_json::Value =
        serde_json::from_slice(&request(base_revision, target_revision)).expect("v2 request JSON");
    let request = value.as_object_mut().expect("request object");
    request.insert(
        "schema".to_owned(),
        serde_json::Value::String("reviewgraphen.generic_review_request.v3".to_owned()),
    );
    request.insert(
        "context_policy_id".to_owned(),
        serde_json::Value::String("context.subject_windows@3".to_owned()),
    );
    reviewgraphen_core::canonical_json(&value).expect("canonical v3 request")
}

fn source(callee_value: u64) -> String {
    let mut source = format!(
        "pub struct Receiver;\nimpl Receiver {{ pub fn unresolved(&self) {{}} }}\n\
         pub fn caller() -> u64 {{ callee() }}\npub fn callee() -> u64 {{ {callee_value} }}\n\
         pub fn unresolved_calls(value: &Receiver) {{\n"
    );
    for _ in 0..20_001 {
        source.push_str("value.unresolved();\n");
    }
    source.push_str("}\n");
    source
}

fn file_state(root: &Path) -> BTreeMap<String, (Vec<u8>, SystemTime)> {
    let mut state = BTreeMap::new();
    for entry in walkdir(root) {
        let relative = entry.strip_prefix(root).expect("relative output");
        let metadata = fs::metadata(&entry).expect("output metadata");
        if metadata.is_file() {
            state.insert(
                relative.display().to_string(),
                (
                    fs::read(&entry).expect("output bytes"),
                    metadata.modified().expect("mtime"),
                ),
            );
        }
    }
    state
}

fn walkdir(root: &Path) -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(root).expect("read output directory") {
        let path = entry.expect("directory entry").path();
        if fs::metadata(&path).expect("entry metadata").is_dir() {
            paths.extend(walkdir(&path));
        } else {
            paths.push(path);
        }
    }
    paths
}

fn cwd_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("cwd lock")
}

#[test]
fn v2_quickstart_creates_pinned_layout_and_refuses_second_run_without_mutation() {
    let _cwd_guard = cwd_lock();
    let temporary = tempdir().expect("temporary repository");
    let repository = temporary.path().join("repository");
    fs::create_dir(&repository).expect("repository directory");
    git(&repository, &["init", "-q"]);
    fs::write(repository.join("lib.rs"), source(1)).expect("base source");
    git(&repository, &["add", "lib.rs"]);
    git(&repository, &["commit", "-q", "-m", "base"]);
    let base = git(&repository, &["rev-parse", "HEAD"]);
    fs::write(repository.join("lib.rs"), source(2)).expect("target source");
    git(&repository, &["add", "lib.rs"]);
    git(&repository, &["commit", "-q", "-m", "target"]);
    let target = git(&repository, &["rev-parse", "HEAD"]);
    fs::write(repository.join("request.v2.json"), request(&base, &target)).expect("request");

    let previous_cwd = env::current_dir().expect("current directory");
    env::set_current_dir(&repository).expect("test invocation root");
    let arguments = vec![
        "review".to_owned(),
        "--request".to_owned(),
        "request.v2.json".to_owned(),
        "--artifacts".to_owned(),
        ".reviewgraphen-quickstart-output".to_owned(),
    ];
    let first = run(arguments.clone());
    env::set_current_dir(&previous_cwd).expect("restore current directory");

    assert_eq!(first.exit_code, 0, "{}", first.stderr);
    let output = repository.join(".reviewgraphen-quickstart-output");
    assert_eq!(
        first.stdout,
        fs::read(output.join("audit.run.v2.json")).expect("audit bytes")
    );
    let audit: serde_json::Value = serde_json::from_slice(&first.stdout).expect("audit JSON");
    assert_eq!(
        audit["ingestion_report_v2"]["observed_occurrence_count"],
        20_001
    );
    assert_eq!(
        audit["ingestion_report_v2"]["source_occurrence_summaries"]
            .as_array()
            .expect("source summaries")
            .len(),
        1
    );
    assert_eq!(
        audit["coverage"]["enumeration_obstruction_summary_ids"]
            .as_array()
            .expect("summary IDs")
            .len(),
        1
    );
    assert!(
        audit["ingestion_report_v2"]
            .get("located_call_occurrences")
            .is_none()
    );
    let d_obligations = audit["obligation_contract"]
        .as_array()
        .expect("obligation contract")
        .iter()
        .filter(|obligation| {
            obligation["rule_id"] == "relation.changed_public_callee@1"
                && obligation["property_id"] == "rust.callee_contract_review@1"
        })
        .collect::<Vec<_>>();
    assert_eq!(d_obligations.len(), 1);
    assert_eq!(d_obligations[0]["applicability_status"], "applicable");
    let mut count_mutation = audit.clone();
    count_mutation["coverage"]["observed_unresolved_call_occurrence_count"] =
        serde_json::json!(20_000);
    let mutated_bytes = reviewgraphen_core::canonical_json(&count_mutation).expect("mutation");
    assert!(reviewgraphen_report::generate_generic_human_report(&mutated_bytes).is_err());
    let state = file_state(&output);
    assert!(state.contains_key("artifact-manifest.v1.json"));
    assert!(state.contains_key("human-report.manifest.v1.json"));
    assert!(state.contains_key("human-report.md"));
    assert!(
        state
            .keys()
            .any(|path| path.contains("provider-free-reviewer-packet.v1.json"))
    );
    assert!(
        state
            .keys()
            .any(|path| path.contains("deterministic-observer-output.v1.json"))
    );

    env::set_current_dir(&repository).expect("test invocation root");
    let second = run(arguments);
    env::set_current_dir(&previous_cwd).expect("restore current directory");
    assert_eq!(second.exit_code, 20);
    assert_eq!(second.stderr, "generic review artifact root already exists");
    assert_eq!(
        file_state(&output),
        state,
        "second run must not mutate outputs"
    );

    env::set_current_dir(&repository).expect("test invocation root");
    let traversal = run(vec![
        "review".to_owned(),
        "--request".to_owned(),
        "request.v2.json".to_owned(),
        "--artifacts".to_owned(),
        "../escaped-output".to_owned(),
    ]);
    env::set_current_dir(&previous_cwd).expect("restore current directory");
    assert_eq!(traversal.exit_code, 20);
    assert_eq!(
        traversal.stderr,
        "generic review artifact root path traversal"
    );
    assert_eq!(file_state(&output), state);

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&output, repository.join("linked-output"))
            .expect("output symlink");
        env::set_current_dir(&repository).expect("test invocation root");
        let symlink = run(vec![
            "review".to_owned(),
            "--request".to_owned(),
            "request.v2.json".to_owned(),
            "--artifacts".to_owned(),
            "linked-output".to_owned(),
        ]);
        env::set_current_dir(&previous_cwd).expect("restore current directory");
        assert_eq!(symlink.exit_code, 20);
        assert_eq!(
            symlink.stderr,
            "generic review artifact root already exists"
        );
        assert_eq!(file_state(&output), state);
    }
}

#[test]
fn v3_dispatches_only_to_v3_and_rejects_cross_version_or_policy_mutations() {
    let _cwd_guard = cwd_lock();
    let temporary = tempdir().expect("temporary repository");
    let repository = temporary.path().join("repository");
    fs::create_dir(&repository).expect("repository directory");
    git(&repository, &["init", "-q"]);
    fs::write(
        repository.join("lib.rs"),
        "pub fn caller() -> u64 { callee() }\npub fn callee() -> u64 { 1 }\n",
    )
    .expect("base source");
    git(&repository, &["add", "lib.rs"]);
    git(&repository, &["commit", "-q", "-m", "base"]);
    let base = git(&repository, &["rev-parse", "HEAD"]);
    fs::write(
        repository.join("lib.rs"),
        "pub fn caller() -> u64 { callee() }\npub fn callee() -> u64 { 2 }\n",
    )
    .expect("target source");
    git(&repository, &["add", "lib.rs"]);
    git(&repository, &["commit", "-q", "-m", "target"]);
    let target = git(&repository, &["rev-parse", "HEAD"]);

    let good_v3 = request_v3(&base, &target);
    fs::write(repository.join("request.v3.json"), &good_v3).expect("v3 request");
    let previous_cwd = env::current_dir().expect("current directory");
    env::set_current_dir(&repository).expect("test invocation root");
    let successful = run(vec![
        "review".to_owned(),
        "--request".to_owned(),
        "request.v3.json".to_owned(),
        "--artifacts".to_owned(),
        "v3-output".to_owned(),
    ]);
    env::set_current_dir(&previous_cwd).expect("restore current directory");
    assert_eq!(successful.exit_code, 0, "{}", successful.stderr);
    let output = repository.join("v3-output");
    assert_eq!(
        successful.stdout,
        fs::read(output.join("audit.run.v3.json")).expect("v3 audit bytes")
    );
    assert!(output.join("human-report.manifest.v2.json").is_file());
    assert!(!output.join("human-report.manifest.v1.json").exists());

    let v2_with_v3_selector = {
        let mut value: serde_json::Value =
            serde_json::from_slice(&request(&base, &target)).expect("v2 request JSON");
        value["context_policy_id"] =
            serde_json::Value::String("context.subject_windows@3".to_owned());
        reviewgraphen_core::canonical_json(&value).expect("cross-version mutation")
    };
    fs::write(repository.join("v2-with-v3.json"), v2_with_v3_selector).expect("mutation");
    for (name, mutation) in [
        ("v3-without-selector.json", None),
        (
            "v3-wrong-selector.json",
            Some(serde_json::Value::String(
                "context.subject_windows@2".to_owned(),
            )),
        ),
        (
            "v3-inline-selector.json",
            Some(json!({"id":"context.subject_windows@3"})),
        ),
        (
            "v3-hash-selector.json",
            Some(serde_json::Value::String(
                "context.subject_windows@3#sha256:00".to_owned(),
            )),
        ),
    ] {
        let mut value: serde_json::Value =
            serde_json::from_slice(&good_v3).expect("v3 request JSON");
        if let Some(selector) = mutation {
            value["context_policy_id"] = selector;
        } else {
            value
                .as_object_mut()
                .expect("request object")
                .remove("context_policy_id");
        }
        fs::write(
            repository.join(name),
            reviewgraphen_core::canonical_json(&value).expect("mutation"),
        )
        .expect("write mutation");
    }

    for request_name in [
        "v2-with-v3.json",
        "v3-without-selector.json",
        "v3-wrong-selector.json",
        "v3-inline-selector.json",
        "v3-hash-selector.json",
    ] {
        env::set_current_dir(&repository).expect("test invocation root");
        let rejected = run(vec![
            "review".to_owned(),
            "--request".to_owned(),
            request_name.to_owned(),
            "--artifacts".to_owned(),
            format!("{request_name}-output"),
        ]);
        env::set_current_dir(&previous_cwd).expect("restore current directory");
        assert_eq!(rejected.exit_code, 3, "{request_name}: {}", rejected.stderr);
        assert!(rejected.stderr.starts_with("invalid generic review v"));
    }
}
