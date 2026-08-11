use proptest::prelude::*;
use reviewgraphen_core::MvpRulePack;
use reviewgraphen_ingest::{
    CapabilityState, CargoToolAdmission, IngestConfig, IngestError, IngestLimits, IngestRequest,
    IngestResult, IngestionObstructionKind, ingest, ingest_with_sources,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

/// Stable identity shared by every clone of the fixture repository in these
/// tests; it must never be derived from a temporary clone's absolute path.
const FIXTURE_IDENTITY: &str = "reviewgraphen.test/m2-fixture";

const EXTRACTION_REPORT_SCHEMA: &[u8] =
    include_bytes!("../../../schemas/reviewgraphen.extraction_report.v1.schema.json");

/// Admits the trusted `cargo` binary these tests run Cargo metadata through,
/// entirely outside `reviewgraphen-ingest`'s own trust boundary: this is the
/// "caller/harness already admitted this executable" boundary
/// `CargoToolAdmission::TrustedExecutable` documents (see
/// docs/adr/0012-mise-host-admitted-cargo-toolchain.md) -- production
/// `ingest()` itself never resolves this path on its own; the default
/// `IngestConfig` stays `Disabled` unless a test explicitly opts in via
/// [`TempGitRepository::request_with_trusted_cargo`].
///
/// `REVIEWGRAPHEN_TRUSTED_CARGO` is the only admitted source: `mise run
/// test-ingest` is the official entry point for any test that calls this
/// function, and exports it from `scripts/resolve-trusted-cargo.sh`'s
/// mise-admitted, host-installed rust@1.95.0 `cargo`, which is the exact
/// pinned toolchain binary, never a rustup/mise multiplexer shim (see that
/// script's own documentation of why `mise which cargo` is unsuitable
/// here). There is deliberately no `CARGO` fallback: `cargo test`'s own
/// `$CARGO` names whatever binary happens to be running the test process,
/// which -- when `cargo` on `PATH` is a rustup/mise multiplexer shim -- can
/// itself be that shim's own path, re-dispatching to whichever toolchain is
/// active at call time rather than one fixed, independently verified
/// toolchain; a basename/proxy check alone cannot tell a shim from a genuine
/// toolchain binary, so it is not treated as an admission boundary here. A
/// direct `cargo test -p reviewgraphen-ingest` still works for every test
/// that does not need real Cargo metadata (see
/// [`TempGitRepository::request`]'s `Disabled` default); only the tests that
/// call this function require `mise run test-ingest`.
///
/// The canonical-basename check below is kept for defense in depth (a
/// directory or a wrapper script under any other name is still rejected),
/// matching the same posture `admit_cargo_executable`
/// (`crates/reviewgraphen-ingest/src/git.rs`) and
/// `scripts/resolve-trusted-cargo.sh` already apply to their own inputs.
fn trusted_test_cargo_admission() -> CargoToolAdmission {
    let cargo_path = std::env::var_os("REVIEWGRAPHEN_TRUSTED_CARGO")
        .map(PathBuf::from)
        .expect(
            "REVIEWGRAPHEN_TRUSTED_CARGO must name a cargo executable -- run this test through \
             `mise run test-ingest`, which resolves and exports it via \
             scripts/resolve-trusted-cargo.sh (see docs/adr/0012-mise-host-admitted-cargo-toolchain.md)",
        );
    let canonical = fs::canonicalize(&cargo_path)
        .expect("the admitted cargo path names an existing executable");
    assert_eq!(
        canonical.file_name().and_then(|name| name.to_str()),
        Some("cargo"),
        "admitted cargo executable's basename must be `cargo`: {canonical:?}"
    );
    CargoToolAdmission::TrustedExecutable(canonical)
}

struct TempGitRepository {
    workspace: TempDir,
    repository: PathBuf,
    identity: String,
    base: String,
    target: String,
}

impl TempGitRepository {
    /// `cargo_admission` defaults to `Disabled` (`IngestConfig::default()`),
    /// so most tests -- anything that does not itself assert on a
    /// Cargo-metadata-derived fact (a `"package"` artifact, a `depends_on`
    /// relation, `capabilities["cargo_metadata"]` being non-`Missing`, or an
    /// `adapter_set_hash`/fact-ID comparison against a run that used
    /// [`Self::request_with_trusted_cargo`]) -- run without needing a real,
    /// externally-admitted `cargo` at all. Only tests that genuinely need
    /// Cargo metadata to run should call
    /// [`Self::request_with_trusted_cargo`] instead.
    fn request(&self) -> IngestRequest {
        IngestRequest::new(
            self.workspace.path(),
            &self.repository,
            self.identity.clone(),
            &self.base,
            &self.target,
        )
    }

    /// Like [`Self::request`], but admits the external harness's trusted
    /// `cargo` (see `trusted_test_cargo_admission`) so Cargo metadata
    /// actually runs. Use only where the test's own assertions require it.
    fn request_with_trusted_cargo(&self) -> IngestRequest {
        let mut request = self.request();
        request.config.cargo_admission = trusted_test_cargo_admission();
        request
    }
}

fn fixture_repository() -> TempGitRepository {
    fixture_repository_with_identity(FIXTURE_IDENTITY)
}

fn zero_byte_repository() -> TempGitRepository {
    let workspace = tempfile::tempdir().expect("temporary workspace");
    let repository = workspace.path().join("zero-byte-fixture");
    fs::create_dir(&repository).expect("repository directory");
    git(&repository, ["init", "--quiet"]);
    git(
        &repository,
        ["config", "user.email", "reviewgraphen@example.test"],
    );
    git(&repository, ["config", "user.name", "ReviewGraphen test"]);
    write(&repository, "src/empty.rs", "");
    git(&repository, ["add", "."]);
    git(
        &repository,
        ["commit", "--quiet", "-m", "zero-byte snapshot"],
    );
    let revision = git_stdout(&repository, ["rev-parse", "HEAD"]);
    TempGitRepository {
        workspace,
        repository,
        identity: "reviewgraphen.test/zero-byte-fixture".to_owned(),
        base: revision.clone(),
        target: revision,
    }
}

fn fixture_repository_with_identity(identity: &str) -> TempGitRepository {
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
        "[package]\nname = \"m2-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    write(
        &repository,
        "src/lib.rs",
        "use crate::api::worker;\n\nmod api;\n\nmacro_rules! retained_macro { () => {}; }\n\npub fn entry() {\n    let mut state = 0;\n    state += 1;\n    worker();\n    retained_macro!();\n}\n\npub trait Runner { fn run(&self); }\n\npub fn dynamic(runner: &dyn Runner) {\n    runner.run();\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn covers_worker() {\n        crate::api::worker();\n    }\n}\n",
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
        identity: identity.to_owned(),
        base,
        target,
    }
}

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().expect("parent")).expect("source directory");
    fs::write(path, content).expect("source file");
}

fn commit_target_file(repository: &mut TempGitRepository, path: &str, content: &str) {
    write(&repository.repository, path, content);
    git(&repository.repository, ["add", "."]);
    git(
        &repository.repository,
        ["commit", "--quiet", "-m", "source bundle target"],
    );
    repository.target = git_stdout(&repository.repository, ["rev-parse", "HEAD"]);
}

fn copy_dir_recursive(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("destination directory");
    for entry in fs::read_dir(source).expect("read source directory") {
        let entry = entry.expect("directory entry");
        let file_type = entry.file_type().expect("entry file type");
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &target);
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target).expect("copy file");
        }
    }
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
    assert!(output.status.success(), "test Git output must succeed");
    String::from_utf8(output.stdout)
        .expect("UTF-8 Git output")
        .trim()
        .to_owned()
}

#[test]
fn ingests_a_bounded_git_snapshot_with_rust_facts_and_changed_structure() {
    let repository = fixture_repository();
    let result = ingest(&repository.request_with_trusted_cargo()).expect("M2 ingest succeeds");

    let serialized = serde_json::to_value(&result.program_space).unwrap();
    assert_eq!(
        serialized["schema"],
        serde_json::json!("reviewgraphen.program_space.input.v3")
    );
    let closure = result
        .program_space
        .accepted_git_revision_closure()
        .expect("native ingestion emits accepted v3 revision closure");
    let base_tree_spec = format!("{}^{{tree}}", repository.base);
    let target_tree_spec = format!("{}^{{tree}}", repository.target);
    assert_eq!(closure.base_commit_oid(), repository.base);
    assert_eq!(closure.target_commit_oid(), repository.target);
    assert_eq!(
        closure.base_tree_hash().as_str(),
        format!(
            "git:{}",
            git_stdout(&repository.repository, ["rev-parse", &base_tree_spec])
        )
    );
    assert_eq!(
        closure.target_tree_hash().as_str(),
        format!(
            "git:{}",
            git_stdout(&repository.repository, ["rev-parse", &target_tree_spec])
        )
    );
    let anchors = result
        .program_space
        .accepted_rust_symbol_anchors()
        .expect("native ingestion emits accepted v3 Rust anchors");
    assert_eq!(
        anchors.len(),
        result
            .program_space
            .artifacts()
            .iter()
            .filter(|artifact| {
                artifact.language.as_deref() == Some("rust")
                    && matches!(artifact.kind.as_str(), "function" | "method" | "type")
            })
            .count()
    );
    let relation_orders = result
        .program_space
        .accepted_relation_target_order()
        .expect("native ingestion emits accepted v3 relation order");
    assert!(result.program_space.relations().iter().all(|relation| {
        relation_orders.get(&relation.id).is_some_and(|ordered| {
            ordered.iter().cloned().collect::<BTreeSet<_>>() == relation.target_ids
        })
    }));

    let artifact_kinds = result
        .program_space
        .artifacts()
        .iter()
        .map(|artifact| artifact.kind.as_str())
        .collect::<Vec<_>>();
    assert!(artifact_kinds.contains(&"file"));
    assert!(artifact_kinds.contains(&"module"));
    assert!(artifact_kinds.contains(&"function"));
    assert!(artifact_kinds.contains(&"test"));
    assert!(artifact_kinds.contains(&"state"));
    assert!(
        artifact_kinds.contains(&"package"),
        "cargo report: {:#?}",
        result.extraction_report
    );
    let relation_kinds = result
        .program_space
        .relations()
        .iter()
        .map(|relation| relation.kind.as_str())
        .collect::<Vec<_>>();
    for expected in [
        "contains",
        "calls",
        "imports",
        "covers",
        "writes",
        "changed_by",
    ] {
        assert!(relation_kinds.contains(&expected), "missing {expected}");
    }
    assert_eq!(
        result.extraction_report.capabilities["changed_structure"],
        CapabilityState::Complete
    );
    assert!(
        result
            .program_space
            .artifacts()
            .iter()
            .filter(|artifact| artifact.kind == "function")
            .all(|artifact| artifact.location.is_some() && artifact.content_hash.is_some())
    );
}

#[test]
fn source_handoff_is_exact_bounded_deterministic_and_reuses_accepted_file_bytes() {
    let mut repository = fixture_repository();
    commit_target_file(&mut repository, "src/empty.rs", "");
    let request = repository.request();

    let ordinary = ingest(&request).expect("ordinary ingest remains available");
    let unbounded = ingest_with_sources(&request, u64::MAX).expect("source ingest succeeds");
    assert_eq!(unbounded.program_space, ordinary.program_space);
    assert_eq!(unbounded.extraction_report, ordinary.extraction_report);
    assert!(
        unbounded
            .source_bundle
            .entries()
            .iter()
            .any(|entry| entry.path() == "src/empty.rs" && entry.bytes().is_empty()),
        "zero-byte tracked regular files are retained"
    );

    let total = unbounded.source_bundle.total_bytes();
    let exact = ingest_with_sources(&request, total).expect("exact aggregate limit succeeds");
    assert_eq!(exact.source_bundle, unbounded.source_bundle);
    assert_eq!(
        exact
            .source_bundle
            .canonical_bytes()
            .expect("canonical source bytes"),
        ingest_with_sources(&request, total)
            .expect("repeat source ingest")
            .source_bundle
            .canonical_bytes()
            .expect("canonical source bytes"),
        "the same snapshot has byte-stable source handoff output"
    );
    assert!(
        exact
            .source_bundle
            .entries()
            .windows(2)
            .all(|pair| pair[0].path() < pair[1].path()),
        "entries are canonically ordered by path"
    );

    assert!(matches!(
        ingest_with_sources(&request, total - 1),
        Err(IngestError::SourceBundleTooLarge {
            max_total_source_bytes,
            actual_total_source_bytes,
        }) if max_total_source_bytes == total - 1 && actual_total_source_bytes > max_total_source_bytes
    ));
    assert!(
        ingest(&request).is_ok(),
        "a source-limit failure returns no partial result and does not alter ordinary ingestion"
    );

    for entry in exact.source_bundle.entries() {
        let artifact = exact
            .program_space
            .artifact(entry.artifact_id())
            .expect("source entry names an accepted artifact");
        assert_eq!(artifact.kind, "file");
        assert_eq!(
            artifact.content_hash.as_ref(),
            Some(entry.content_hash()),
            "the bundle uses the same already-read bytes whose hash the file artifact retained"
        );
    }
}

