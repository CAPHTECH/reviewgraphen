//! O9-R SUPPLEMENT-1 (Acc-O9R): a repo-local `diff.algorithm` must not change
//! Rust v4 review output.
//!
//! A mutation run found that dropping
//! `--diff-algorithm=myers` from the `ChangedLines` argv survived: the fixtures
//! in `o9r_git_config_hygiene_acceptance.rs` produce identical `-U0` hunks
//! under every algorithm. This fixture was found by a search and chosen
//! because patience splits the change differently (the hunks were recorded
//! with `git diff --no-index -U0 --inter-hunk-context=0 --indent-heuristic`):
//!
//! * myers / histogram / minimal: `-7,5 +7` and `-22 +18,3`. The changed
//!   target lines fall in `beta` (7) and `delta` (18-20).
//! * patience: `-6,0 +7,5`, `-8 +13,5` and `-15,11 +23,0`. Target lines 13-17
//!   cover the untouched `gamma` (11-15), which `uses_gamma` calls.
//!
//! So with `diff.algorithm=patience` unpinned, `gamma` becomes changed and a
//! spurious `relation.changed_public_callee@1` appears. GREEN on 57578d65 and
//! RED once `--diff-algorithm=myers` is removed from the argv. The
//! `histogram`/`minimal` variants are kept as guards but do NOT discriminate
//! on this fixture (their hunks equal myers). No distinguishing fixture for
//! them was found in a bounded search.

use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};
use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_reviewgraphen");
const ARTIFACTS: &str = ".o9r-sup1-artifacts";

const CARGO_TOML: &str = "[package]\nname = \"o9rsup1\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n";

const LIB_BASE: &str = "pub fn alpha(x: u32) -> u32 {
    drop(x);
    x
}

pub fn beta(x: u32) -> u32 {
    let z = x * 2;
    let y = x + 1;
    if x > 2 {
        return x;
    }
    x
}

pub fn gamma(x: u32) -> u32 {
    let z = x * 2;
    drop(x);
    x
}

pub fn delta(x: u32) -> u32 {
    let y = x + 1;
    x
}

pub fn eps(x: u32) -> u32 {
    drop(x);
    x
}

pub fn uses_gamma(x: u32) -> u32 {
    gamma(x)
}
";

const LIB_TARGET: &str = "pub fn alpha(x: u32) -> u32 {
    drop(x);
    x
}

pub fn beta(x: u32) -> u32 {
    drop(x);
    x
}

pub fn gamma(x: u32) -> u32 {
    let z = x * 2;
    drop(x);
    x
}

pub fn delta(x: u32) -> u32 {
    if x > 2 {
        return x;
    }
    x
}

pub fn eps(x: u32) -> u32 {
    drop(x);
    x
}

pub fn uses_gamma(x: u32) -> u32 {
    gamma(x)
}
";

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Acc-O9R")
        .env("GIT_AUTHOR_EMAIL", "acc-o9r@example.invalid")
        .env("GIT_COMMITTER_NAME", "Acc-O9R")
        .env("GIT_COMMITTER_EMAIL", "acc-o9r@example.invalid")
        .env("GIT_AUTHOR_DATE", "1700000000 +0000")
        .env("GIT_COMMITTER_DATE", "1700000000 +0000")
        .output()
        .expect("git is available");
    assert!(
        output.status.success(),
        "fixture git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git stdout UTF-8")
        .trim()
        .to_owned()
}

struct Fixture {
    _dir: TempDir,
    root: PathBuf,
    base: String,
    target: String,
}

fn fixture() -> Fixture {
    let dir = TempDir::new().expect("fixture dir");
    let root = dir.path().join("repository");
    fs::create_dir_all(root.join("src")).expect("src dir");
    git(
        &root,
        &["init", "--quiet", "--object-format=sha1", "--template="],
    );
    git(&root, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    fs::write(root.join("Cargo.toml"), CARGO_TOML).expect("Cargo.toml");
    fs::write(root.join("src/lib.rs"), LIB_BASE).expect("base lib.rs");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "--quiet", "--no-verify", "-m", "base"]);
    let base = git(&root, &["rev-parse", "HEAD"]);
    fs::write(root.join("src/lib.rs"), LIB_TARGET).expect("target lib.rs");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "--quiet", "--no-verify", "-m", "target"]);
    let target = git(&root, &["rev-parse", "HEAD"]);
    Fixture {
        _dir: dir,
        root,
        base,
        target,
    }
}

fn request(fixture: &Fixture) -> Vec<u8> {
    let mut request: Value = serde_json::from_slice(include_bytes!(
        "../../../schemas/reviewgraphen.generic_review_request.v4.example.json"
    ))
    .expect("request v4 example");
    request["workspace_admission_root"] = json!(".");
    request["repository_admission_root"] = json!(".");
    request["repository_identity"] = json!("o9r-sup1/rust@1");
    request["base_revision"] = json!(fixture.base);
    request["target_revision"] = json!(fixture.target);
    request["ingest"]["max_files"] = json!(64);
    request["ingest"]["max_file_bytes"] = json!(1_048_576);
    request["ingest"]["max_total_source_bytes"] = json!(1_048_576);
    request["verifier_descriptor_id"] = Value::Null;
    reviewgraphen_core::canonical_json(&request).expect("canonical request")
}

