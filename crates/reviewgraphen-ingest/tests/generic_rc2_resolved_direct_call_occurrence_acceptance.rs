//! Acceptance seam for one source-observed, syntactically unique Rust call.

use reviewgraphen_core::ContentHash;
use reviewgraphen_ingest::{IngestRequest, ingest_with_sources};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const IDENTITY: &str = "reviewgraphen.test/generic-rc2-resolved-direct-call-occurrence";
const FIXTURE_GIT_DATE: &str = "2000-01-01T00:00:00Z";
const CANONICAL_PATH: &str = "src/direct.rs";
const SOURCE: &str = "fn resolved() {}\npub fn caller() {\n    resolved();\n}\n";
const CALL_START_LINE: u64 = 3;
const CALL_END_LINE: u64 = 3;
const CALL_START_COLUMN: u64 = 5;
const CALL_END_COLUMN: u64 = 14;

struct Repository {
    _workspace: TempDir,
    workspace_root: PathBuf,
    root: PathBuf,
    base: String,
    target: String,
}

impl Repository {
    fn request(&self) -> IngestRequest {
        IngestRequest::new(
            &self.workspace_root,
            &self.root,
            IDENTITY,
            &self.base,
            &self.target,
        )
    }
}

fn command<const N: usize>(root: &Path, arguments: [&str; N]) {
    let status = Command::new("git")
        .current_dir(root)
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join(".xdg-config"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_AUTHOR_DATE", FIXTURE_GIT_DATE)
        .env("GIT_COMMITTER_DATE", FIXTURE_GIT_DATE)
        .args(arguments)
        .status()
        .expect("git launches");
    assert!(status.success(), "git command succeeds");
}

fn output<const N: usize>(root: &Path, arguments: [&str; N]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join(".xdg-config"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_AUTHOR_DATE", FIXTURE_GIT_DATE)
        .env("GIT_COMMITTER_DATE", FIXTURE_GIT_DATE)
        .args(arguments)
        .output()
        .expect("git launches");
    assert!(output.status.success(), "git command succeeds");
    String::from_utf8(output.stdout)
        .expect("git output is UTF-8")
        .trim()
        .to_owned()
}

fn write(root: &Path, path: &str, bytes: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().expect("path has a parent")).expect("parent exists");
    fs::write(path, bytes).expect("fixture source writes");
}

fn repository() -> Repository {
    let workspace = tempfile::tempdir().expect("temporary workspace");
    let workspace_root = workspace.path().to_owned();
    let root = workspace_root.join("fixture");
    fs::create_dir(&root).expect("repository directory");
    command(&root, ["init", "--quiet", "--object-format=sha1"]);
    command(
        &root,
        ["config", "user.email", "reviewgraphen@example.test"],
    );
    command(&root, ["config", "user.name", "ReviewGraphen test"]);
    write(&root, CANONICAL_PATH, "fn baseline() {}\n");
    command(&root, ["add", "."]);
    command(&root, ["commit", "--quiet", "-m", "base"]);
    let base = output(&root, ["rev-parse", "HEAD"]);
    write(&root, CANONICAL_PATH, SOURCE);
    command(&root, ["add", "."]);
    command(&root, ["commit", "--quiet", "-m", "resolved direct call"]);
    let target = output(&root, ["rev-parse", "HEAD"]);
    Repository {
        _workspace: workspace,
        workspace_root,
        root,
        base,
        target,
    }
}

#[test]
fn accepted_syntactic_unique_direct_call_retains_its_source_occurrence() {
    let repository = repository();
    let result = ingest_with_sources(&repository.request(), u64::MAX).expect("ingestion succeeds");

    let source_entries = result.source_bundle.entries();
    assert_eq!(source_entries.len(), 1, "fixture admits one source file");
    let source = &source_entries[0];
    assert_eq!(source.path(), CANONICAL_PATH);
    assert_eq!(source.bytes(), SOURCE.as_bytes());
    assert_eq!(
        source.content_hash(),
        &ContentHash::sha256(SOURCE.as_bytes())
    );
    assert_eq!(source.cas_hash(), &ContentHash::sha256(SOURCE.as_bytes()));

    let calls = result
        .program_space
        .relations()
        .iter()
        .filter(|relation| relation.kind == "calls")
        .collect::<Vec<_>>();
    assert_eq!(
        calls.len(),
        1,
        "literal fixture has one accepted direct call"
    );
    let relation = calls[0];
    assert_eq!(
        relation.attributes.get("resolution"),
        Some(&Value::String("syntactic_unique".to_owned()))
    );

    let occurrences = result.resolved_direct_call_occurrences();
    assert_eq!(
        occurrences.len(),
        1,
        "one accepted call has one occurrence record"
    );
    let occurrence = &occurrences[0];
    assert_eq!(occurrence.relation_id(), &relation.id);
    assert_eq!(occurrence.path(), source.path());
    assert_eq!(occurrence.start_line(), CALL_START_LINE);
    assert_eq!(occurrence.end_line(), CALL_END_LINE);
    assert_eq!(occurrence.start_column(), CALL_START_COLUMN);
    assert_eq!(occurrence.end_column(), CALL_END_COLUMN);
}