#[test]
fn source_handoff_accepts_an_all_zero_byte_snapshot_at_a_zero_byte_limit() {
    let repository = zero_byte_repository();
    let result = ingest_with_sources(&repository.request(), 0)
        .expect("zero-byte regular files fit a zero-byte aggregate limit");

    assert!(!result.source_bundle.entries().is_empty());
    assert_eq!(result.source_bundle.total_bytes(), 0);
    assert!(
        result
            .source_bundle
            .entries()
            .iter()
            .all(|entry| entry.bytes().is_empty()),
        "every tracked regular file in this snapshot is zero-byte"
    );
}

#[test]
fn unresolved_macro_and_dynamic_dispatch_remain_explicit_unknowns() {
    let repository = fixture_repository();
    let result = ingest(&repository.request()).expect("M2 ingest succeeds");
    let kinds = result
        .extraction_report
        .obstructions
        .iter()
        .map(|obstruction| obstruction.kind)
        .collect::<Vec<_>>();
    assert!(kinds.contains(&IngestionObstructionKind::MacroExpansionUnresolved));
    assert!(kinds.contains(&IngestionObstructionKind::DynamicDispatchUnresolved));
    assert_eq!(
        result.extraction_report.capabilities["direct_calls"],
        CapabilityState::Partial
    );
}

#[test]
fn qualified_call_to_an_unrelated_module_is_not_matched_to_a_same_named_local_target() {
    let repository = fixture_repository();
    write(
        &repository.repository,
        "src/risky.rs",
        "pub fn calls_external_worker() {\n    external::worker();\n}\n",
    );
    git(&repository.repository, ["add", "."]);
    git(
        &repository.repository,
        ["commit", "--quiet", "-m", "qualified external call"],
    );
    let target = git_stdout(&repository.repository, ["rev-parse", "HEAD"]);
    let mut request = repository.request();
    request.target_revision = target;
    let result = ingest(&request).expect("M2 ingest succeeds");

    let local_worker = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| artifact.kind == "function" && artifact.label == "crate::api::worker")
        .expect("local worker function artifact exists");
    let caller = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| {
            artifact.kind == "function" && artifact.label == "crate::risky::calls_external_worker"
        })
        .expect("caller function artifact exists");

    assert!(
        !result.program_space.relations().iter().any(|relation| {
            relation.kind == "calls"
                && relation.source_id == caller.id
                && relation.target_ids.contains(&local_worker.id)
        }),
        "a call to an unrelated `external::worker()` must never resolve to the local `api::worker`"
    );
    assert!(
        result
            .extraction_report
            .obstructions
            .iter()
            .any(|obstruction| {
                obstruction.kind == IngestionObstructionKind::RelationUnresolved
                    && obstruction.description.contains("external::worker")
            }),
        "the unresolved qualified call must be retained as a typed obstruction"
    );
}

#[test]
fn ufcs_style_call_is_not_matched_to_a_same_named_method_without_type_proof() {
    let repository = fixture_repository();
    write(
        &repository.repository,
        "src/risky.rs",
        "pub struct Widget;\n\nimpl Widget {\n    pub fn worker(&self) {}\n}\n\npub fn calls_via_ufcs(widget: &Widget) {\n    Widget::worker(widget);\n}\n",
    );
    git(&repository.repository, ["add", "."]);
    git(
        &repository.repository,
        ["commit", "--quiet", "-m", "ufcs-style call"],
    );
    let target = git_stdout(&repository.repository, ["rev-parse", "HEAD"]);
    let mut request = repository.request();
    request.target_revision = target;
    let result = ingest(&request).expect("M2 ingest succeeds");

    let widget_method = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| {
            artifact.kind == "method" && artifact.label == "crate::risky::worker::Widget"
        })
        .expect("Widget::worker method artifact exists");
    let caller = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| {
            artifact.kind == "function" && artifact.label == "crate::risky::calls_via_ufcs"
        })
        .expect("caller function artifact exists");

    assert!(
        !result.program_space.relations().iter().any(|relation| {
            relation.kind == "calls"
                && relation.source_id == caller.id
                && relation.target_ids.contains(&widget_method.id)
        }),
        "a UFCS-style `Type::method(&receiver)` call must not resolve without syntactic type proof"
    );
    assert!(
        result
            .extraction_report
            .obstructions
            .iter()
            .any(|obstruction| {
                obstruction.kind == IngestionObstructionKind::RelationUnresolved
                    && obstruction.description.contains("Widget::worker")
            }),
        "the unresolved UFCS-style call must be retained as a typed obstruction"
    );
}

#[test]
fn unqualified_call_is_not_matched_by_crate_wide_short_name_uniqueness() {
    // Minimal counterexample: a same-named function `drop` declared in an
    // unrelated local module, and a `use std::mem::drop;` import in the
    // caller's own module. Resolving the caller's bare `drop(value)` by
    // crate-wide short-name uniqueness (the previous behavior) would wrongly
    // bind it to the local `local_mod::drop`, even though the caller's
    // module never declares `drop` itself and the actually-in-scope `drop`
    // is the imported `std::mem::drop`, which M2 does not resolve.
    let repository = fixture_repository();
    write(
        &repository.repository,
        "src/local_mod.rs",
        "pub fn drop() {}\n",
    );
    write(
        &repository.repository,
        "src/caller.rs",
        "use std::mem::drop;\n\npub fn discard(value: String) {\n    drop(value);\n}\n",
    );
    git(&repository.repository, ["add", "."]);
    git(
        &repository.repository,
        ["commit", "--quiet", "-m", "unqualified call counterexample"],
    );
    let target = git_stdout(&repository.repository, ["rev-parse", "HEAD"]);
    let mut request = repository.request();
    request.target_revision = target;
    let result = ingest(&request).expect("M2 ingest succeeds");

    let local_drop = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| artifact.kind == "function" && artifact.label == "crate::local_mod::drop")
        .expect("local_mod::drop function artifact exists");
    let caller = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| artifact.kind == "function" && artifact.label == "crate::caller::discard")
        .expect("caller::discard function artifact exists");

    assert!(
        !result.program_space.relations().iter().any(|relation| {
            relation.kind == "calls"
                && relation.source_id == caller.id
                && relation.target_ids.contains(&local_drop.id)
        }),
        "an unqualified call to a `use`-imported name must never resolve to an unrelated \
         same-named local item just because it is the only match crate-wide"
    );
    assert!(
        !result
            .program_space
            .relations()
            .iter()
            .any(|relation| { relation.kind == "calls" && relation.source_id == caller.id }),
        "the caller's unqualified `drop(value)` must not resolve to any accepted call target"
    );
    assert!(
        result
            .extraction_report
            .obstructions
            .iter()
            .any(|obstruction| {
                obstruction.kind == IngestionObstructionKind::RelationUnresolved
                    && obstruction.source_ids.contains(&caller.id)
            }),
        "the unresolved unqualified call must be retained as a typed obstruction"
    );
}

/// Shared oracle for a same-module unqualified call that is shadowed by a
/// local binding: it must never resolve to the same-named module function,
/// and must be retained as a typed `relation_unresolved` obstruction
/// grounded in the caller.
fn assert_call_is_shadowed_not_module_target(
    result: &reviewgraphen_ingest::IngestResult,
    caller_label: &str,
    target_label: &str,
) {
    let target = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| artifact.kind == "function" && artifact.label == target_label)
        .unwrap_or_else(|| panic!("{target_label} function artifact exists"));
    let caller = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| artifact.kind == "function" && artifact.label == caller_label)
        .unwrap_or_else(|| panic!("{caller_label} function artifact exists"));

    assert!(
        !result.program_space.relations().iter().any(|relation| {
            relation.kind == "calls"
                && relation.source_id == caller.id
                && relation.target_ids.contains(&target.id)
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
                    && obstruction.source_ids.contains(&caller.id)
            }),
        "{caller_label}'s shadowed unqualified call must be retained as an unresolved obstruction"
    );
}

/// Shared oracle for a same-module unqualified call that is genuinely
/// unshadowed: it must still resolve to the same-named module function,
/// exactly as before this shadow-tracking fix.
fn assert_call_resolves_to_module_target(
    result: &reviewgraphen_ingest::IngestResult,
    caller_label: &str,
    target_label: &str,
) {
    let target = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| artifact.kind == "function" && artifact.label == target_label)
        .unwrap_or_else(|| panic!("{target_label} function artifact exists"));
    let caller = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| artifact.kind == "function" && artifact.label == caller_label)
        .unwrap_or_else(|| panic!("{caller_label} function artifact exists"));

    assert!(
        result.program_space.relations().iter().any(|relation| {
            relation.kind == "calls"
                && relation.source_id == caller.id
                && relation.target_ids.contains(&target.id)
        }),
        "{caller_label} must still resolve its unshadowed unqualified call to {target_label}"
    );
}

#[test]
fn unqualified_call_shadowed_by_a_local_binding_is_never_matched_to_a_module_function() {
    // Minimal counterexample from the audit finding: `fn target(){}` at
    // module scope, and `fn caller(target: impl Fn()){ target(); }` --
    // resolving `caller`'s `target()` to the module function would be
    // wrong; Rust's own scoping rules mean the parameter always shadows it,
    // whether or not the shadowing binding happens to be callable in
    // practice. Each function below exercises one distinct shadowing
    // construct against the same module-level `target`.
    let repository = fixture_repository();
    write(
        &repository.repository,
        "src/shadowing.rs",
        "fn target() {}\n\n\
         pub fn shadowed_by_parameter(target: impl Fn()) {\n    target();\n}\n\n\
         pub fn shadowed_by_let_binding() {\n    let target = ();\n    target();\n}\n\n\
         pub fn shadowed_by_closure_parameter() {\n    \
             let _closure = |target: ()| {\n        target();\n    };\n}\n\n\
         pub fn shadowed_by_pattern_binding() {\n    \
             let (target, _unused) = ((), 1);\n    target();\n}\n\n\
         pub fn shadowed_by_match_arm() {\n    \
             match Some(()) {\n        Some(target) => target(),\n        None => {}\n    }\n}\n\n\
         pub fn shadowed_by_for_loop_pattern() {\n    \
             for target in [()] {\n        target();\n    }\n}\n\n\
         pub fn shadowed_by_if_let() {\n    \
             if let Some(target) = Some(()) {\n        target();\n    }\n}\n\n\
         pub fn shadow_does_not_leak_past_its_block() {\n    \
             {\n        let target = ();\n        target();\n    }\n    target();\n}\n\n\
         pub fn unshadowed_call_still_resolves() {\n    target();\n}\n",
    );
    git(&repository.repository, ["add", "."]);
    git(
        &repository.repository,
        ["commit", "--quiet", "-m", "local shadowing counterexamples"],
    );
    let target_revision = git_stdout(&repository.repository, ["rev-parse", "HEAD"]);
    let mut request = repository.request();
    request.target_revision = target_revision;
    let result = ingest(&request).expect("M2 ingest succeeds");

    const MODULE_TARGET: &str = "crate::shadowing::target";
    for caller in [
        "crate::shadowing::shadowed_by_parameter",
        "crate::shadowing::shadowed_by_let_binding",
        "crate::shadowing::shadowed_by_closure_parameter",
        "crate::shadowing::shadowed_by_pattern_binding",
        "crate::shadowing::shadowed_by_match_arm",
        "crate::shadowing::shadowed_by_for_loop_pattern",
        "crate::shadowing::shadowed_by_if_let",
    ] {
        assert_call_is_shadowed_not_module_target(&result, caller, MODULE_TARGET);
    }

    // The shadow inside the block must not leak past it: the call after
    // the block, once the local `target` has gone out of scope, resolves
    // to the module function exactly like the fully unshadowed case.
    assert_call_resolves_to_module_target(
        &result,
        "crate::shadowing::shadow_does_not_leak_past_its_block",
        MODULE_TARGET,
    );
    assert_call_resolves_to_module_target(
        &result,
        "crate::shadowing::unshadowed_call_still_resolves",
        MODULE_TARGET,
    );

    // `shadow_does_not_leak_past_its_block` contains two `target()` call
    // sites on different lines: one shadowed (inside the block, which must
    // stay unresolved) and one not (after the block, which must resolve).
    // A regression that failed to pop the block's shadow would additionally
    // resolve the *inner* call too, producing a second, distinct `calls`
    // relation (each call site's `line` attribute differs, so the two
    // would never collapse into the same relation ID) -- checking only
    // "at least one resolved edge exists" would miss that regression, so
    // this asserts the count is exactly one.
    let module_target = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| artifact.kind == "function" && artifact.label == MODULE_TARGET)
        .expect("module target function artifact exists");
    let leak_caller = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| {
            artifact.kind == "function"
                && artifact.label == "crate::shadowing::shadow_does_not_leak_past_its_block"
        })
        .expect("shadow_does_not_leak_past_its_block function artifact exists");
    let resolved_edge_count = result
        .program_space
        .relations()
        .iter()
        .filter(|relation| {
            relation.kind == "calls"
                && relation.source_id == leak_caller.id
                && relation.target_ids.contains(&module_target.id)
        })
        .count();
    assert_eq!(
        resolved_edge_count, 1,
        "exactly the post-block call must resolve to the module target, not the shadowed \
         in-block call too"
    );
}