#[derive(Debug, PartialEq, Eq)]
struct RunResult {
    exit: Option<i32>,
    stdout: Vec<u8>,
    artifacts: Option<BTreeMap<String, Vec<u8>>>,
}

/// Reviews a private clone carrying `config` in its own `.git/config`.
fn run(config: &[(&str, &str)]) -> (RunResult, String) {
    let fixture = fixture();
    let dir = TempDir::new().expect("run dir");
    git(
        dir.path(),
        &[
            "clone",
            "--quiet",
            "--no-hardlinks",
            fixture.root.to_str().expect("UTF-8 path"),
            "clone",
        ],
    );
    let clone = dir.path().join("clone");
    for (key, value) in config {
        git(&clone, &["config", "--local", key, value]);
    }
    let request_path = dir.path().join("request.json");
    fs::write(&request_path, request(&fixture)).expect("request");
    let mut command = Command::new(BIN);
    command
        .args(["review", "--request"])
        .arg(&request_path)
        .args(["--artifacts", ARTIFACTS])
        .current_dir(&clone)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default());
    if let Some(home) = std::env::var_os("HOME") {
        command.env("HOME", home);
    }
    let output = command.output().expect("run reviewgraphen");
    let root = clone.join(ARTIFACTS);
    let artifacts = root.is_dir().then(|| {
        fs::read_dir(&root)
            .expect("artifact root")
            .map(|entry| {
                let entry = entry.expect("artifact entry");
                (
                    entry.file_name().into_string().expect("UTF-8 name"),
                    fs::read(entry.path()).expect("artifact bytes"),
                )
            })
            .collect()
    });
    (
        RunResult {
            exit: output.status.code(),
            stdout: output.stdout,
            artifacts,
        },
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn baseline() -> &'static RunResult {
    static BASELINE: OnceLock<RunResult> = OnceLock::new();
    BASELINE.get_or_init(|| {
        let (result, stderr) = run(&[]);
        assert_eq!(result.exit, Some(0), "baseline must succeed: {stderr}");
        result
    })
}

fn callee_mentions(result: &RunResult) -> usize {
    result
        .artifacts
        .as_ref()
        .and_then(|a| a.get("audit.run.v4.json"))
        .map(|bytes| {
            String::from_utf8_lossy(bytes)
                .matches("\"relation.changed_public_callee@1\"")
                .count()
        })
        .unwrap_or(0)
}

fn check(algorithm: &str) {
    let expected = baseline();
    let (actual, stderr) = run(&[("diff.algorithm", algorithm)]);
    assert!(
        &actual == expected,
        "repo-local diff.algorithm={algorithm} changed review output \
         (exit {:?} -> {:?}; stdout identical: {}; artifacts identical: {}; \
         changed_public_callee mentions {} -> {}); stderr: {}",
        expected.exit,
        actual.exit,
        actual.stdout == expected.stdout,
        actual.artifacts == expected.artifacts,
        callee_mentions(expected),
        callee_mentions(&actual),
        stderr.lines().last().unwrap_or("")
    );
}

/// The fixture really discriminates at the plain-git level, independently
/// of the product: patience moves a changed range onto `gamma`.
#[test]
fn o9r_sup1_fixture_hunks_differ_between_myers_and_patience() {
    let fixture = fixture();
    let hunks = |algorithm: &str| {
        git(
            &fixture.root,
            &[
                "-c",
                "core.attributesFile=/dev/null",
                "diff",
                "--no-color",
                "--no-ext-diff",
                "--inter-hunk-context=0",
                "--indent-heuristic",
                &format!("--diff-algorithm={algorithm}"),
                "--unified=0",
                &fixture.base,
                &fixture.target,
            ],
        )
        .lines()
        .filter(|line| line.starts_with("@@ "))
        .map(|line| line.split(" @@").next().unwrap_or(line).to_owned())
        .collect::<Vec<_>>()
    };
    assert_eq!(hunks("myers"), ["@@ -7,5 +7", "@@ -22 +18,3"]);
    assert_eq!(
        hunks("patience"),
        ["@@ -6,0 +7,5", "@@ -8 +13,5", "@@ -15,11 +23,0"]
    );
    // Recorded, not discriminating on this fixture.
    assert_eq!(hunks("histogram"), hunks("myers"));
    assert_eq!(hunks("minimal"), hunks("myers"));
    // Target lines 11-15 are `gamma`, which the target change leaves alone.
    let gamma: Vec<&str> = LIB_TARGET.lines().skip(10).take(5).collect();
    assert_eq!(gamma[0], "pub fn gamma(x: u32) -> u32 {");
    assert_eq!(gamma[4], "}");
}

#[test]
fn o9r_sup1_rust_diff_algorithm_patience() {
    check("patience");
}

#[test]
fn o9r_sup1_rust_diff_algorithm_histogram() {
    check("histogram");
}

#[test]
fn o9r_sup1_rust_diff_algorithm_minimal() {
    check("minimal");
}

#[test]
fn o9r_sup1_rust_diff_algorithm_myers_explicit() {
    check("myers");
}
