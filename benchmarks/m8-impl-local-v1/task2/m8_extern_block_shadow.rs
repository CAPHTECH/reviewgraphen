//! Harness-owned acceptance test for m8-impl-local-v1 task 2.
//!
//! Written by the experiment harness, never by a candidate. Copied into
//! `crates/reviewgraphen-ingest/tests/m8_extern_block_shadow.rs` of an
//! isolated scratch copy before a candidate's edit is applied. A candidate
//! may only change `crates/reviewgraphen-ingest/src/rust.rs`.
//!
//! The helpers below are copied verbatim from `tests/m2.rs` so this file
//! stands alone; they are fixture plumbing, not the property under test.

use reviewgraphen_ingest::{IngestRequest, IngestionObstructionKind, ingest};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const FIXTURE_IDENTITY: &str = "reviewgraphen.test/m8-fixture";

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
        "[package]\nname = \"m8-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
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

fn artifact_id(result: &reviewgraphen_ingest::IngestResult, label: &str) -> reviewgraphen_core::StableId {
    result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| artifact.kind == "function" && artifact.label == label)
        .unwrap_or_else(|| panic!("{label} function artifact exists"))
        .id
        .clone()
}

/// The call must NOT be matched to the module-level target, and the
/// unresolved call must be retained as an explicit obstruction rather than
/// silently dropped.
fn assert_call_is_shadowed_not_module_target(
    result: &reviewgraphen_ingest::IngestResult,
    caller_label: &str,
    target_label: &str,
) {
    let target = artifact_id(result, target_label);
    let caller = artifact_id(result, caller_label);
    assert!(
        !result.program_space.relations().iter().any(|relation| {
            relation.kind == "calls"
                && relation.source_id == caller
                && relation.target_ids.contains(&target)
        }),
        "{caller_label} must never resolve its shadowed unqualified call to {target_label}"
    );
    assert!(
        result
            .extraction_report
            .obstructions
            .iter()
            .any(|obstruction| {
                obstruction.kind == IngestionObstructionKind::RelationUnresolved
                    && obstruction.source_ids.contains(&caller)
            }),
        "{caller_label}'s shadowed unqualified call must be retained as an unresolved obstruction"
    );
}

fn assert_call_resolves_to_module_target(
    result: &reviewgraphen_ingest::IngestResult,
    caller_label: &str,
    target_label: &str,
) {
    let target = artifact_id(result, target_label);
    let caller = artifact_id(result, caller_label);
    assert!(
        result.program_space.relations().iter().any(|relation| {
            relation.kind == "calls"
                && relation.source_id == caller
                && relation.target_ids.contains(&target)
        }),
        "{caller_label} must still resolve its unshadowed unqualified call to {target_label}"
    );
}

fn ingest_block_shadow_fixture() -> reviewgraphen_ingest::IngestResult {
    let repository = fixture_repository();
    write(
        &repository.repository,
        "src/blockshadow.rs",
        "fn target() {}\n\n\
         pub fn shadowed_by_extern_fn() {\n    \
             extern \"C\" {\n        fn target();\n    }\n    target();\n}\n\n\
         pub fn shadowed_by_extern_static() {\n    \
             extern \"C\" {\n        static target: u8;\n    }\n    target();\n}\n\n\
         pub fn extern_shadow_does_not_leak_past_its_block() {\n    \
             {\n        extern \"C\" {\n            fn target();\n        }\n        target();\n    }\n    target();\n}\n\n\
         pub fn extern_block_without_the_shadowed_name_still_resolves() {\n    \
             extern \"C\" {\n        fn unrelated();\n    }\n    target();\n}\n\n\
         pub fn unshadowed_call_still_resolves() {\n    target();\n}\n",
    );
    write(
        &repository.repository,
        "src/lib.rs",
        "mod api;\nmod blockshadow;\n\npub fn entry() {\n    crate::api::worker();\n}\n",
    );
    git(&repository.repository, ["add", "."]);
    git(
        &repository.repository,
        ["commit", "--quiet", "-m", "block-local extern item shadowing"],
    );
    let target_revision = git_stdout(&repository.repository, ["rev-parse", "HEAD"]);
    let mut request = repository.request();
    request.target_revision = target_revision;
    ingest(&request).expect("M2 ingest succeeds")
}

const MODULE_TARGET: &str = "crate::blockshadow::target";

#[test]
fn block_local_extern_fn_shadows_a_same_named_module_function() {
    let result = ingest_block_shadow_fixture();
    assert_call_is_shadowed_not_module_target(
        &result,
        "crate::blockshadow::shadowed_by_extern_fn",
        MODULE_TARGET,
    );
}

#[test]
fn block_local_extern_static_shadows_a_same_named_module_function() {
    let result = ingest_block_shadow_fixture();
    assert_call_is_shadowed_not_module_target(
        &result,
        "crate::blockshadow::shadowed_by_extern_static",
        MODULE_TARGET,
    );
}

#[test]
fn extern_block_shadow_does_not_leak_past_its_block() {
    let result = ingest_block_shadow_fixture();
    // The call after the block must still resolve, and exactly one resolved
    // edge must exist: a regression that failed to scope the shadow to the
    // block would additionally resolve the inner call, producing a second
    // distinct `calls` relation (the two call sites are on different lines,
    // so their relation IDs never collapse).
    assert_call_resolves_to_module_target(
        &result,
        "crate::blockshadow::extern_shadow_does_not_leak_past_its_block",
        MODULE_TARGET,
    );
    let target = artifact_id(&result, MODULE_TARGET);
    let caller = artifact_id(
        &result,
        "crate::blockshadow::extern_shadow_does_not_leak_past_its_block",
    );
    let resolved = result
        .program_space
        .relations()
        .iter()
        .filter(|relation| {
            relation.kind == "calls"
                && relation.source_id == caller
                && relation.target_ids.contains(&target)
        })
        .count();
    assert_eq!(
        resolved, 1,
        "exactly the call after the block may resolve; the one inside it must not"
    );
}

#[test]
fn an_extern_block_not_naming_the_called_symbol_does_not_block_resolution() {
    let result = ingest_block_shadow_fixture();
    assert_call_resolves_to_module_target(
        &result,
        "crate::blockshadow::extern_block_without_the_shadowed_name_still_resolves",
        MODULE_TARGET,
    );
}

#[test]
fn an_unshadowed_call_still_resolves() {
    let result = ingest_block_shadow_fixture();
    assert_call_resolves_to_module_target(
        &result,
        "crate::blockshadow::unshadowed_call_still_resolves",
        MODULE_TARGET,
    );
}