#[test]
fn block_local_use_const_static_and_glob_imports_are_tracked_as_shadows() {
    // Re-audit counterexample: `fn target(){}; mod ext{pub fn target(){}}
    // fn caller(){ use ext::target; target(); }` -- a block-local `use`
    // must shadow the module-level `target` exactly like a `let`/parameter
    // already does, never letting `caller`'s call bind to `ext::target`'s
    // *sibling* same-module declaration. Also covers block-local
    // `const`/`static` shadowing, a renamed `use` binding only its alias
    // (not the original name), and a glob/unenumerable `use` making an
    // entire scope's unqualified-call resolution conservative -- even for
    // a call whose name has nothing to do with the glob's own target.
    let repository = fixture_repository();
    write(
        &repository.repository,
        "src/block_scope_shadowing.rs",
        "fn target() {}\n\n\
         mod ext {\n    pub fn target() {}\n}\n\n\
         fn helper_under_glob() {}\n\n\
         pub fn shadowed_by_block_use() {\n    \
             use ext::target;\n    target();\n}\n\n\
         pub fn unshadowed_by_renamed_block_use() {\n    \
             use ext::target as renamed_target;\n    target();\n}\n\n\
         pub fn shadowed_by_block_const() {\n    \
             const target: () = ();\n    target();\n}\n\n\
         pub fn shadowed_by_block_static() {\n    \
             static target: () = ();\n    target();\n}\n\n\
         pub fn call_unresolved_by_glob_import() {\n    \
             use ext::*;\n    target();\n}\n\n\
         pub fn call_blocked_by_glob_even_when_name_is_unambiguous() {\n    \
             use ext::*;\n    helper_under_glob();\n}\n\n\
         pub fn let_chain_binding_is_visible_to_a_later_chain_element() {\n    \
             if let target = (|| {}) && target() {}\n}\n\n\
         pub fn let_chain_binding_is_not_visible_to_an_earlier_chain_element() {\n    \
             if target() && let target = Some(()) {}\n}\n",
    );
    git(&repository.repository, ["add", "."]);
    git(
        &repository.repository,
        [
            "commit",
            "--quiet",
            "-m",
            "block-scope shadowing counterexamples",
        ],
    );
    let target_revision = git_stdout(&repository.repository, ["rev-parse", "HEAD"]);
    let mut request = repository.request();
    request.target_revision = target_revision;
    let result = ingest(&request).expect("M2 ingest succeeds");

    const MODULE_TARGET: &str = "crate::block_scope_shadowing::target";
    for caller in [
        "crate::block_scope_shadowing::shadowed_by_block_use",
        "crate::block_scope_shadowing::shadowed_by_block_const",
        "crate::block_scope_shadowing::shadowed_by_block_static",
        "crate::block_scope_shadowing::call_unresolved_by_glob_import",
        "crate::block_scope_shadowing::let_chain_binding_is_visible_to_a_later_chain_element",
    ] {
        assert_call_is_shadowed_not_module_target(&result, caller, MODULE_TARGET);
    }
    // A renamed `use` only binds its alias, never the original name, so an
    // unrelated bare `target()` call in that same scope is unshadowed.
    assert_call_resolves_to_module_target(
        &result,
        "crate::block_scope_shadowing::unshadowed_by_renamed_block_use",
        MODULE_TARGET,
    );
    // A chain element visited *before* a later `let` binds it must not see
    // that later binding -- ordering runs strictly left to right, never
    // treating the whole condition as one flat, order-independent scope.
    assert_call_resolves_to_module_target(
        &result,
        "crate::block_scope_shadowing::let_chain_binding_is_not_visible_to_an_earlier_chain_element",
        MODULE_TARGET,
    );

    // The glob-blocked call to `helper_under_glob` must never resolve to
    // it, and must be retained as an unresolved obstruction -- proving the
    // glob's conservative effect covers the *whole* scope, not only calls
    // whose name happens to already be a known local binding.
    let helper = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| {
            artifact.kind == "function"
                && artifact.label == "crate::block_scope_shadowing::helper_under_glob"
        })
        .expect("helper_under_glob function artifact exists");
    let glob_caller = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| {
            artifact.kind == "function"
                && artifact.label
                    == "crate::block_scope_shadowing::call_blocked_by_glob_even_when_name_is_unambiguous"
        })
        .expect("call_blocked_by_glob_even_when_name_is_unambiguous function artifact exists");
    assert!(
        !result.program_space.relations().iter().any(|relation| {
            relation.kind == "calls"
                && relation.source_id == glob_caller.id
                && relation.target_ids.contains(&helper.id)
        }),
        "a call in a scope reachable by a glob import must never resolve, even to an \
         otherwise-unambiguous same-module function"
    );
    assert!(
        result
            .extraction_report
            .obstructions
            .iter()
            .any(|obstruction| {
                obstruction.kind == IngestionObstructionKind::RelationUnresolved
                    && obstruction.source_ids.contains(&glob_caller.id)
            }),
        "the glob-blocked call must be retained as an unresolved obstruction"
    );
}

#[test]
fn same_module_naked_call_lookup_never_matches_an_impl_method() {
    // Audit counterexample: an `impl` method is never callable via bare
    // `name()` syntax in real Rust (only `self.name()`/method-call syntax,
    // always unresolved, or a type-qualified/UFCS path this crate does not
    // resolve without type proof). Registering it under the same-module
    // naked-call lookup anyway would let an unrelated bare call in that
    // module -- here, a name that is actually an unresolvable imported
    // symbol -- wrongly bind to the method just because it is the only
    // same-module declaration sharing that identifier. The same shape is
    // repeated inside a `#[cfg(test)]` module, so a `#[test]` function's
    // bare call must not wrongly produce a `covers` edge to a test-scoped
    // mock's same-named method either. `helper`/`uses_helper` is the
    // control: an actual free function's bare call must still resolve.
    let repository = fixture_repository();
    write(
        &repository.repository,
        "src/impl_shadow.rs",
        "use ext::worker;\n\n\
         struct W;\n\n\
         impl W {\n    fn worker() {}\n}\n\n\
         fn helper() {}\n\n\
         fn caller() {\n    worker();\n}\n\n\
         fn uses_helper() {\n    helper();\n}\n\n\
         #[cfg(test)]\n\
         mod tests {\n    \
             struct Mock;\n\n    \
             impl Mock {\n        fn worker() {}\n    }\n\n    \
             #[test]\n    \
             fn impl_shadow_covers_worker() {\n        worker();\n    }\n}\n",
    );
    git(&repository.repository, ["add", "."]);
    git(
        &repository.repository,
        [
            "commit",
            "--quiet",
            "-m",
            "impl-method naked-call counterexample",
        ],
    );
    let target_revision = git_stdout(&repository.repository, ["rev-parse", "HEAD"]);
    let mut request = repository.request();
    request.target_revision = target_revision;
    let result = ingest(&request).expect("M2 ingest succeeds");

    let w_worker = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| {
            artifact.kind == "method" && artifact.label == "crate::impl_shadow::worker::W"
        })
        .expect("W::worker method artifact exists");
    let caller = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| {
            artifact.kind == "function" && artifact.label == "crate::impl_shadow::caller"
        })
        .expect("caller function artifact exists");

    assert!(
        !result.program_space.relations().iter().any(|relation| {
            relation.kind == "calls"
                && relation.source_id == caller.id
                && relation.target_ids.contains(&w_worker.id)
        }),
        "an unqualified call must never resolve to a same-named impl method, whether the name \
         is otherwise an unresolvable imported symbol or anything else"
    );
    assert!(
        result
            .extraction_report
            .obstructions
            .iter()
            .any(|obstruction| {
                obstruction.kind == IngestionObstructionKind::RelationUnresolved
                    && obstruction.source_ids.contains(&caller.id)
            }),
        "the caller's unqualified `worker()` must be retained as an unresolved obstruction"
    );

    let mock_worker = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| {
            artifact.kind == "method" && artifact.label == "crate::impl_shadow::tests::worker::Mock"
        })
        .expect("Mock::worker method artifact exists");
    let covers_worker_test = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| artifact.kind == "test" && artifact.label == "impl_shadow_covers_worker")
        .expect("impl_shadow_covers_worker test artifact exists");

    assert!(
        !result.program_space.relations().iter().any(|relation| {
            relation.kind == "calls"
                && relation.source_id == covers_worker_test.id
                && relation.target_ids.contains(&mock_worker.id)
        }),
        "a test function's unqualified call must never resolve to a same-named test-scoped \
         impl method"
    );
    assert!(
        !result.program_space.relations().iter().any(|relation| {
            relation.kind == "covers"
                && relation.source_id == covers_worker_test.id
                && relation.target_ids.contains(&mock_worker.id)
        }),
        "a `covers` edge must never be synthesized from a call that never resolved"
    );
    assert!(
        result
            .extraction_report
            .obstructions
            .iter()
            .any(|obstruction| {
                obstruction.kind == IngestionObstructionKind::RelationUnresolved
                    && obstruction.source_ids.contains(&covers_worker_test.id)
            }),
        "the test function's unqualified `worker()` must be retained as an unresolved obstruction"
    );

    // Control: a genuine free function's unqualified call in the same
    // module must still resolve, exactly as before this fix.
    let helper = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| {
            artifact.kind == "function" && artifact.label == "crate::impl_shadow::helper"
        })
        .expect("helper function artifact exists");
    let uses_helper = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| {
            artifact.kind == "function" && artifact.label == "crate::impl_shadow::uses_helper"
        })
        .expect("uses_helper function artifact exists");
    assert!(
        result.program_space.relations().iter().any(|relation| {
            relation.kind == "calls"
                && relation.source_id == uses_helper.id
                && relation.target_ids.contains(&helper.id)
        }),
        "a genuine same-module free-function call must still resolve"
    );
}

