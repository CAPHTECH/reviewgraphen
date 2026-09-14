//! Acceptance tests for ADR 0049's accepted Rust responsibility-signal fact.

use reviewgraphen_ingest::{IngestRequest, ingest_v2};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const IDENTITY: &str = "reviewgraphen.test/responsibility-signals";
const FIXTURE_GIT_DATE: &str = "2000-01-01T00:00:00Z";
const SIGNAL_EXTRACTOR: &str = "reviewgraphen.ingest.rust-responsibility-signals@1";

struct Repository {
    _workspace: TempDir,
    root: PathBuf,
    base: String,
    target: String,
}

impl Repository {
    fn request(&self) -> IngestRequest {
        IngestRequest::new(
            self._workspace.path(),
            &self.root,
            IDENTITY,
            &self.base,
            &self.target,
        )
    }
}

fn git<const N: usize>(root: &Path, args: [&str; N]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .env("GIT_AUTHOR_DATE", FIXTURE_GIT_DATE)
        .env("GIT_COMMITTER_DATE", FIXTURE_GIT_DATE)
        .args(args)
        .output()
        .expect("git launches");
    assert!(
        output.status.success(),
        "git succeeds: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git output is UTF-8")
        .trim()
        .to_owned()
}

fn repository_with_source(source: &str) -> Repository {
    let workspace = tempfile::tempdir().expect("temporary workspace");
    let root = workspace.path().join("fixture");
    fs::create_dir(&root).expect("repository directory");
    git(&root, ["init", "--quiet"]);
    git(
        &root,
        ["config", "user.email", "reviewgraphen@example.test"],
    );
    git(&root, ["config", "user.name", "ReviewGraphen test"]);
    fs::create_dir(root.join("src")).expect("source directory");
    fs::write(root.join("src/lib.rs"), "pub fn before() {}\n").expect("base source");
    git(&root, ["add", "."]);
    git(&root, ["commit", "--quiet", "-m", "base"]);
    let base = git(&root, ["rev-parse", "HEAD"]);
    fs::write(root.join("src/lib.rs"), source).expect("target source");
    git(&root, ["add", "."]);
    git(&root, ["commit", "--quiet", "-m", "target"]);
    let target = git(&root, ["rev-parse", "HEAD"]);
    Repository {
        _workspace: workspace,
        root,
        base,
        target,
    }
}

fn accepted_signals(source: &str) -> BTreeMap<String, (String, Value)> {
    let repository = repository_with_source(source);
    let result = ingest_v2(&repository.request()).expect("v2 ingestion succeeds");
    result
        .legacy
        .program_space
        .artifacts()
        .iter()
        .filter(|artifact| matches!(artifact.kind.as_str(), "function" | "method"))
        .map(|artifact| {
            let extractor = artifact
                .attributes
                .get("responsibility_signals_extractor")
                .and_then(Value::as_str)
                .expect("accepted responsibility-signal extractor")
                .to_owned();
            let signals = artifact
                .attributes
                .get("responsibility_signals")
                .expect("accepted responsibility signals")
                .clone();
            (artifact.label.clone(), (extractor, signals))
        })
        .collect()
}

fn accepted_test_scopes(source: &str) -> BTreeMap<String, String> {
    let repository = repository_with_source(source);
    let result = ingest_v2(&repository.request()).expect("v2 ingestion succeeds");
    result
        .legacy
        .program_space
        .artifacts()
        .iter()
        .filter(|artifact| matches!(artifact.kind.as_str(), "function" | "method"))
        .map(|artifact| {
            let scope = artifact
                .attributes
                .get("test_scope")
                .and_then(Value::as_str)
                .expect("accepted Rust test-scope fact")
                .to_owned();
            let extractor = artifact
                .attributes
                .get("test_scope_extractor")
                .and_then(Value::as_str);
            assert_eq!(
                extractor,
                Some("reviewgraphen.ingest.rust-test-scope@1"),
                "test scope must come from the accepted extractor"
            );
            (artifact.label.clone(), scope)
        })
        .collect()
}

