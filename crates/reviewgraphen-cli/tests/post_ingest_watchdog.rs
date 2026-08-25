use serde_json::json;
use std::{collections::BTreeMap, fs, path::Path, process::Command};
use tempfile::tempdir;

type DiagnosticMutation = (&'static str, fn(&mut serde_json::Value));

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "Watchdog Test")
        .env("GIT_AUTHOR_EMAIL", "watchdog@example.invalid")
        .env("GIT_COMMITTER_NAME", "Watchdog Test")
        .env("GIT_COMMITTER_EMAIL", "watchdog@example.invalid")
        .output()
        .expect("git is available");
    assert!(output.status.success(), "git failed: {arguments:?}");
    String::from_utf8(output.stdout)
        .expect("git stdout UTF-8")
        .trim()
        .to_owned()
}

fn repository() -> (tempfile::TempDir, std::path::PathBuf, String, String) {
    let temporary = tempdir().expect("temporary repository");
    let repository = temporary.path().join("repository");
    fs::create_dir(&repository).expect("repository directory");
    git(&repository, &["init", "-q"]);
    fs::write(repository.join("lib.rs"), "pub fn value() -> u64 { 1 }\n").expect("base");
    git(&repository, &["add", "lib.rs"]);
    git(&repository, &["commit", "-q", "-m", "base"]);
    let base = git(&repository, &["rev-parse", "HEAD"]);
    fs::write(repository.join("lib.rs"), "pub fn value() -> u64 { 2 }\n").expect("target");
    git(&repository, &["add", "lib.rs"]);
    git(&repository, &["commit", "-q", "-m", "target"]);
    let target = git(&repository, &["rev-parse", "HEAD"]);
    (temporary, repository, base, target)
}

fn request(schema: &str, base: &str, target: &str) -> Vec<u8> {
    let mut value = json!({
        "schema": schema,
        "workspace_admission_root": ".",
        "repository_admission_root": ".",
        "repository_identity": "watchdog-product-fixture@1",
        "base_revision": base,
        "target_revision": target,
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
    });
    if schema.ends_with("v3") {
        value["context_policy_id"] = json!("context.subject_windows@3");
    }
    reviewgraphen_core::canonical_json(&value).expect("canonical request")
}

fn tree_bytes(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn visit(root: &Path, directory: &Path, rows: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(directory).expect("artifact directory") {
            let path = entry.expect("artifact entry").path();
            if path.is_dir() {
                visit(root, &path, rows);
            } else {
                rows.insert(
                    path.strip_prefix(root)
                        .expect("relative artifact")
                        .to_string_lossy()
                        .into_owned(),
                    fs::read(path).expect("artifact bytes"),
                );
            }
        }
    }
    let mut rows = BTreeMap::new();
    visit(root, root, &mut rows);
    rows
}