#[test]
fn block_local_tuple_and_unit_struct_constructors_are_tracked_as_value_namespace_shadows() {
    // Audit counterexample: `fn target(){} fn caller(){ struct target();
    // target(); }` -- a block-local tuple (or unit) struct puts its own name
    // in the *value* namespace as its own constructor, exactly like a local
    // `fn`/`let` binding, and must shadow the outer/module `fn target` for a
    // bare `target()` call in that block, whether or not the resulting call
    // itself type-checks (the same syntactic-only posture already used for
    // `let target = ();` in
    // `unqualified_call_shadowed_by_a_local_binding_is_never_matched_to_a_module_function`).
    // A block-local *named-field* struct is the opposite: real Rust puts a
    // named-field struct's name only in the *type* namespace, so it never
    // shadows a same-named callable at all -- that block's own bare call
    // must still resolve to the module function, exactly as if the struct
    // were not declared.
    let repository = fixture_repository();
    write(
        &repository.repository,
        "src/struct_shadowing.rs",
        "fn target() {}\n\n\
         pub fn shadowed_by_tuple_struct() {\n    \
             struct target();\n    target();\n}\n\n\
         pub fn shadowed_by_unit_struct() {\n    \
             struct target;\n    target();\n}\n\n\
         pub fn unshadowed_by_named_field_struct() {\n    \
             struct target { value: i32 }\n    target();\n}\n\n\
         pub fn tuple_struct_shadow_does_not_leak_past_its_block() {\n    \
             {\n        struct target();\n        target();\n    }\n    target();\n}\n",
    );
    git(&repository.repository, ["add", "."]);
    git(
        &repository.repository,
        [
            "commit",
            "--quiet",
            "-m",
            "block-local struct value-namespace counterexamples",
        ],
    );
    let target_revision = git_stdout(&repository.repository, ["rev-parse", "HEAD"]);
    let mut request = repository.request();
    request.target_revision = target_revision;
    let result = ingest(&request).expect("M2 ingest succeeds");

    const MODULE_TARGET: &str = "crate::struct_shadowing::target";
    for caller in [
        "crate::struct_shadowing::shadowed_by_tuple_struct",
        "crate::struct_shadowing::shadowed_by_unit_struct",
    ] {
        assert_call_is_shadowed_not_module_target(&result, caller, MODULE_TARGET);
    }
    assert_call_resolves_to_module_target(
        &result,
        "crate::struct_shadowing::unshadowed_by_named_field_struct",
        MODULE_TARGET,
    );
    assert_call_resolves_to_module_target(
        &result,
        "crate::struct_shadowing::tuple_struct_shadow_does_not_leak_past_its_block",
        MODULE_TARGET,
    );

    // Exactly the post-block call must resolve; the in-block call, shadowed
    // by the block-local tuple struct constructor, must not add a second
    // resolved edge (the same leak-detection shape as the existing
    // `shadow_does_not_leak_past_its_block` oracle).
    let module_target = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| artifact.kind == "function" && artifact.label == MODULE_TARGET)
        .expect("module target function artifact exists");
    let leak_caller = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| {
            artifact.kind == "function"
                && artifact.label
                    == "crate::struct_shadowing::tuple_struct_shadow_does_not_leak_past_its_block"
        })
        .expect("tuple_struct_shadow_does_not_leak_past_its_block function artifact exists");
    let resolved_edge_count = result
        .program_space
        .relations()
        .iter()
        .filter(|relation| {
            relation.kind == "calls"
                && relation.source_id == leak_caller.id
                && relation.target_ids.contains(&module_target.id)
        })
        .count();
    assert_eq!(
        resolved_edge_count, 1,
        "exactly the post-block call must resolve to the module target, not the tuple-struct-\
         shadowed in-block call too"
    );
}

#[test]
fn pat_macro_and_verbatim_patterns_conservatively_block_naked_call_resolution() {
    // Audit counterexample: a `let`/`let-else` (or any other pattern
    // position) binding whose pattern `syn` cannot resolve into a known
    // shape -- a pattern-position macro invocation (`Pat::Macro`, for
    // example `let mymac!(target) = ..;`) or an opaque, unparsed pattern
    // (`Pat::Verbatim`, the shape `syn` produces for `let box target = ..;`)
    // -- may bind any name at all once expanded/interpreted, and `syn`
    // cannot tell this crate which. Treating such a pattern as contributing
    // zero bindings (silently, like a truly no-binding pattern such as `_`)
    // would let a bare call in that scope wrongly resolve to a same-named
    // module function it may actually be shadowed by. Instead the whole
    // enclosing scope must become conservatively unresolved, exactly like a
    // glob `use` (see
    // `block_local_use_const_static_and_glob_imports_are_tracked_as_shadows`):
    // even a call whose name never appears anywhere in the opaque pattern's
    // own tokens must not resolve either, since what it actually binds can
    // never be proven.
    let repository = fixture_repository();
    write(
        &repository.repository,
        "src/pat_unknown_shadowing.rs",
        "fn target() {}\n\n\
         pub fn shadowed_by_pattern_macro() {\n    \
             let mymac!(target) = ();\n    target();\n}\n\n\
         pub fn shadowed_by_box_pattern() {\n    \
             let box target = ();\n    target();\n}\n\n\
         pub fn unrelated_call_is_blocked_by_pattern_macro_too() {\n    \
             let mymac!(unrelated) = ();\n    target();\n}\n\n\
         pub fn conservative_scope_does_not_leak_past_its_block() {\n    \
             {\n        let box target = ();\n        target();\n    }\n    target();\n}\n",
    );
    git(&repository.repository, ["add", "."]);
    git(
        &repository.repository,
        [
            "commit",
            "--quiet",
            "-m",
            "unknown-pattern conservative-shadow counterexamples",
        ],
    );
    let target_revision = git_stdout(&repository.repository, ["rev-parse", "HEAD"]);
    let mut request = repository.request();
    request.target_revision = target_revision;
    let result = ingest(&request).expect("M2 ingest succeeds");

    const MODULE_TARGET: &str = "crate::pat_unknown_shadowing::target";
    for caller in [
        "crate::pat_unknown_shadowing::shadowed_by_pattern_macro",
        "crate::pat_unknown_shadowing::shadowed_by_box_pattern",
        "crate::pat_unknown_shadowing::unrelated_call_is_blocked_by_pattern_macro_too",
    ] {
        assert_call_is_shadowed_not_module_target(&result, caller, MODULE_TARGET);
    }
    assert_call_resolves_to_module_target(
        &result,
        "crate::pat_unknown_shadowing::conservative_scope_does_not_leak_past_its_block",
        MODULE_TARGET,
    );

    // Exactly the post-block call must resolve; the in-block call, blocked
    // by the box-pattern's conservative scope, must not add a second
    // resolved edge.
    let module_target = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| artifact.kind == "function" && artifact.label == MODULE_TARGET)
        .expect("module target function artifact exists");
    let leak_caller = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| {
            artifact.kind == "function"
                && artifact.label
                    == "crate::pat_unknown_shadowing::conservative_scope_does_not_leak_past_its_block"
        })
        .expect("conservative_scope_does_not_leak_past_its_block function artifact exists");
    let resolved_edge_count = result
        .program_space
        .relations()
        .iter()
        .filter(|relation| {
            relation.kind == "calls"
                && relation.source_id == leak_caller.id
                && relation.target_ids.contains(&module_target.id)
        })
        .count();
    assert_eq!(
        resolved_edge_count, 1,
        "exactly the post-block call must resolve to the module target, not the box-pattern-\
         conservative in-block call too"
    );
}

/// Audit counterexample: `FunctionBodyVisitor`'s `visit_local`/`visit_arm`/
/// `visit_expr_closure`/`visit_expr_for_loop` overrides never delegate to
/// `syn::visit::Visit`'s default pattern traversal (each fully replaces it
/// to control scope-push/pop timing instead), so an opaque pattern
/// (`Pat::Macro`/`Pat::Verbatim`) was previously only ever detected through
/// `has_unknown`, which feeds the shadowing/conservative-scope logic, not
/// the obstruction ledger -- a function containing only an opaque pattern
/// and no other call would retain zero trace of it at all. This asserts the
/// fix directly: a `let` binding whose pattern is a pattern-position macro
/// invocation, or an opaque unparsed (`box`) pattern, must be retained as a
/// typed, function-source-grounded obstruction even with no subsequent call
/// anywhere in the function to otherwise trip an unresolved-relation
/// obstruction.
#[test]
fn opaque_local_pattern_without_a_subsequent_call_is_still_recorded_as_an_obstruction() {
    let repository = fixture_repository();
    write(
        &repository.repository,
        "src/pat_opaque_local.rs",
        "pub fn macro_pattern_without_a_call() {\n    \
             let mymac!(target) = ();\n}\n\n\
         pub fn verbatim_pattern_without_a_call() {\n    \
             let box target = ();\n}\n\n\
         pub fn two_macro_patterns_without_a_call() {\n    \
             let mymac!(first) = ();\n    \
             let mymac!(second) = ();\n}\n",
    );
    git(&repository.repository, ["add", "."]);
    git(
        &repository.repository,
        [
            "commit",
            "--quiet",
            "-m",
            "opaque local pattern without a subsequent call",
        ],
    );
    let target_revision = git_stdout(&repository.repository, ["rev-parse", "HEAD"]);
    let mut request = repository.request();
    request.target_revision = target_revision;
    let result = ingest(&request).expect("M2 ingest succeeds");

    let function_id = |label: &str| {
        result
            .program_space
            .artifacts()
            .iter()
            .find(|artifact| artifact.kind == "function" && artifact.label == label)
            .unwrap_or_else(|| panic!("{label} function artifact exists"))
            .id
            .clone()
    };
    let obstructions_for = |source_id: &reviewgraphen_core::StableId,
                            kind: IngestionObstructionKind| {
        result
            .extraction_report
            .obstructions
            .iter()
            .filter(|obstruction| {
                obstruction.kind == kind && obstruction.source_ids.contains(source_id)
            })
            .count()
    };

    let macro_pattern_fn = function_id("crate::pat_opaque_local::macro_pattern_without_a_call");
    assert_eq!(
        obstructions_for(
            &macro_pattern_fn,
            IngestionObstructionKind::MacroExpansionUnresolved
        ),
        1,
        "a pattern-position macro invocation with no other call in its function must still be \
         retained as exactly one macro_expansion_unresolved obstruction grounded to that function"
    );

    let verbatim_pattern_fn =
        function_id("crate::pat_opaque_local::verbatim_pattern_without_a_call");
    assert_eq!(
        obstructions_for(&verbatim_pattern_fn, IngestionObstructionKind::Unknown),
        1,
        "an opaque unparsed (box) pattern with no other call in its function must still be \
         retained as exactly one unknown obstruction grounded to that function"
    );

    // Two distinct opaque patterns in the same function must each be
    // recorded once -- neither collapsed into a single obstruction nor
    // multiplied by any recursive re-visit of the same pattern node.
    let two_patterns_fn = function_id("crate::pat_opaque_local::two_macro_patterns_without_a_call");
    assert_eq!(
        obstructions_for(
            &two_patterns_fn,
            IngestionObstructionKind::MacroExpansionUnresolved
        ),
        2,
        "two distinct pattern-position macro invocations in one function must each be recorded \
         exactly once, with neither dropped nor duplicated"
    );
}

/// The same opaque-pattern-without-a-subsequent-call gap, exercised across
/// every other lexical-scope-pushing pattern position `FunctionBodyVisitor`
/// handles: a closure parameter, a match arm, and a `for` loop variable.
/// Each of these overrides (`visit_expr_closure`/`visit_arm`/
/// `visit_expr_for_loop`) pushes its own scope and, like `visit_local`,
/// never delegates to `syn::visit::Visit`'s default pattern traversal.
#[test]
fn opaque_pattern_in_closure_arm_and_for_loop_without_a_subsequent_call_is_still_recorded() {
    let repository = fixture_repository();
    write(
        &repository.repository,
        "src/pat_opaque_scopes.rs",
        "pub fn closure_macro_pattern_without_a_call() {\n    \
             let _ = |mymac!(target)| ();\n}\n\n\
         pub fn arm_macro_pattern_without_a_call() {\n    \
             match () {\n        mymac!(target) => {}\n    }\n}\n\n\
         pub fn for_loop_macro_pattern_without_a_call() {\n    \
             for mymac!(target) in [()] {}\n}\n",
    );
    git(&repository.repository, ["add", "."]);
    git(
        &repository.repository,
        [
            "commit",
            "--quiet",
            "-m",
            "opaque push-scope pattern without a subsequent call",
        ],
    );
    let target_revision = git_stdout(&repository.repository, ["rev-parse", "HEAD"]);
    let mut request = repository.request();
    request.target_revision = target_revision;
    let result = ingest(&request).expect("M2 ingest succeeds");

    let function_id = |label: &str| {
        result
            .program_space
            .artifacts()
            .iter()
            .find(|artifact| artifact.kind == "function" && artifact.label == label)
            .unwrap_or_else(|| panic!("{label} function artifact exists"))
            .id
            .clone()
    };

    for label in [
        "crate::pat_opaque_scopes::closure_macro_pattern_without_a_call",
        "crate::pat_opaque_scopes::arm_macro_pattern_without_a_call",
        "crate::pat_opaque_scopes::for_loop_macro_pattern_without_a_call",
    ] {
        let source_id = function_id(label);
        let matching = result
            .extraction_report
            .obstructions
            .iter()
            .filter(|obstruction| {
                obstruction.kind == IngestionObstructionKind::MacroExpansionUnresolved
                    && obstruction.source_ids.contains(&source_id)
            })
            .count();
        assert_eq!(
            matching, 1,
            "{label}'s pattern-position macro invocation, with no other call in the function, \
             must still be retained as exactly one macro_expansion_unresolved obstruction"
        );
    }
}