#[test]
fn accepted_signal_fact_is_sorted_unique_and_bound_to_the_exact_extractor() {
    let signals = accepted_signals(
        r#"
pub struct Input;
pub struct Output;
pub struct Failure;
pub struct Pipeline;

pub fn load_record(input: Input) -> Result<Output, Failure> {
    let parsed = parse_item(input, "first", 7);
    let duplicate = parse_item(parsed, "second", 9);
    duplicate.normalize()
}

impl Pipeline {
    pub fn save_record(&self, input: Input) -> Result<Output, Failure> {
        persist_item(input).normalize()
    }
}
"#,
    );

    let (extractor, function) = signals
        .iter()
        .find_map(|(name, value)| name.ends_with("::load_record").then_some(value))
        .expect("function signal fact");
    assert_eq!(extractor, SIGNAL_EXTRACTOR);
    assert_eq!(
        function,
        &json!({
            "callable": ["load", "record"],
            "signature": ["Failure", "Input", "Output", "Result"],
            "operation": ["normalize", "parse_item"]
        })
    );

    let (extractor, method) = signals
        .iter()
        .find_map(|(name, value)| name.ends_with("::save_record::Pipeline").then_some(value))
        .expect("method signal fact");
    assert_eq!(extractor, SIGNAL_EXTRACTOR);
    assert_eq!(
        method,
        &json!({
            "callable": ["record", "save"],
            "signature": ["Failure", "Input", "Output", "Result"],
            "operation": ["normalize", "persist_item"]
        })
    );
}

#[test]
fn comments_formatting_and_literal_values_do_not_change_signal_facts() {
    let compact = accepted_signals(
        "pub struct Input; pub struct Output; pub struct Failure; \
         pub fn load_record(input: Input) -> Result<Output, Failure> { \
         parse_item(input, \"one\", 1).normalize() }\n",
    );
    let reformatted = accepted_signals(
        r#"
pub struct Input;
pub struct Output;
pub struct Failure;

// This comment and all literal values are deliberately different.
pub fn load_record(
    input: Input,
) -> Result<Output, Failure> {
    parse_item(
        input,
        "a completely different literal",
        999_999,
    )
    .normalize()
}
"#,
    );

    let select = |facts: &BTreeMap<String, (String, Value)>| {
        facts
            .iter()
            .find_map(|(name, value)| name.ends_with("::load_record").then_some(value.clone()))
            .expect("load_record signal fact")
    };
    assert_eq!(select(&compact), select(&reformatted));
}

#[test]
fn cfg_test_scope_requires_test_in_every_enabled_configuration() {
    let scopes = accepted_test_scopes(
        r#"
#[cfg(test)]
pub fn cfg_test() {}

#[cfg(all(test, feature = "x"))]
pub fn cfg_all_test_and_feature() {}

#[cfg(any(test))]
pub fn cfg_any_test_only() {}

#[cfg(all(any(test), feature = "x"))]
pub fn cfg_all_any_test_and_feature() {}

#[cfg(not(test))]
pub fn cfg_not_test() {}

#[cfg(feature = "contest")]
pub fn cfg_contest_feature() {}

#[cfg(any(test, unix))]
pub fn cfg_test_or_unix() {}
"#,
    );

    let scope = |suffix: &str| {
        scopes
            .iter()
            .find_map(|(name, scope)| name.ends_with(suffix).then_some(scope.as_str()))
            .unwrap_or_else(|| panic!("scope fact for {suffix}: {scopes:?}"))
    };

    assert_eq!(
        [
            ("cfg_test", scope("::cfg_test")),
            (
                "cfg_all_test_and_feature",
                scope("::cfg_all_test_and_feature"),
            ),
            ("cfg_any_test_only", scope("::cfg_any_test_only")),
            (
                "cfg_all_any_test_and_feature",
                scope("::cfg_all_any_test_and_feature"),
            ),
            ("cfg_not_test", scope("::cfg_not_test")),
            ("cfg_contest_feature", scope("::cfg_contest_feature")),
            ("cfg_test_or_unix", scope("::cfg_test_or_unix")),
        ],
        [
            ("cfg_test", "test"),
            ("cfg_all_test_and_feature", "test"),
            ("cfg_any_test_only", "test"),
            ("cfg_all_any_test_and_feature", "test"),
            ("cfg_not_test", "production"),
            ("cfg_contest_feature", "production"),
            ("cfg_test_or_unix", "production"),
        ]
    );
}