#[test]
fn diagnostics_are_external_and_do_not_change_canonical_bytes() {
    let (_temporary, repository, base, target) = repository();
    fs::write(
        repository.join("request.v3.json"),
        request("reviewgraphen.generic_review_request.v3", &base, &target),
    )
    .expect("request");
    let plain = Command::new(env!("CARGO_BIN_EXE_reviewgraphen"))
        .args([
            "review",
            "--request",
            "request.v3.json",
            "--artifacts",
            "plain",
        ])
        .current_dir(&repository)
        .output()
        .expect("plain product run");
    assert_eq!(plain.status.code(), Some(0));
    let diagnosed = Command::new(env!("CARGO_BIN_EXE_reviewgraphen"))
        .args([
            "review",
            "--request",
            "request.v3.json",
            "--artifacts",
            "diagnosed",
            "--diagnostics",
            "diagnostics.json",
        ])
        .current_dir(&repository)
        .output()
        .expect("diagnosed product run");
    assert_eq!(diagnosed.status.code(), Some(0));
    assert_eq!(diagnosed.stdout, plain.stdout);
    assert_eq!(
        tree_bytes(&repository.join("plain")),
        tree_bytes(&repository.join("diagnosed"))
    );
    let diagnostic: serde_json::Value = serde_json::from_slice(
        &fs::read(repository.join("diagnostics.json")).expect("diagnostic bytes"),
    )
    .expect("diagnostic JSON");
    assert_eq!(
        diagnostic["schema"],
        "reviewgraphen.generic_review_diagnostics.v1"
    );
    let stages = diagnostic["stages"].as_array().expect("stage rows");
    assert_eq!(stages.len(), 6);
    assert!(
        stages
            .iter()
            .all(|row| row["status"].as_str() == Some("completed"))
    );
    assert_eq!(
        stages
            .iter()
            .map(|row| row["stage"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "ingest",
            "synthesize",
            "context",
            "observer",
            "report",
            "artifact_write"
        ]
    );
    let schema_check = Command::new(env!("CARGO_BIN_EXE_reviewgraphen"))
        .args(["schema", "validate", "diagnostics.json"])
        .current_dir(&repository)
        .output()
        .expect("validate diagnostic schema");
    assert_eq!(schema_check.status.code(), Some(0));

    let mutations: [DiagnosticMutation; 3] = [
        (
            "diagnostics-unknown.json",
            |value: &mut serde_json::Value| value["unknown"] = json!(true),
        ),
        (
            "diagnostics-reordered.json",
            |value: &mut serde_json::Value| {
                value["stages"].as_array_mut().unwrap().swap(0, 1);
            },
        ),
        (
            "diagnostics-missing.json",
            |value: &mut serde_json::Value| {
                value["stages"].as_array_mut().unwrap().pop();
            },
        ),
    ];
    for (name, mutate) in mutations {
        let mut mutated = diagnostic.clone();
        mutate(&mut mutated);
        fs::write(
            repository.join(name),
            reviewgraphen_core::canonical_json(&mutated).expect("canonical mutation"),
        )
        .expect("diagnostic mutation");
        let rejected = Command::new(env!("CARGO_BIN_EXE_reviewgraphen"))
            .args(["schema", "validate", name])
            .current_dir(&repository)
            .output()
            .expect("reject diagnostic mutation");
        assert_eq!(rejected.status.code(), Some(3), "{name}");
    }
}

#[test]
fn diagnostics_reject_overlap_and_existing_paths_before_outputs() {
    let (_temporary, repository, base, target) = repository();
    fs::write(
        repository.join("request.v3.json"),
        request("reviewgraphen.generic_review_request.v3", &base, &target),
    )
    .expect("request");
    fs::write(repository.join("existing.json"), b"keep").expect("existing diagnostic");
    fs::create_dir(repository.join("ancestor")).expect("ancestor directory");
    for (artifact, diagnostic) in [
        ("inside-root", "inside-root/diagnostic.json"),
        ("ancestor/artifacts", "ancestor"),
        ("existing-output", "existing.json"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_reviewgraphen"))
            .args([
                "review",
                "--request",
                "request.v3.json",
                "--artifacts",
                artifact,
                "--diagnostics",
                diagnostic,
            ])
            .current_dir(&repository)
            .output()
            .expect("rejected product run");
        assert_eq!(output.status.code(), Some(2), "{artifact} {diagnostic}");
        assert!(!repository.join(artifact).exists());
    }
    assert_eq!(fs::read(repository.join("existing.json")).unwrap(), b"keep");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("existing.json", repository.join("diagnostic-link.json"))
            .expect("diagnostic symlink");
        let output = Command::new(env!("CARGO_BIN_EXE_reviewgraphen"))
            .args([
                "review",
                "--request",
                "request.v3.json",
                "--artifacts",
                "symlink-output",
                "--diagnostics",
                "diagnostic-link.json",
            ])
            .current_dir(&repository)
            .output()
            .expect("rejected symlink run");
        assert_eq!(output.status.code(), Some(2));
        assert!(!repository.join("symlink-output").exists());
    }
}