#[test]
fn parse_failure_is_retained_without_aborting_other_snapshot_facts() {
    let repository = fixture_repository();
    write(
        &repository.repository,
        "src/broken.rs",
        "pub fn broken( {\n",
    );
    git(&repository.repository, ["add", "."]);
    git(
        &repository.repository,
        ["commit", "--quiet", "-m", "broken"],
    );
    let target = git_stdout(&repository.repository, ["rev-parse", "HEAD"]);
    let mut request = repository.request();
    request.target_revision = target;
    let result = ingest(&request).expect("parse failure becomes an obstruction");
    assert!(
        result
            .program_space
            .artifacts()
            .iter()
            .any(|artifact| { artifact.kind == "file" && artifact.label == "src/broken.rs" })
    );
    assert!(
        result
            .extraction_report
            .obstructions
            .iter()
            .any(|obstruction| obstruction.kind == IngestionObstructionKind::ParseFailure)
    );
    assert_eq!(
        result.extraction_report.capabilities["ast"],
        CapabilityState::Partial
    );
}

#[test]
fn target_worktree_is_not_modified() {
    let repository = fixture_repository();
    let before = git_stdout(&repository.repository, ["status", "--porcelain"]);
    let _ = ingest(&repository.request()).expect("ingest succeeds");
    let after = git_stdout(&repository.repository, ["status", "--porcelain"]);
    assert_eq!(before, after);
}

#[test]
fn workspace_escape_is_rejected_before_git_ingestion() {
    let repository = fixture_repository();
    let other_workspace = tempfile::tempdir().expect("unrelated workspace");
    let request = IngestRequest::new(
        other_workspace.path(),
        &repository.repository,
        repository.identity.clone(),
        &repository.base,
        &repository.target,
    );
    assert!(matches!(
        ingest(&request),
        Err(IngestError::WorkspaceEscape { .. })
    ));
}

#[test]
fn configured_blob_bound_is_enforced() {
    let repository = fixture_repository();
    let mut request = repository.request();
    request.config = IngestConfig {
        limits: IngestLimits {
            max_files: 100,
            max_file_bytes: 4,
        },
        ..IngestConfig::default()
    };
    assert!(matches!(
        ingest(&request),
        Err(IngestError::FileTooLarge { .. })
    ));
}

#[cfg(unix)]
/// Shared oracle for an excluded Git tree entry (symlink, submodule, or
/// another unsupported mode/type): `git_snapshot` must always downgrade to
/// `partial` and the obstruction's limitation must be related to it with a
/// non-empty, non-dangling source trace. `changed_structure` must downgrade
/// -- and the same limitation must relate to it -- only when the excluded
/// path is one Git's own diff also reports changed between base and target;
/// otherwise `changed_structure` must stay `complete` and unrelated.
fn assert_excluded_entry_capability_trace(
    result: &reviewgraphen_ingest::IngestResult,
    kind: IngestionObstructionKind,
    path: &str,
    expect_changed_structure_related: bool,
) {
    let obstruction = result
        .extraction_report
        .obstructions
        .iter()
        .find(|obstruction| obstruction.kind == kind && obstruction.paths.contains(path))
        .unwrap_or_else(|| panic!("expected a `{kind:?}` obstruction for `{path}`"));

    assert_eq!(
        result.extraction_report.capabilities["git_snapshot"],
        CapabilityState::Partial,
        "an excluded tree entry must always downgrade git_snapshot"
    );

    let known_ids = result.program_space.known_ids();
    let extraction = result.program_space.extraction();
    let limitation = extraction
        .limitations
        .iter()
        .find(|limitation| limitation.id == obstruction.id)
        .expect("the obstruction's limitation exists in the ProgramSpace extraction");

    assert!(
        limitation.related_capabilities.contains("git_snapshot"),
        "the excluded-entry limitation must be related to git_snapshot"
    );
    assert!(
        !limitation.source_ids.is_empty(),
        "the excluded-entry limitation must have a non-empty source trace"
    );
    for source_id in &limitation.source_ids {
        assert!(
            known_ids.contains(source_id),
            "excluded-entry limitation has a dangling source `{source_id}`"
        );
    }

    if expect_changed_structure_related {
        assert_eq!(
            result.extraction_report.capabilities["changed_structure"],
            CapabilityState::Partial,
            "an excluded entry Git's own diff also reports changed must downgrade changed_structure"
        );
        assert!(
            limitation
                .related_capabilities
                .contains("changed_structure"),
            "an excluded entry that also changed must relate its limitation to changed_structure"
        );
    } else {
        assert_eq!(
            result.extraction_report.capabilities["changed_structure"],
            CapabilityState::Complete,
            "an excluded entry outside the diff must not downgrade changed_structure"
        );
        assert!(
            !limitation
                .related_capabilities
                .contains("changed_structure"),
            "an excluded entry outside the diff must not relate its limitation to changed_structure"
        );
    }
}

#[test]
fn tracked_symlinks_are_not_followed_and_remain_explicitly_excluded() {
    use std::os::unix::fs::symlink;

    let repository = fixture_repository();
    symlink("/etc/passwd", repository.repository.join("escaped-link")).expect("test symlink");
    git(&repository.repository, ["add", "escaped-link"]);
    git(
        &repository.repository,
        ["commit", "--quiet", "-m", "symlink"],
    );
    let target = git_stdout(&repository.repository, ["rev-parse", "HEAD"]);
    let mut request = repository.request();
    request.target_revision = target;
    let result = ingest(&request).expect("symlink becomes an explicit exclusion");
    assert!(
        !result
            .program_space
            .artifacts()
            .iter()
            .any(|artifact| artifact.label == "escaped-link")
    );
    assert!(
        result
            .extraction_report
            .obstructions
            .iter()
            .any(|obstruction| {
                obstruction.kind == IngestionObstructionKind::RegionExcluded
                    && obstruction.paths.contains("escaped-link")
            })
    );
    // The symlink was added by the very commit being diffed against the
    // fixture's original base, so it is also a Git diff entry.
    assert_excluded_entry_capability_trace(
        &result,
        IngestionObstructionKind::RegionExcluded,
        "escaped-link",
        true,
    );
}

#[test]
fn excluded_entry_unchanged_between_base_and_target_does_not_downgrade_changed_structure() {
    use std::os::unix::fs::symlink;

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
        "[package]\nname = \"excluded-entry-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    write(&repository, "src/lib.rs", "pub fn entry() {}\n");
    symlink("/etc/passwd", repository.join("stable-link")).expect("test symlink");
    git(&repository, ["add", "."]);
    git(&repository, ["commit", "--quiet", "-m", "base"]);
    let base = git_stdout(&repository, ["rev-parse", "HEAD"]);
    write(
        &repository,
        "src/lib.rs",
        "pub fn entry() {\n    let _ = 1;\n}\n",
    );
    git(&repository, ["add", "."]);
    git(&repository, ["commit", "--quiet", "-m", "target"]);
    let target = git_stdout(&repository, ["rev-parse", "HEAD"]);

    let request = IngestRequest::new(
        workspace.path(),
        &repository,
        "reviewgraphen.test/excluded-entry-fixture".to_owned(),
        &base,
        &target,
    );
    let result = ingest(&request).expect("M2 ingest succeeds");

    assert!(
        result
            .extraction_report
            .obstructions
            .iter()
            .any(|obstruction| {
                obstruction.kind == IngestionObstructionKind::RegionExcluded
                    && obstruction.paths.contains("stable-link")
            }),
        "the untouched symlink must still be retained as an excluded obstruction"
    );
    // `stable-link` exists unchanged in both base and target, so it never
    // appears in `git diff --name-status base target`.
    assert_excluded_entry_capability_trace(
        &result,
        IngestionObstructionKind::RegionExcluded,
        "stable-link",
        false,
    );
}

#[test]
fn git_submodule_entry_is_retained_as_a_typed_obstruction_without_aborting_ingestion() {
    let repository = fixture_repository();

    let submodule_source = tempfile::tempdir().expect("submodule source repository");
    git(submodule_source.path(), ["init", "--quiet"]);
    git(
        submodule_source.path(),
        ["config", "user.email", "reviewgraphen@example.test"],
    );
    git(
        submodule_source.path(),
        ["config", "user.name", "ReviewGraphen test"],
    );
    write(submodule_source.path(), "README.md", "submodule fixture\n");
    git(submodule_source.path(), ["add", "."]);
    git(
        submodule_source.path(),
        ["commit", "--quiet", "-m", "submodule init"],
    );
    let submodule_path = submodule_source
        .path()
        .to_str()
        .expect("submodule path is UTF-8");
    git(
        &repository.repository,
        [
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            submodule_path,
            "sub",
        ],
    );
    git(&repository.repository, ["add", "-A"]);
    git(
        &repository.repository,
        ["commit", "--quiet", "-m", "add submodule"],
    );
    let target = git_stdout(&repository.repository, ["rev-parse", "HEAD"]);
    let mut request = repository.request();
    request.target_revision = target;
    let result = ingest(&request).expect("a Git submodule entry must not abort ingestion");

    assert!(
        result
            .extraction_report
            .obstructions
            .iter()
            .any(|obstruction| {
                obstruction.kind == IngestionObstructionKind::UnsupportedInput
                    && obstruction.paths.contains("sub")
            }),
        "the submodule entry must be retained as a typed `unsupported_input` obstruction"
    );
    // The submodule was added by the very commit being diffed against the
    // fixture's original base, so it is also a Git diff entry.
    assert_excluded_entry_capability_trace(
        &result,
        IngestionObstructionKind::UnsupportedInput,
        "sub",
        true,
    );
    assert!(
        !result
            .program_space
            .artifacts()
            .iter()
            .any(|artifact| artifact.label == "sub"),
        "a submodule entry must never become an accepted file artifact"
    );

    let git_adapter = result
        .extraction_report
        .adapters
        .iter()
        .find(|adapter| adapter.id == "reviewgraphen.ingest.git")
        .expect("git adapter report exists");
    assert_eq!(
        git_adapter.excluded,
        Some(1),
        "the git adapter's excluded count must include the submodule entry"
    );
    assert_eq!(git_adapter.failed, Some(0));
    assert!(
        git_adapter.total.expect("total is present")
            > git_adapter.parsed.expect("parsed is present"),
        "discovered (total) must exceed accepted (parsed) once an entry is excluded"
    );
}

/// The default `IngestConfig.cargo_admission` is `Disabled`: even though
/// this test process's own real, valid `cargo` sits on `PATH` and the
/// fixture repository has a perfectly ordinary root `Cargo.toml`, an
/// `IngestRequest` built without explicitly admitting an executable (via
/// `IngestRequest::new`, never `TempGitRepository::request()`, which opts
/// into `trusted_test_cargo_admission()`) must never run Cargo metadata --
/// proving there is no automatic `PATH`/`rustup` fallback left anywhere in
/// production `ingest()`.
#[test]
fn default_disabled_admission_never_runs_cargo_metadata_even_with_a_real_cargo_on_path() {
    let repository = fixture_repository();
    let request = IngestRequest::new(
        repository.workspace.path(),
        &repository.repository,
        repository.identity.clone(),
        &repository.base,
        &repository.target,
    );
    assert!(matches!(
        request.config.cargo_admission,
        CargoToolAdmission::Disabled
    ));
    let result = ingest(&request).expect("M2 ingest still succeeds for every non-Cargo fact");

    assert_eq!(
        result.extraction_report.capabilities["cargo_metadata"],
        CapabilityState::Missing,
        "Disabled admission must report cargo_metadata as missing even though a real cargo \
         is available on this host's PATH"
    );
    assert!(
        result
            .extraction_report
            .obstructions
            .iter()
            .any(|obstruction| {
                obstruction.kind == IngestionObstructionKind::CargoMetadataUnavailable
            }),
        "the Disabled precondition must be retained as a typed obstruction, not silently absorbed"
    );
    assert!(
        !result
            .program_space
            .artifacts()
            .iter()
            .any(|artifact| artifact.kind == "package"),
        "no package/dependency facts may be accepted without an explicitly admitted executable"
    );
}

