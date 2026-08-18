//! Diagnostic probe for the `_ => {}` gap. **Not a gate.**
//!
//! This file is never part of the frozen question-3 acceptance suite, is
//! never shown to a candidate, and never changes whether a trial counts as
//! passing. It exists so the recurrence of one specific defect can be
//! measured mechanically, per trial, independently of what the blind judge
//! happens to notice.
//!
//! The defect, found by the blind judge in both arms of the completed
//! task-2 run (`RESULTS-002.md` section 4.1): a `_ => {}` catch-all over
//! `syn::ForeignItem` silently drops `ForeignItem::Macro` and
//! `ForeignItem::Verbatim`. A macro inside `extern "C" { ... }` can expand
//! to `fn`/`static` declarations whose names cannot be enumerated here, so
//! the crate's own documented posture — conservatism for unenumerable
//! bindings, as it already applies to a glob `use` and to an opaque pattern
//! — should mark the block conservatively unresolved. Falling through
//! instead leaves a naked call wrongly matched to a module-level function.
//!
//! PASS  => the change is fail-closed for unenumerable foreign items.
//! FAIL  => the gap is present.
//!
//! On the pinned revision this test also fails, because the whole
//! foreign-module arm is absent there. That is expected and is recorded as
//! the pre-change reference point, not as a trial result.

use reviewgraphen_ingest::{IngestRequest, ingest};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const FIXTURE_IDENTITY: &str = "reviewgraphen.test/m8-probe2-fixture";

struct TempGitRepository {
    workspace: TempDir,
    repository: PathBuf,
    identity: String,
    base: String,
    target: String,
}

impl TempGitRepository {
    fn request(&self) -> IngestRequest {
        IngestRequest::new(
            self.workspace.path(),
            &self.repository,
            self.identity.clone(),
            &self.base,
            &self.target,
        )
    }
}

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().expect("parent")).expect("source directory");
    fs::write(path, content).expect("source file");
}

fn git<const N: usize>(root: &Path, args: [&str; N]) {
    let status = Command::new("git")
        .current_dir(root)
        .args(args)
        .status()
        .expect("start test git command");
    assert!(status.success(), "test git command must succeed");
}

fn git_stdout<const N: usize>(root: &Path, args: [&str; N]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .expect("start test git command");
    assert!(output.status.success(), "test git command must succeed");
    String::from_utf8(output.stdout)
        .expect("git output is UTF-8")
        .trim()
        .to_owned()
}

fn fixture_repository() -> TempGitRepository {
    let workspace = tempfile::tempdir().expect("temporary workspace");
    let repository = workspace.path().join("fixture");
    fs::create_dir(&repository).expect("repository directory");
    git(&repository, ["init", "--quiet"]);
    git(
        &repository,
        ["config", "user.email", "reviewgraphen@example.test"],
    );
    git(&repository, ["config", "user.name", "ReviewGraphen test"]);
    write(
        &repository,
        "Cargo.toml",
        "[package]\nname = \"m8-probe\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    write(
        &repository,
        "src/lib.rs",
        "mod api;\n\npub fn entry() {\n    crate::api::worker();\n}\n",
    );
    write(&repository, "src/api.rs", "pub fn worker() {}\n");
    git(&repository, ["add", "."]);
    git(&repository, ["commit", "--quiet", "-m", "base"]);
    let base = git_stdout(&repository, ["rev-parse", "HEAD"]);
    write(
        &repository,
        "src/api.rs",
        "pub fn worker() {\n    let _changed = true;\n}\n",
    );
    git(&repository, ["add", "."]);
    git(&repository, ["commit", "--quiet", "-m", "target"]);
    let target = git_stdout(&repository, ["rev-parse", "HEAD"]);
    TempGitRepository {
        workspace,
        repository,
        identity: FIXTURE_IDENTITY.to_owned(),
        base,
        target,
    }
}

fn ingest_probe_fixture() -> reviewgraphen_ingest::IngestResult {
    let repository = fixture_repository();
    write(
        &repository.repository,
        "src/probe.rs",
        "fn target() {}\n\n\
         pub fn foreign_safe_fn_declares_target() {\n    \
             extern \"C\" {\n        safe fn target();\n    }\n    target();\n}\n",
    );
    write(
        &repository.repository,
        "src/lib.rs",
        "mod api;\nmod probe;\n\npub fn entry() {\n    crate::api::worker();\n}\n",
    );
    git(&repository.repository, ["add", "."]);
    git(
        &repository.repository,
        ["commit", "--quiet", "-m", "foreign macro probe"],
    );
    let target_revision = git_stdout(&repository.repository, ["rev-parse", "HEAD"]);
    let mut request = repository.request();
    request.target_revision = target_revision;
    ingest(&request).expect("M2 ingest succeeds")
}

/// A macro inside a foreign module may expand to value-namespace `fn`/
/// `static` declarations that this pass cannot enumerate. The crate's own
/// posture for an unenumerable binding is conservatism, so the naked call
/// in that block must NOT be matched to the module-level `target`.
#[test]
fn unenumerable_foreign_item_conservatively_blocks_naked_call_resolution() {
    let result = ingest_probe_fixture();
    let target = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| artifact.kind == "function" && artifact.label == "crate::probe::target")
        .expect("module target function artifact exists")
        .id
        .clone();
    let caller = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| {
            artifact.kind == "function"
                && artifact.label == "crate::probe::foreign_safe_fn_declares_target"
        })
        .expect("caller function artifact exists")
        .id
        .clone();
    assert!(
        !result.program_space.relations().iter().any(|relation| {
            relation.kind == "calls"
                && relation.source_id == caller
                && relation.target_ids.contains(&target)
        }),
        "a foreign module containing an unenumerable macro item must conservatively \
         block naked-call resolution in that block, exactly as a glob `use` and an \
         opaque pattern already do"
    );
}