#[test]
fn unsafe_cargo_path_dependency_is_not_followed() {
    let repository = fixture_repository();
    write(
        &repository.repository,
        "Cargo.toml",
        "[package]\nname = \"m2-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\noutside = { path = \"../outside\" }\n",
    );
    git(&repository.repository, ["add", "Cargo.toml"]);
    git(
        &repository.repository,
        ["commit", "--quiet", "-m", "path dependency"],
    );
    let target = git_stdout(&repository.repository, ["rev-parse", "HEAD"]);
    let mut request = repository.request_with_trusted_cargo();
    request.target_revision = target;
    let result = ingest(&request).expect("unsafe metadata is retained as unknown");
    assert_eq!(
        result.extraction_report.capabilities["cargo_metadata"],
        CapabilityState::Missing
    );
    assert!(
        result
            .extraction_report
            .obstructions
            .iter()
            .any(|obstruction| {
                obstruction.kind == IngestionObstructionKind::CargoMetadataUnavailable
            })
    );
}

#[test]
fn package_workspace_pointing_outside_the_snapshot_blocks_cargo_metadata() {
    let repository = fixture_repository();
    write(
        &repository.repository,
        "Cargo.toml",
        "[package]\nname = \"m2-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\nworkspace = \"../outside-workspace\"\n",
    );
    git(&repository.repository, ["add", "Cargo.toml"]);
    git(
        &repository.repository,
        ["commit", "--quiet", "-m", "escaping package.workspace"],
    );
    let target = git_stdout(&repository.repository, ["rev-parse", "HEAD"]);
    let mut request = repository.request_with_trusted_cargo();
    request.target_revision = target;
    let result = ingest(&request).expect("an escaping package.workspace is retained as unknown");
    assert_eq!(
        result.extraction_report.capabilities["cargo_metadata"],
        CapabilityState::Missing
    );
    assert!(
        result
            .extraction_report
            .obstructions
            .iter()
            .any(|obstruction| {
                obstruction.kind == IngestionObstructionKind::CargoMetadataUnavailable
            })
    );
}

#[test]
fn malformed_cargo_manifest_blocks_cargo_metadata_instead_of_being_treated_as_safe() {
    let repository = fixture_repository();
    write(&repository.repository, "Cargo.toml", "[package]\nname = \n");
    git(&repository.repository, ["add", "Cargo.toml"]);
    git(
        &repository.repository,
        ["commit", "--quiet", "-m", "malformed manifest"],
    );
    let target = git_stdout(&repository.repository, ["rev-parse", "HEAD"]);
    let mut request = repository.request_with_trusted_cargo();
    request.target_revision = target;
    let result = ingest(&request).expect("other M2 facts still ingest despite a bad manifest");
    assert_eq!(
        result.extraction_report.capabilities["cargo_metadata"],
        CapabilityState::Missing,
        "a manifest that cannot be parsed must never be silently treated as safe to run \
         Cargo metadata against"
    );
}

#[test]
fn sibling_workspace_path_dependency_within_the_snapshot_does_not_block_cargo_metadata() {
    let workspace = tempfile::tempdir().expect("temporary workspace");
    let repository = workspace.path().join("repo");
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
        "[workspace]\nmembers = [\"member-a\", \"member-b\"]\nresolver = \"2\"\n",
    );
    write(
        &repository,
        "member-a/Cargo.toml",
        "[package]\nname = \"member-a\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    write(&repository, "member-a/src/lib.rs", "pub fn hello() {}\n");
    write(
        &repository,
        "member-b/Cargo.toml",
        "[package]\nname = \"member-b\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nmember-a = { path = \"../member-a\" }\n",
    );
    write(&repository, "member-b/src/lib.rs", "pub fn hi() {}\n");
    git(&repository, ["add", "."]);
    git(&repository, ["commit", "--quiet", "-m", "workspace"]);
    let target = git_stdout(&repository, ["rev-parse", "HEAD"]);
    let mut request = IngestRequest::new(
        workspace.path(),
        &repository,
        "reviewgraphen.test/sibling-workspace",
        &target,
        &target,
    );
    request.config.cargo_admission = trusted_test_cargo_admission();
    let result = ingest(&request).expect("M2 ingest succeeds");
    assert_eq!(
        result.extraction_report.capabilities["cargo_metadata"],
        CapabilityState::Complete,
        "a path dependency that stays within the bounded snapshot must not block Cargo metadata"
    );
    assert!(
        !result
            .extraction_report
            .obstructions
            .iter()
            .any(|obstruction| {
                obstruction.kind == IngestionObstructionKind::CargoMetadataUnavailable
            })
    );
    let package_labels = result
        .program_space
        .artifacts()
        .iter()
        .filter(|artifact| artifact.kind == "package")
        .map(|artifact| artifact.label.as_str())
        .collect::<BTreeSet<_>>();
    assert!(package_labels.contains("member-a"));
    assert!(package_labels.contains("member-b"));

    // P0-3: a workspace-internal path dependency must resolve to the
    // already-accepted `member-a` package artifact, never a fabricated
    // `external-package:*` stub.
    let member_a = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| artifact.kind == "package" && artifact.label == "member-a")
        .expect("member-a package artifact exists");
    let member_b = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| artifact.kind == "package" && artifact.label == "member-b")
        .expect("member-b package artifact exists");
    assert_eq!(
        result
            .program_space
            .artifacts()
            .iter()
            .filter(|artifact| artifact.kind == "package" && artifact.label == "member-a")
            .count(),
        1,
        "exactly one package artifact must exist for member-a, no external stub alongside it"
    );
    assert_ne!(
        member_a.attributes.get("external"),
        Some(&serde_json::Value::Bool(true)),
        "an internal workspace-member path dependency must never be marked external"
    );
    assert!(
        result.program_space.relations().iter().any(|relation| {
            relation.kind == "depends_on"
                && relation.source_id == member_b.id
                && relation.target_ids.contains(&member_a.id)
        }),
        "member-b must depend_on the accepted member-a package artifact directly"
    );
}

#[test]
fn external_registry_dependency_is_still_marked_external() {
    let repository = fixture_repository();
    write(
        &repository.repository,
        "Cargo.toml",
        "[package]\nname = \"m2-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nonce_cell = \"1\"\n",
    );
    git(&repository.repository, ["add", "Cargo.toml"]);
    git(
        &repository.repository,
        ["commit", "--quiet", "-m", "add external dependency"],
    );
    let target = git_stdout(&repository.repository, ["rev-parse", "HEAD"]);
    let mut request = repository.request_with_trusted_cargo();
    request.target_revision = target;
    let result = ingest(&request).expect("M2 ingest succeeds");

    assert_eq!(
        result.extraction_report.capabilities["cargo_metadata"],
        CapabilityState::Complete
    );
    let external_package = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| artifact.kind == "package" && artifact.label == "once_cell")
        .expect("once_cell external package artifact exists");
    assert_eq!(
        external_package.attributes.get("external"),
        Some(&serde_json::Value::Bool(true)),
        "a genuine registry dependency must still be marked external"
    );
    let m2_fixture = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| artifact.kind == "package" && artifact.label == "m2-fixture")
        .expect("m2-fixture package artifact exists");
    assert!(
        result.program_space.relations().iter().any(|relation| {
            relation.kind == "depends_on"
                && relation.source_id == m2_fixture.id
                && relation.target_ids.contains(&external_package.id)
        }),
        "m2-fixture must depend_on the external once_cell package artifact"
    );
}

#[test]
fn stable_repository_identity_is_independent_of_the_local_clone_path() {
    let original = fixture_repository();
    let original_result =
        ingest(&original.request_with_trusted_cargo()).expect("original ingest succeeds");

    let cloned_workspace = tempfile::tempdir().expect("second temporary workspace");
    let cloned_repository = cloned_workspace.path().join("clone");
    copy_dir_recursive(&original.repository, &cloned_repository);
    let mut cloned_request = IngestRequest::new(
        cloned_workspace.path(),
        &cloned_repository,
        original.identity.clone(),
        &original.base,
        &original.target,
    );
    cloned_request.config.cargo_admission = trusted_test_cargo_admission();
    let cloned_result = ingest(&cloned_request).expect("cloned ingest succeeds");

    assert_eq!(
        original_result.program_space.repository_id(),
        cloned_result.program_space.repository_id(),
        "repository_id must not depend on the absolute clone path"
    );
    assert_eq!(
        original_result.program_space.snapshot_id(),
        cloned_result.program_space.snapshot_id(),
        "snapshot_id must not depend on the absolute clone path"
    );
    let original_ids = original_result
        .program_space
        .artifacts()
        .iter()
        .map(|artifact| artifact.id.clone())
        .collect::<BTreeSet<_>>();
    let cloned_ids = cloned_result
        .program_space
        .artifacts()
        .iter()
        .map(|artifact| artifact.id.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        original_ids, cloned_ids,
        "artifact IDs must be identical across clones of the same repository"
    );
    assert_eq!(
        original_result.extraction_report.adapter_set_hash,
        cloned_result.extraction_report.adapter_set_hash,
        "adapter_set_hash binds the real git/cargo/syn tool versions, which are host-wide, \
         not clone-path-specific, so it must not depend on the absolute clone path either"
    );
}

#[test]
fn different_limits_produce_a_different_adapter_set_hash() {
    let repository = fixture_repository();
    let mut request = repository.request();
    let default_result = ingest(&request).expect("default-limits ingest succeeds");

    request.config.limits.max_file_bytes += 1;
    let widened_result = ingest(&request).expect("widened-limits ingest succeeds");

    assert_ne!(
        default_result.extraction_report.adapter_set_hash,
        widened_result.extraction_report.adapter_set_hash,
        "a limits change that could affect what gets parsed must change adapter_set_hash"
    );
    // The limits change alone must not perturb the target tree's own
    // identity: only the extractor-set tuple/hash a caller can use to tell
    // two runs' configurations apart should differ.
    assert_eq!(
        default_result.program_space.snapshot_id(),
        widened_result.program_space.snapshot_id()
    );
}

/// The bounded Git command policy (see `git::git_command_policy_fingerprint`
/// in the crate) must make every allow-listed `git` call deterministic
/// regardless of the cloned repository's own `.git/config`: two clones of
/// the exact same content, given two *different* adversarial configs (one
/// even declaring an external diff helper, both forcing ANSI-colored diff
/// output), must still produce identical accepted facts, snapshot identity,
/// and `adapter_set_hash` -- and the declared external diff helper must
/// never actually run.
#[test]
fn divergent_repo_config_does_not_change_accepted_facts_or_hashes() {
    let baseline = fixture_repository();
    let baseline_result =
        ingest(&baseline.request_with_trusted_cargo()).expect("baseline ingest succeeds");

    let external_diff_sentinel = baseline
        .workspace
        .path()
        .join("external-diff-invoked.marker");

    // Clone A: config that would visibly change diff/rename output (a
    // different diff algorithm, renames disabled, and a renameLimit so low
    // it would normally suppress detection) if the command policy did not
    // override it.
    let clone_a_workspace = tempfile::tempdir().expect("clone A workspace");
    let clone_a_repository = clone_a_workspace.path().join("clone-a");
    copy_dir_recursive(&baseline.repository, &clone_a_repository);
    git(
        &clone_a_repository,
        ["config", "diff.algorithm", "patience"],
    );
    git(&clone_a_repository, ["config", "diff.renames", "false"]);
    git(&clone_a_repository, ["config", "diff.renameLimit", "1"]);
    git(&clone_a_repository, ["config", "core.pager", "false"]);
    git(&clone_a_repository, ["config", "color.ui", "always"]);
    git(&clone_a_repository, ["config", "color.diff", "always"]);

    // Clone B: a *different* adversarial config, additionally declaring an
    // external diff helper that writes a sentinel file if it is ever
    // actually invoked.
    let clone_b_workspace = tempfile::tempdir().expect("clone B workspace");
    let clone_b_repository = clone_b_workspace.path().join("clone-b");
    copy_dir_recursive(&baseline.repository, &clone_b_repository);
    git(
        &clone_b_repository,
        ["config", "diff.algorithm", "histogram"],
    );
    git(&clone_b_repository, ["config", "diff.renames", "true"]);
    git(&clone_b_repository, ["config", "diff.renameLimit", "0"]);
    git(&clone_b_repository, ["config", "color.ui", "always"]);
    git(&clone_b_repository, ["config", "color.diff", "always"]);
    let external_diff_script = clone_b_workspace.path().join("external-diff.sh");
    fs::write(
        &external_diff_script,
        format!(
            "#!/bin/sh\ntouch \"{}\"\nexit 1\n",
            external_diff_sentinel.display()
        ),
    )
    .expect("write external diff sentinel script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&external_diff_script)
            .expect("sentinel script metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&external_diff_script, permissions)
            .expect("make sentinel script executable");
    }
    git(
        &clone_b_repository,
        [
            "config",
            "diff.external",
            external_diff_script
                .to_str()
                .expect("sentinel script path is UTF-8"),
        ],
    );

    let mut request_a = IngestRequest::new(
        clone_a_workspace.path(),
        &clone_a_repository,
        baseline.identity.clone(),
        &baseline.base,
        &baseline.target,
    );
    request_a.config.cargo_admission = trusted_test_cargo_admission();
    let mut request_b = IngestRequest::new(
        clone_b_workspace.path(),
        &clone_b_repository,
        baseline.identity.clone(),
        &baseline.base,
        &baseline.target,
    );
    request_b.config.cargo_admission = trusted_test_cargo_admission();
    let result_a = ingest(&request_a).expect("clone A ingest succeeds despite hostile config");
    let result_b = ingest(&request_b).expect("clone B ingest succeeds despite hostile config");

    assert!(
        !external_diff_sentinel.exists(),
        "a repo-declared external diff helper must never be invoked by the bounded Git \
         command policy"
    );

    fn fact_ids(result: &IngestResult) -> BTreeSet<String> {
        result
            .program_space
            .artifacts()
            .iter()
            .map(|artifact| artifact.id.to_string())
            .collect()
    }
    assert_eq!(
        fact_ids(&baseline_result),
        fact_ids(&result_a),
        "divergent repo diff/rename config must not change accepted fact IDs, including \
         `change` facts"
    );
    assert_eq!(fact_ids(&baseline_result), fact_ids(&result_b));

    assert_eq!(
        baseline_result.program_space.snapshot_id(),
        result_a.program_space.snapshot_id(),
        "divergent repo config must not change snapshot_id"
    );
    assert_eq!(
        baseline_result.program_space.snapshot_id(),
        result_b.program_space.snapshot_id()
    );
    assert_eq!(
        baseline_result.extraction_report.adapter_set_hash,
        result_a.extraction_report.adapter_set_hash,
        "divergent repo config must not change adapter_set_hash"
    );
    assert_eq!(
        baseline_result.extraction_report.adapter_set_hash,
        result_b.extraction_report.adapter_set_hash
    );

    // Direct proof that `changed_lines` itself -- not just the fact IDs and
    // hashes it happens to feed into -- survived the forced `color.ui`/
    // `color.diff` config unharmed: `src/api.rs`'s `worker` function only
    // carries a `changed_by` edge when its location overlaps a real, non-
    // empty changed-line range. Without `--no-color` on the `ChangedLines`
    // diff, the ANSI-wrapped `@@ ...` hunk header would silently stop
    // matching `changed_lines`'s `line.strip_prefix("@@ ")` parse, emptying
    // the fact and dropping exactly this edge in both clones.
    let worker_has_changed_by_edge = |result: &IngestResult| {
        let worker = result
            .program_space
            .artifacts()
            .iter()
            .find(|artifact| artifact.kind == "function" && artifact.label == "crate::api::worker")
            .expect("crate::api::worker function artifact exists");
        result
            .program_space
            .relations()
            .iter()
            .any(|relation| relation.kind == "changed_by" && relation.source_id == worker.id)
    };
    assert!(
        worker_has_changed_by_edge(&baseline_result),
        "control: the baseline clone (no adversarial config) must have a changed_by edge"
    );
    assert!(
        worker_has_changed_by_edge(&result_a),
        "clone A's forced `color.ui=always`/`color.diff=always` must not empty out \
         changed_lines -- the changed_by edge must survive identically to the baseline"
    );
    assert!(
        worker_has_changed_by_edge(&result_b),
        "clone B's forced `color.ui=always`/`color.diff=always` must not empty out \
         changed_lines -- the changed_by edge must survive identically to the baseline"
    );
}

/// A clone-local `*.rs binary` override in `.git/info/attributes` (never
/// tracked, so it can differ freely between clones of the identical
/// history) must not empty out `changed_lines`: without `--text` on the
/// `ChangedLines` diff, Git would report "Binary files ... differ" with no
/// `@@` hunks at all for a path it believes is binary, silently dropping
/// the line-level changed-structure fact for a file that genuinely changed.
#[test]
fn clone_local_binary_attribute_override_does_not_change_changed_lines_or_hashes() {
    let baseline = fixture_repository();
    let baseline_result =
        ingest(&baseline.request_with_trusted_cargo()).expect("baseline ingest succeeds");

    let clone_workspace = tempfile::tempdir().expect("clone workspace");
    let clone_repository = clone_workspace.path().join("clone");
    copy_dir_recursive(&baseline.repository, &clone_repository);
    fs::create_dir_all(clone_repository.join(".git/info")).expect("git info directory");
    fs::write(
        clone_repository.join(".git/info/attributes"),
        "*.rs binary\n",
    )
    .expect("write clone-local binary attribute override");

    let mut request = IngestRequest::new(
        clone_workspace.path(),
        &clone_repository,
        baseline.identity.clone(),
        &baseline.base,
        &baseline.target,
    );
    request.config.cargo_admission = trusted_test_cargo_admission();
    let result =
        ingest(&request).expect("clone ingest succeeds despite a binary attribute override");

    fn fact_ids(result: &IngestResult) -> BTreeSet<String> {
        result
            .program_space
            .artifacts()
            .iter()
            .map(|artifact| artifact.id.to_string())
            .collect()
    }
    assert_eq!(
        fact_ids(&baseline_result),
        fact_ids(&result),
        "a clone-local `*.rs binary` override must not change accepted fact IDs"
    );
    assert_eq!(
        baseline_result.extraction_report.adapter_set_hash,
        result.extraction_report.adapter_set_hash,
        "a clone-local `*.rs binary` override must not change adapter_set_hash"
    );

    // The strongest, most direct proof: `src/api.rs`'s `worker` function
    // genuinely changed between base and target, so it must carry a
    // `changed_by` edge in both -- a binary misclassification silently
    // emptying `changed_lines` would still leave the *file*-level edge
    // (files always link to their own change entry regardless of location)
    // but drop exactly this function-level one, since it only exists when
    // the function's location overlaps a real changed line.
    let worker_has_changed_by_edge = |result: &IngestResult| {
        let worker = result
            .program_space
            .artifacts()
            .iter()
            .find(|artifact| artifact.kind == "function" && artifact.label == "crate::api::worker")
            .expect("crate::api::worker function artifact exists");
        result
            .program_space
            .relations()
            .iter()
            .any(|relation| relation.kind == "changed_by" && relation.source_id == worker.id)
    };
    assert!(
        worker_has_changed_by_edge(&baseline_result),
        "control: the baseline clone (no attribute override) must have a changed_by edge"
    );
    assert!(
        worker_has_changed_by_edge(&result),
        "the `*.rs binary` clone must still have the same changed_by edge -- changed_lines \
         must not have been silently emptied by the binary classification"
    );
}

/// A `refs/replace/*` ref (`git replace`) transparently substitutes a
/// different object wherever the original is read, and is a purely
/// clone-local ref -- never part of the tracked history, so two clones of
/// the exact same commit can disagree in which (if any) replacement refs
/// they carry. Without `--no-replace-objects` on every allow-listed `git`
/// call, a clone carrying a replacement for the requested target revision
/// would silently ingest the *replacement*'s content instead of the real
/// target's.
#[test]
fn a_replacement_ref_present_in_only_one_clone_does_not_change_accepted_facts_or_hashes() {
    let baseline = fixture_repository();

    // A decoy commit with content unrelated to the real target, used only
    // as a replacement object -- if replacement were honored, resolving
    // `target` would silently yield this tree/content instead.
    write(
        &baseline.repository,
        "src/api.rs",
        "pub fn decoy_worker() {\n    let _decoy = true;\n}\n",
    );
    git(&baseline.repository, ["add", "."]);
    git(
        &baseline.repository,
        [
            "commit",
            "--quiet",
            "-m",
            "decoy commit, never itself requested",
        ],
    );
    let decoy = git_stdout(&baseline.repository, ["rev-parse", "HEAD"]);

    let baseline_result =
        ingest(&baseline.request_with_trusted_cargo()).expect("baseline ingest succeeds");

    let clone_workspace = tempfile::tempdir().expect("clone workspace");
    let clone_repository = clone_workspace.path().join("clone");
    copy_dir_recursive(&baseline.repository, &clone_repository);
    git(&clone_repository, ["replace", &baseline.target, &decoy]);

    let mut request = IngestRequest::new(
        clone_workspace.path(),
        &clone_repository,
        baseline.identity.clone(),
        &baseline.base,
        &baseline.target,
    );
    request.config.cargo_admission = trusted_test_cargo_admission();
    let result =
        ingest(&request).expect("clone ingest succeeds despite a clone-local replacement ref");

    fn fact_ids(result: &IngestResult) -> BTreeSet<String> {
        result
            .program_space
            .artifacts()
            .iter()
            .map(|artifact| artifact.id.to_string())
            .collect()
    }
    assert_eq!(
        fact_ids(&baseline_result),
        fact_ids(&result),
        "a clone-local replacement ref for the requested target revision must not change \
         accepted fact IDs -- the real target's content must be read, not the replacement's"
    );
    assert_eq!(
        baseline_result.program_space.snapshot_id(),
        result.program_space.snapshot_id(),
        "a clone-local replacement ref must not change snapshot_id"
    );
    assert_eq!(
        baseline_result.extraction_report.adapter_set_hash,
        result.extraction_report.adapter_set_hash,
        "a clone-local replacement ref must not change adapter_set_hash"
    );

    // The decoy's own distinctive function must never appear as an
    // accepted fact -- proof the replacement's content was never actually
    // read, not merely that some other ID happened to still match.
    assert!(
        !result
            .program_space
            .artifacts()
            .iter()
            .any(|artifact| artifact.label.contains("decoy_worker")),
        "the decoy replacement's content must never be accepted as a fact of the real target"
    );
}

#[test]
fn snapshot_and_fact_ids_are_independent_of_the_diff_base_revision() {
    let repository = fixture_repository();

    let mut request_a = repository.request();
    request_a.base_revision.clone_from(&repository.base);
    let result_a = ingest(&request_a).expect("ingest with the original base succeeds");

    let mut request_b = repository.request();
    // Diffing the target against itself is a legal, different base revision
    // for the very same target tree.
    request_b.base_revision.clone_from(&repository.target);
    let result_b = ingest(&request_b).expect("ingest with an alternate base succeeds");

    assert_eq!(
        result_a.program_space.snapshot_id(),
        result_b.program_space.snapshot_id(),
        "snapshot_id must not depend on base_revision"
    );
    assert_eq!(
        result_a.program_space.repository_id(),
        result_b.program_space.repository_id()
    );

    let worker_id = |result: &reviewgraphen_ingest::IngestResult| {
        result
            .program_space
            .artifacts()
            .iter()
            .find(|artifact| artifact.kind == "function" && artifact.label == "crate::api::worker")
            .expect("worker function artifact exists")
            .id
            .clone()
    };
    assert_eq!(
        worker_id(&result_a),
        worker_id(&result_b),
        "a non-changed-structure fact's ID must not depend on base_revision"
    );

    let call_relation_ids = |result: &reviewgraphen_ingest::IngestResult| {
        result
            .program_space
            .relations()
            .iter()
            .filter(|relation| relation.kind == "calls")
            .map(|relation| relation.id.clone())
            .collect::<BTreeSet<_>>()
    };
    assert_eq!(
        call_relation_ids(&result_a),
        call_relation_ids(&result_b),
        "`calls` relation IDs must not depend on base_revision"
    );

    // Under `request_a`'s base, `src/api.rs` (and `worker` within it)
    // actually differ from the target; under `request_b`'s self-diff base
    // they do not. A same-ID artifact that still carried a base-relative
    // `changed`/`changed_lines` attribute would diverge right here even
    // though every ID compared above is already identical, so this is the
    // oracle for the comparison-specific-attributes bug, not just the
    // weaker "IDs match" check.
    let non_change_artifacts_by_id = |result: &reviewgraphen_ingest::IngestResult| {
        result
            .program_space
            .artifacts()
            .iter()
            .filter(|artifact| artifact.kind != "custom")
            .map(|artifact| (artifact.id.clone(), artifact.clone()))
            .collect::<BTreeMap<_, _>>()
    };
    assert_eq!(
        non_change_artifacts_by_id(&result_a),
        non_change_artifacts_by_id(&result_b),
        "every non-change artifact's canonical body (attributes, location, content hash, \
         provenance) must be byte-identical across different diff bases for the same target"
    );

    let non_changed_by_relations_by_id = |result: &reviewgraphen_ingest::IngestResult| {
        result
            .program_space
            .relations()
            .iter()
            .filter(|relation| relation.kind != "changed_by")
            .map(|relation| (relation.id.clone(), relation.clone()))
            .collect::<BTreeMap<_, _>>()
    };
    assert_eq!(
        non_changed_by_relations_by_id(&result_a),
        non_changed_by_relations_by_id(&result_b),
        "every non-`changed_by` relation must be byte-identical across different diff bases"
    );

    let changed_by_relation_count = |result: &reviewgraphen_ingest::IngestResult| {
        result
            .program_space
            .relations()
            .iter()
            .filter(|relation| relation.kind == "changed_by")
            .count()
    };
    assert!(
        changed_by_relation_count(&result_a) > 0,
        "the original base actually changed src/api.rs, so at least one `changed_by` \
         relation is expected"
    );
    assert_eq!(
        changed_by_relation_count(&result_b),
        0,
        "the self-diff base has no changes, so no `changed_by` relation is expected"
    );
}

#[test]
fn capability_and_limitation_source_ids_resolve_within_the_program_space() {
    let repository = fixture_repository();
    let result = ingest(&repository.request()).expect("M2 ingest succeeds");
    let known_ids = result.program_space.known_ids();
    let extraction = result.program_space.extraction();

    for (capability, declaration) in &extraction.capabilities {
        assert!(
            !declaration.source_ids.is_empty(),
            "capability `{capability}` must have a non-empty source trace"
        );
        for source_id in &declaration.source_ids {
            assert!(
                known_ids.contains(source_id),
                "capability `{capability}` has a dangling source `{source_id}`"
            );
        }
    }
    for limitation in &extraction.limitations {
        assert!(
            !limitation.source_ids.is_empty(),
            "limitation `{}` must have a non-empty source trace",
            limitation.id
        );
        for source_id in &limitation.source_ids {
            assert!(
                known_ids.contains(source_id),
                "limitation `{}` has a dangling source `{source_id}`",
                limitation.id
            );
        }
    }
}

#[test]
fn m2_output_feeds_m1_synthesis_without_a_migration_gap() {
    let repository = fixture_repository();
    let result = ingest(&repository.request()).expect("M2 ingest succeeds");
    let bundle = MvpRulePack::synthesize(&result.program_space)
        .expect("a native v2 ProgramSpace synthesizes without an explicit migration step");
    assert_eq!(
        bundle.universe().snapshot_id(),
        result.program_space.snapshot_id()
    );
    assert!(
        !bundle.obligations().is_empty(),
        "M2's bounded capabilities should synthesize at least one M1 obligation"
    );
}

#[test]
fn capability_gap_obligation_reason_and_qualification_trace_are_verified() {
    let repository = fixture_repository();
    let result = ingest(&repository.request()).expect("M2 ingest succeeds");
    let bundle = MvpRulePack::synthesize(&result.program_space)
        .expect("a native v2 ProgramSpace synthesizes without an explicit migration step");

    // M2 rule packs can synthesize more than one capability-gap obligation
    // (one per rule with an unmet capability requirement, including rules
    // requiring a capability M2 never declares at all, which is a
    // legitimately different -- `capability_undeclared` -- reason). This
    // test is specifically about M2's *own* permanently-partial
    // capabilities, so it must select the gap obligation that actually
    // names one of them, not merely the first gap obligation in sort order.
    const PERMANENTLY_PARTIAL: [&str; 5] = [
        "direct_calls",
        "imports",
        "module_dependencies",
        "test_mapping",
        "state_writes",
    ];
    let gap_obligation = bundle
        .obligations()
        .iter()
        .find(|obligation| {
            obligation.version().rule() == "capability_gap.origin_rule@1"
                && obligation.property_id() == "reviewgraphen.capability_gap"
                && obligation.applicability_reasons().iter().any(|reason| {
                    PERMANENTLY_PARTIAL
                        .iter()
                        .any(|capability| *reason == format!("capability_partial:{capability}"))
                })
        })
        .expect(
            "M2's permanently-partial capabilities must synthesize at least one \
             capability-gap obligation naming one of them",
        );
    assert_eq!(
        gap_obligation.applicability_status(),
        "unknown",
        "a capability-gap obligation's applicability status must be `unknown`"
    );

    let reasons = gap_obligation.applicability_reasons();
    assert!(
        reasons
            .iter()
            .any(|reason| reason.starts_with("origin_rule:")),
        "reasons must name the origin rule the gap was raised for: {reasons:?}"
    );
    let partial_reasons = reasons
        .iter()
        .filter(|reason| {
            PERMANENTLY_PARTIAL
                .iter()
                .any(|capability| *reason == &format!("capability_partial:{capability}"))
        })
        .collect::<Vec<_>>();
    assert!(
        !partial_reasons.is_empty(),
        "reasons must distinguish `capability_partial:<name>` for one of M2's \
         permanently-partial capabilities, not a collapsed generic tag: {reasons:?}"
    );

    let qualification_ids = gap_obligation.qualification_ids();
    assert!(
        !qualification_ids.is_empty(),
        "a capability-gap obligation must carry a qualification-ID trace to its \
         justifying limitation(s), not an untraceable `unknown` applicability"
    );
    let limitations_by_id = result
        .program_space
        .extraction()
        .limitations
        .iter()
        .map(|limitation| (&limitation.id, limitation))
        .collect::<std::collections::BTreeMap<_, _>>();
    for qualification_id in qualification_ids {
        let limitation = limitations_by_id
            .get(qualification_id)
            .expect("qualification_id must resolve to a real ProgramSpace limitation");
        assert!(
            !limitation.related_capabilities.is_empty(),
            "a limitation reachable via qualification_ids must actually relate to a \
             named capability, not an untied obstruction"
        );
        assert!(
            limitation.related_capabilities.iter().any(|capability| {
                reasons
                    .iter()
                    .any(|reason| reason.ends_with(&format!(":{capability}")))
            }),
            "the qualifying limitation `{}`'s related_capabilities {:?} must \
             correspond to one of the obligation's reasons {reasons:?}",
            limitation.id,
            limitation.related_capabilities
        );
    }
}

#[test]
fn a_real_ingest_run_extraction_report_validates_against_the_public_schema() {
    let repository = fixture_repository();
    let result = ingest(&repository.request()).expect("M2 ingest succeeds");
    let schema: serde_json::Value =
        serde_json::from_slice(EXTRACTION_REPORT_SCHEMA).expect("schema parses as JSON");
    let report_bytes = reviewgraphen_core::canonical_json(&result.extraction_report)
        .expect("extraction report canonicalizes");
    let report: serde_json::Value =
        serde_json::from_slice(&report_bytes).expect("canonical report parses as JSON");
    jsonschema::validator_for(&schema)
        .expect("public extraction_report schema is valid")
        .validate(&report)
        .expect("a real M2 extraction_report validates against the public schema");
}

/// A small pool of candidate Rust modules the property test below draws
/// from. Each module calls its own private helper, so a generated
/// repository still exercises real containment/call-resolution facts, not
/// just inert files.
const CANDIDATE_MODULES: [(&str, &str); 3] = [
    (
        "mod_a",
        "pub fn entry_a() {\n    helper_a();\n}\n\nfn helper_a() {}\n",
    ),
    (
        "mod_b",
        "pub fn entry_b() {\n    helper_b();\n}\n\nfn helper_b() {}\n",
    ),
    (
        "mod_c",
        "pub fn entry_c() {\n    helper_c();\n}\n\nfn helper_c() {}\n",
    ),
];

/// A deterministic Fisher-Yates permutation of `0..len`, driven entirely by
/// `seed`. Used to vary the *order* the property test below writes files
/// and picks which ones change on the target commit -- a different seed
/// must produce a different order/selection, so the property actually
/// exercises order-independence instead of repeating one fixed shape.
fn shuffled_indices(seed: u64, len: usize) -> Vec<usize> {
    let mut indices = (0..len).collect::<Vec<_>>();
    let mut state = seed | 1;
    for i in (1..len).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let j = (state % (i as u64 + 1)) as usize;
        indices.swap(i, j);
    }
    indices
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn canonical_output_is_deterministic_across_generated_repository_shapes(
        shuffle_seed in any::<u64>(),
        file_count in 1usize..=CANDIDATE_MODULES.len(),
        max_files_slack in 0usize..8,
    ) {
        // `order` drives both the on-disk *write* order for the base commit
        // (the resulting tree/commit is identical either way, since a
        // single `git add .` captures the whole tree at once -- this
        // exercises the filesystem write sequence, not Git's own,
        // content-addressed tree ordering) and, via its prefix, which
        // module(s) get a real content change for the target commit --
        // i.e. which `change:*`/`changed_by` facts exist at all. Both are
        // genuinely determined by `shuffle_seed`, not fixed.
        let order = shuffled_indices(shuffle_seed, file_count);
        let changed_count = (shuffle_seed as usize) % (file_count + 1);
        let changed = &order[..changed_count];

        let workspace = tempfile::tempdir().expect("temporary workspace");
        let repository_path = workspace.path().join("repo");
        fs::create_dir(&repository_path).expect("repository directory");
        git(&repository_path, ["init", "--quiet"]);
        git(
            &repository_path,
            ["config", "user.email", "reviewgraphen@example.test"],
        );
        git(&repository_path, ["config", "user.name", "ReviewGraphen test"]);
        // Deliberately no `Cargo.toml`: `cargo_metadata` is already covered
        // by its own dedicated tests, and omitting it keeps each of the 32
        // generated cases to Git/`syn`-level work only (no `cargo`
        // subprocess), which is what this property actually exercises.
        let mut lib_rs = String::new();
        for (name, _) in &CANDIDATE_MODULES[..file_count] {
            lib_rs.push_str(&format!("mod {name};\n"));
        }
        write(&repository_path, "src/lib.rs", &lib_rs);
        for &index in &order {
            let (name, content) = CANDIDATE_MODULES[index];
            write(&repository_path, &format!("src/{name}.rs"), content);
        }
        git(&repository_path, ["add", "."]);
        git(&repository_path, ["commit", "--quiet", "-m", "base"]);
        let base = git_stdout(&repository_path, ["rev-parse", "HEAD"]);

        for &index in changed {
            let (name, content) = CANDIDATE_MODULES[index];
            write(
                &repository_path,
                &format!("src/{name}.rs"),
                &format!("{content}pub fn extra_{index}() {{}}\n"),
            );
        }
        git(&repository_path, ["add", "."]);
        git(
            &repository_path,
            ["commit", "--quiet", "--allow-empty", "-m", "target"],
        );
        let target = git_stdout(&repository_path, ["rev-parse", "HEAD"]);

        let mut request = IngestRequest::new(
            workspace.path(),
            &repository_path,
            "reviewgraphen.test/pbt-generated-fixture".to_owned(),
            &base,
            &target,
        );
        // Also vary `IngestConfig.limits` (config), always widened above the
        // default so it never rejects the generated repository.
        request.config.limits.max_files = IngestLimits::default().max_files + max_files_slack;

        let first = ingest(&request).expect("first ingest succeeds");
        let second = ingest(&request).expect("second ingest succeeds");
        prop_assert_eq!(
            first.canonical_output().expect("first canonical output"),
            second.canonical_output().expect("second canonical output")
        );
    }
}
