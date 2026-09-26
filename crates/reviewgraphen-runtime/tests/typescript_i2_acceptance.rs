//! R5 acceptance calls for I2 D aggregation and N/D/G closure.
//!
//! Source-derived IDs and closed vocabulary make the same assertions without
//! reopening string constructors. Identity preimages use the frozen, name-free
//! canonical JSON keys rather than R3's callable display labels.
//! Every test currently reaches a skeleton `todo!()`.

// Imports used only by the unimplemented A2-A6/D1 contracts below.
#![cfg_attr(not(reviewgraphen_unimplemented_contracts), allow(unused_imports))]

use reviewgraphen_core::source_review::admitted_source::{
    AdmittedSourceBundleV1, AdmittedSourceFileV1,
};
use reviewgraphen_core::source_review::basis::{
    SourceFileOutcome, SourceReviewBasisV1, SourceReviewFileV1, SourceReviewSyntaxV1,
    SourceSyntaxRole,
};
use reviewgraphen_core::source_review::ids::{
    AccountingMismatch, CallerId, CallsiteId, CanonicalFileKey, ChangeWitnessRef, DeclarationId,
    SnapshotBinding, SourceFileId, SourceHash, SourceRange, SourceWitnessKeyV1, SyntaxKeyV1,
};
use reviewgraphen_core::source_review::reasons::{
    BindingReasonSetV1, BindingReasonV1, BindingStageV1, CallReason, CallStageV1,
    CallableReasonSetV1, CallableReasonV1, ReasonSet, RecordOutcomeV1, ResolutionKind,
    ResolutionOutcomeV1, TypeScriptSyntaxKind,
};
use reviewgraphen_core::source_review::registry::typescript_registry_binding;
use reviewgraphen_core::source_review::synthesize::{ObligationClosureSubmission, SynthesizeInput};
use reviewgraphen_core::typescript::rules::{
    CoverageLayer, GAP_RULE_ID, PropertyId, RuleId, TS_RULE_ARMS, TargetKind, rule_arm,
};
use reviewgraphen_core::{ContentHash, canonical_json};
use reviewgraphen_ingest::source_review::extraction_report::ExtractionReport;
use reviewgraphen_ingest::typescript::payload::{
    CallableOutcomeV1, CandidatePathV1, PayloadError, PayloadImportKindV1, ScopeKindV1, SyntaxRole,
    TypeScriptOutcomeV1, TypeScriptPayload, TypeScriptPayloadData, TypeScriptPrimaryReasonV1,
    TypeScriptReasonsV1, encode_payload_draft, rebuild_payload_from_source,
};
use reviewgraphen_runtime::generic_v5::admission::{
    PayloadSubmission, ReconstructionContextV1, SourceAdmissionBoundsV1, SourceAdmissionRequestV1,
    SourceAdmissionSubmissionV1, SourceAncestorCauseV1, SourceInputError,
    SourceInventoryEntryKindV1, SourceIoFailureV1, SourceProfileClaimV1, SourceProjectionV1,
    SourceReadClaimV1, SourceReadExtentV1, SourceSnapshotSideV1, SourceValidated,
    admit_reconstruction_context, build_source_admission_submission_from_git,
    validate_payload_from_source,
};
// A2-A6/D1 admission is not implemented yet; its contracts compile only with
// `--cfg reviewgraphen_unimplemented_contracts`.
#[cfg(reviewgraphen_unimplemented_contracts)]
use reviewgraphen_runtime::generic_v5::admission::{
    synthesize, validate_catalog_from_basis, validate_extraction_from_basis,
    validate_ingestion_from_sources, validate_obligation_closure, validate_synthesis_input,
};
use reviewgraphen_runtime::generic_v5::ingestion::{
    IngestionReport, IngestionSubmission, ResolvedPair,
};
#[cfg(reviewgraphen_unimplemented_contracts)]
use reviewgraphen_runtime::generic_v5::ingestion::{
    aggregate_pairs, d_partition, validate_ingestion_registry_binding,
};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn file_id(path: &str) -> SourceFileId {
    SourceFileId::from_basis_file_key(
        CanonicalFileKey::from_basis_path(path).expect("canonical fixture path"),
    )
}

fn range(start: u64, end: u64) -> SourceRange {
    SourceRange::new(start, end).expect("fixture source range")
}

const A1_CALLABLE_SCOPE_SOURCE: &str = "export function value(){return 2;}\n";
const A1_KEY_RELATION_SOURCE: &str = "function alpha(){return 1;}\nfunction beta(){return 2;}\n";
const A1_UNRESOLVED_CALL_SOURCE: &str = "export function g(){return f()}\n";
const A1_RESOLVED_LOCAL_CALL_SOURCE: &str =
    "function f(){return 1}\nexport function g(){return f()}\n";
const A1_RELATIVE_CALL_SOURCE: &str =
    "import { dependency } from \"./dependency\";\nexport function value(){return dependency();}\n";
const A1_RELATIVE_CALLEE_SOURCE: &str = "export function dependency(){return 1;}\n";

fn admitted_sources() -> AdmittedSourceBundleV1 {
    let basis = SourceReviewBasisV1::new(
        typescript_registry_binding(),
        vec![
            SourceReviewFileV1 {
                path: "src/client.ts".to_owned(),
                language: "typescript".to_owned(),
                outcome: SourceFileOutcome::Parsed,
            },
            SourceReviewFileV1 {
                path: "src/api.ts".to_owned(),
                language: "typescript".to_owned(),
                outcome: SourceFileOutcome::Parsed,
            },
        ],
        Vec::new(),
    )
    .expect("runtime fixture basis");
    let client = b"export function client(){return api()}\n";
    let api = b"export function api(){return 1}\n";
    AdmittedSourceBundleV1::new(
        &basis,
        vec![
            AdmittedSourceFileV1 {
                file_id: file_id("src/client.ts"),
                bytes: client.to_vec(),
                source_hash: SourceHash::from_source_bytes(client),
            },
            AdmittedSourceFileV1 {
                file_id: file_id("src/api.ts"),
                bytes: api.to_vec(),
                source_hash: SourceHash::from_source_bytes(api),
            },
        ],
    )
    .expect("runtime fixture source bytes")
}

fn git_output(root: &Path, arguments: &[&str]) -> String {
    let inherited_path = std::env::var_os("PATH");
    let mut command = Command::new("git");
    command
        .env_clear()
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("LC_ALL", "C")
        .args(arguments);
    if let Some(path) = inherited_path {
        command.env("PATH", path);
    }
    let output = command
        .output()
        .expect("start replace-ref fixture Git command");
    assert!(output.status.success(), "Git fixture command {arguments:?}");
    String::from_utf8(output.stdout)
        .expect("Git fixture stdout is UTF-8")
        .trim()
        .to_owned()
}

fn git_output_with_input(root: &Path, arguments: &[&str], input: &[u8]) -> String {
    let inherited_path = std::env::var_os("PATH");
    let mut command = Command::new("git");
    command
        .env_clear()
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .args(arguments);
    if let Some(path) = inherited_path {
        command.env("PATH", path);
    }
    let mut child = command
        .spawn()
        .expect("start literal Git fixture command with stdin");
    child
        .stdin
        .take()
        .expect("fixture Git stdin is available")
        .write_all(input)
        .expect("write literal Git fixture input");
    let output = child
        .wait_with_output()
        .expect("wait for literal Git fixture command");
    assert!(output.status.success(), "Git fixture command {arguments:?}");
    String::from_utf8(output.stdout)
        .expect("Git fixture stdout is UTF-8")
        .trim()
        .to_owned()
}

fn literal_git_oid(root: &Path, revision: &str) -> String {
    git_output(root, &["--no-replace-objects", "rev-parse", revision])
}

fn literal_tree_for_commit(root: &Path, commit_oid: &str) -> String {
    let revision = format!("{commit_oid}^{{tree}}");
    literal_git_oid(root, &revision)
}

/// Test-only literal traversal for `git.target-root-set@1`. This deliberately
/// uses Git plumbing rather than I2's production identity materializer.
fn literal_target_root_set_identity(root: &Path, target_commit_oid: &str) -> String {
    let object_format = git_output(
        root,
        &["--no-replace-objects", "rev-parse", "--show-object-format"],
    );
    let mut pending = vec![target_commit_oid.to_owned()];
    let mut visited = BTreeSet::new();
    let mut roots = BTreeSet::new();

    while let Some(commit_oid) = pending.pop() {
        if !visited.insert(commit_oid.clone()) {
            continue;
        }
        assert_eq!(
            git_output(
                root,
                &["--no-replace-objects", "cat-file", "-t", &commit_oid],
            ),
            "commit",
            "identity fixture traversal reads literal commit objects",
        );
        let commit = git_output(
            root,
            &["--no-replace-objects", "cat-file", "-p", &commit_oid],
        );
        let parents = commit
            .lines()
            .take_while(|line| !line.is_empty())
            .filter_map(|line| line.strip_prefix("parent ").map(str::to_owned))
            .collect::<Vec<_>>();
        if parents.is_empty() {
            roots.insert(commit_oid);
        } else {
            pending.extend(parents);
        }
    }

    assert!(
        !roots.is_empty(),
        "a literal target commit history has at least one root"
    );
    format!(
        "git.target-root-set@1:{object_format}:{}",
        roots.into_iter().collect::<Vec<_>>().join(",")
    )
}

fn canonical_other_repository_identity(identity: &str) -> String {
    let (prefix, root_list) = identity
        .rsplit_once(':')
        .expect("canonical identity has an object-format and root list");
    let mut roots = root_list.split(',').map(str::to_owned).collect::<Vec<_>>();
    assert!(!roots.is_empty(), "canonical identity has a root");
    let replacement = "0".repeat(roots[0].len());
    assert_ne!(roots[0], replacement, "fixture root is not the null OID");
    roots[0] = replacement;
    roots.sort();
    roots.dedup();
    let changed = format!("{prefix}:{}", roots.join(","));
    assert_ne!(changed, identity, "one canonical root OID changed");
    changed
}

fn fixture_request(
    workspace: &tempfile::TempDir,
    repository: PathBuf,
    base_commit_oid: String,
    base_tree_oid: String,
    target_commit_oid: String,
    target_tree_oid: String,
) -> SourceAdmissionRequestV1 {
    let repository_identity = literal_target_root_set_identity(&repository, &target_commit_oid);
    SourceAdmissionRequestV1 {
        workspace_admission_root: workspace.path().to_owned(),
        repository_admission_root: repository,
        repository_identity,
        base_commit_oid,
        base_tree_oid,
        target_commit_oid,
        target_tree_oid,
        registry_binding: typescript_registry_binding(),
        bounds: SourceAdmissionBoundsV1 {
            max_files: 10,
            max_file_bytes: 1024,
            max_total_source_bytes: 4096,
        },
    }
}

struct A0GitFixture {
    _workspace: tempfile::TempDir,
    request: SourceAdmissionRequestV1,
    base_commit: String,
    base_tree: String,
    base_blob: String,
    target_commit: String,
    target_tree: String,
    target_blob: String,
    target_src_tree: String,
    replacement_commit: String,
    replacement_tree: String,
    replacement_blob: String,
}

impl A0GitFixture {
    fn with_replace_ref() -> Self {
        let workspace = tempfile::tempdir().expect("temporary A0 replace-ref workspace");
        let repository = workspace.path().join("repository");
        fs::create_dir(&repository).expect("create fixture repository");
        git_output(&repository, &["init", "--quiet"]);
        git_output(&repository, &["config", "user.email", "i2@example.invalid"]);
        git_output(&repository, &["config", "user.name", "I2 acceptance"]);
        fs::create_dir_all(repository.join("src")).expect("create fixture source directory");

        fs::write(
            repository.join("src/app.ts"),
            "export function value(){return 1}\n",
        )
        .expect("write base source");
        git_output(&repository, &["add", "."]);
        git_output(&repository, &["commit", "--quiet", "-m", "base"]);
        let base_commit = literal_git_oid(&repository, "HEAD");
        let base_tree = literal_tree_for_commit(&repository, &base_commit);
        let base_blob = literal_git_oid(&repository, "HEAD:src/app.ts");

        fs::write(
            repository.join("src/app.ts"),
            "export function value(){return 2}\n",
        )
        .expect("write literal target source");
        git_output(&repository, &["add", "."]);
        git_output(&repository, &["commit", "--quiet", "-m", "literal target"]);
        let target_commit = literal_git_oid(&repository, "HEAD");
        let target_tree = literal_tree_for_commit(&repository, &target_commit);
        let target_blob = literal_git_oid(&repository, "HEAD:src/app.ts");
        let target_src_tree = literal_git_oid(&repository, "HEAD:src");

        fs::write(
            repository.join("src/app.ts"),
            "export function value(){return 999}\n",
        )
        .expect("write replacement source");
        git_output(&repository, &["add", "."]);
        git_output(&repository, &["commit", "--quiet", "-m", "replacement"]);
        let replacement_commit = literal_git_oid(&repository, "HEAD");
        let replacement_tree = literal_tree_for_commit(&repository, &replacement_commit);
        let replacement_blob = literal_git_oid(&repository, "HEAD:src/app.ts");
        let replacement_src_tree = literal_git_oid(&repository, "HEAD:src");
        git_output(
            &repository,
            &["replace", &target_commit, &replacement_commit],
        );
        git_output(
            &repository,
            &["replace", &target_src_tree, &replacement_src_tree],
        );
        git_output(&repository, &["replace", &target_blob, &replacement_blob]);

        assert_ne!(
            base_tree, target_tree,
            "fixture commits have distinct literal trees"
        );
        assert_ne!(
            base_blob, target_blob,
            "fixture commits have distinct literal blobs"
        );
        assert_ne!(
            target_tree, replacement_tree,
            "replacement tree differs from the literal target tree"
        );
        assert_ne!(
            target_blob, replacement_blob,
            "replacement blob differs from the literal target blob"
        );

        // These request-side values are read again after all replacement refs
        // exist. The assertions below use the independently-read fields above.
        let request_base_commit = literal_git_oid(&repository, &base_commit);
        let request_base_tree = literal_tree_for_commit(&repository, &base_commit);
        let request_target_commit = literal_git_oid(&repository, &target_commit);
        let request_target_tree = literal_tree_for_commit(&repository, &target_commit);

        Self {
            request: fixture_request(
                &workspace,
                repository,
                request_base_commit,
                request_base_tree,
                request_target_commit,
                request_target_tree,
            ),
            _workspace: workspace,
            base_commit,
            base_tree,
            base_blob,
            target_commit: target_commit.clone(),
            target_tree,
            target_blob,
            target_src_tree,
            replacement_commit,
            replacement_tree,
            replacement_blob,
        }
    }

    fn with_target_main(target_main: &str) -> Self {
        let workspace = tempfile::tempdir().expect("temporary A0 role workspace");
        let repository = workspace.path().join("repository");
        fs::create_dir(&repository).expect("create role fixture repository");
        git_output(&repository, &["init", "--quiet"]);
        git_output(&repository, &["config", "user.email", "i2@example.invalid"]);
        git_output(&repository, &["config", "user.name", "I2 acceptance"]);
        fs::create_dir_all(repository.join("src")).expect("create role fixture source directory");

        fs::write(
            repository.join("src/main.ts"),
            "import { dependency } from \"./dependency\";\nexport function value(){return 1;}\n",
        )
        .expect("write role fixture base main source");
        fs::write(
            repository.join("src/dependency.ts"),
            "export function dependency(){return 1;}\n",
        )
        .expect("write role fixture dependency source");
        git_output(&repository, &["add", "."]);
        git_output(&repository, &["commit", "--quiet", "-m", "base"]);
        let base_commit = literal_git_oid(&repository, "HEAD");
        let base_tree = literal_tree_for_commit(&repository, &base_commit);
        let base_blob = literal_git_oid(&repository, "HEAD:src/main.ts");

        fs::write(repository.join("src/main.ts"), target_main)
            .expect("write role fixture target main source");
        git_output(&repository, &["add", "."]);
        git_output(&repository, &["commit", "--quiet", "-m", "target"]);
        let target_commit = literal_git_oid(&repository, "HEAD");
        let target_tree = literal_tree_for_commit(&repository, &target_commit);
        let target_blob = literal_git_oid(&repository, "HEAD:src/main.ts");
        let target_src_tree = literal_git_oid(&repository, "HEAD:src");

        Self {
            request: fixture_request(
                &workspace,
                repository,
                base_commit.clone(),
                base_tree.clone(),
                target_commit.clone(),
                target_tree.clone(),
            ),
            _workspace: workspace,
            base_commit,
            base_tree,
            base_blob,
            target_commit: target_commit.clone(),
            target_tree: target_tree.clone(),
            target_blob: target_blob.clone(),
            target_src_tree,
            replacement_commit: target_commit.clone(),
            replacement_tree: target_tree,
            replacement_blob: target_blob,
        }
    }

    fn with_call_target() -> Self {
        Self::with_target_main(
            "import { dependency } from \"./dependency\";\nexport function value(){return dependency();}\n",
        )
    }

    fn with_call_free_binding_target() -> Self {
        Self::with_target_main(
            "import { dependency } from \"./dependency\";\nexport function value(){return 2;}\n",
        )
    }

    fn with_callable_scope_target() -> Self {
        Self::with_target_main(A1_CALLABLE_SCOPE_SOURCE)
    }

    fn with_key_relation_target() -> Self {
        Self::with_target_main(A1_KEY_RELATION_SOURCE)
    }

    fn with_unresolved_call_target() -> Self {
        Self::with_target_main(A1_UNRESOLVED_CALL_SOURCE)
    }

    fn with_resolved_local_call_target() -> Self {
        Self::with_target_main(A1_RESOLVED_LOCAL_CALL_SOURCE)
    }

    fn with_parse_failed_target() -> Self {
        Self::with_target_main("export function {\n")
    }

    fn submitted(&self) -> SourceAdmissionSubmissionV1 {
        build_source_admission_submission_from_git(self.request.clone())
            .expect("raw builder produces the A0 submission from literal Git objects")
    }
}

struct MultiRootGitFixture {
    _workspace: tempfile::TempDir,
    request: SourceAdmissionRequestV1,
    object_format: String,
    root_oids: Vec<String>,
}

impl MultiRootGitFixture {
    fn with_unrelated_history_merge() -> Self {
        let workspace = tempfile::tempdir().expect("temporary multiple-root A0 workspace");
        let repository = workspace.path().join("repository");
        fs::create_dir(&repository).expect("create multiple-root fixture repository");
        git_output(&repository, &["init", "--quiet"]);
        git_output(&repository, &["config", "user.email", "i2@example.invalid"]);
        git_output(&repository, &["config", "user.name", "I2 acceptance"]);
        fs::create_dir_all(repository.join("src")).expect("create multiple-root source directory");

        fs::write(
            repository.join("src/app.ts"),
            "export function value(){return 1}\n",
        )
        .expect("write first root source");
        git_output(&repository, &["add", "."]);
        git_output(&repository, &["commit", "--quiet", "-m", "first root"]);
        let first_root = literal_git_oid(&repository, "HEAD");
        let first_tree = literal_tree_for_commit(&repository, &first_root);

        fs::write(
            repository.join("src/app.ts"),
            "export function value(){return 2}\n",
        )
        .expect("write second root source");
        git_output(&repository, &["add", "."]);
        let second_tree = git_output(&repository, &["write-tree"]);
        let second_root = git_output_with_input(
            &repository,
            &["commit-tree", &second_tree],
            b"second root\n",
        );
        let target_commit = git_output_with_input(
            &repository,
            &[
                "commit-tree",
                &second_tree,
                "-p",
                &first_root,
                "-p",
                &second_root,
            ],
            b"merge unrelated histories\n",
        );
        let target_tree = literal_tree_for_commit(&repository, &target_commit);
        let object_format = git_output(
            &repository,
            &["--no-replace-objects", "rev-parse", "--show-object-format"],
        );
        let mut root_oids = vec![first_root.clone(), second_root];
        root_oids.sort();

        Self {
            request: fixture_request(
                &workspace,
                repository,
                first_root,
                first_tree,
                target_commit,
                target_tree,
            ),
            _workspace: workspace,
            object_format,
            root_oids,
        }
    }

    fn submitted(&self) -> SourceAdmissionSubmissionV1 {
        build_source_admission_submission_from_git(self.request.clone())
            .expect("raw builder reads the unrelated-history merge target")
    }
}

struct IdentityUnavailableGitFixture {
    _workspace: tempfile::TempDir,
    request: SourceAdmissionRequestV1,
}

impl IdentityUnavailableGitFixture {
    fn with_missing_target_ancestor() -> Self {
        let workspace = tempfile::tempdir().expect("temporary unavailable-identity A0 workspace");
        let repository = workspace.path().join("repository");
        fs::create_dir(&repository).expect("create unavailable-identity fixture repository");
        git_output(&repository, &["init", "--quiet"]);
        git_output(&repository, &["config", "user.email", "i2@example.invalid"]);
        git_output(&repository, &["config", "user.name", "I2 acceptance"]);
        fs::create_dir_all(repository.join("src"))
            .expect("create unavailable-identity source directory");

        fs::write(
            repository.join("src/app.ts"),
            "export function value(){return 1}\n",
        )
        .expect("write missing-ancestor parent source");
        git_output(&repository, &["add", "."]);
        git_output(
            &repository,
            &["commit", "--quiet", "-m", "missing ancestor"],
        );
        let missing_ancestor = literal_git_oid(&repository, "HEAD");
        let base_tree = literal_tree_for_commit(&repository, &missing_ancestor);
        let base_commit = git_output_with_input(
            &repository,
            &["commit-tree", &base_tree],
            b"independent readable base\n",
        );

        fs::write(
            repository.join("src/app.ts"),
            "export function value(){return 2}\n",
        )
        .expect("write unavailable-identity target source");
        git_output(&repository, &["add", "."]);
        let target_tree = git_output(&repository, &["write-tree"]);
        let target_commit = git_output_with_input(
            &repository,
            &["commit-tree", &target_tree, "-p", &missing_ancestor],
            b"target with removable ancestor\n",
        );
        let request = fixture_request(
            &workspace,
            repository.clone(),
            base_commit,
            base_tree,
            target_commit,
            target_tree,
        );

        let missing_ancestor_path = repository
            .join(".git")
            .join("objects")
            .join(&missing_ancestor[..2])
            .join(&missing_ancestor[2..]);
        assert!(
            missing_ancestor_path.is_file(),
            "new fixture ancestor is a removable loose object"
        );
        fs::remove_file(&missing_ancestor_path).expect("remove target ancestor object");

        Self {
            _workspace: workspace,
            request,
        }
    }

    fn submitted(&self) -> SourceAdmissionSubmissionV1 {
        build_source_admission_submission_from_git(self.request.clone())
            .expect("raw builder reads the selected roots without walking target ancestry")
    }
}

struct PartialGitFixture {
    _workspace: tempfile::TempDir,
    request: SourceAdmissionRequestV1,
    missing_object_oid: String,
}

fn init_partial_fixture_repository() -> (tempfile::TempDir, PathBuf, String, String, String) {
    let workspace = tempfile::tempdir().expect("temporary partial A0 workspace");
    let repository = workspace.path().join("repository");
    fs::create_dir(&repository).expect("create partial fixture repository");
    git_output(&repository, &["init", "--quiet"]);
    git_output(&repository, &["config", "user.email", "i2@example.invalid"]);
    git_output(&repository, &["config", "user.name", "I2 acceptance"]);
    fs::create_dir_all(repository.join("src")).expect("create partial fixture source directory");
    fs::write(
        repository.join("src/good.ts"),
        "export function good(){return 1;}\n",
    )
    .expect("write partial fixture readable source");
    git_output(&repository, &["add", "."]);
    git_output(&repository, &["commit", "--quiet", "-m", "base"]);
    let base_commit = literal_git_oid(&repository, "HEAD");
    let base_tree = literal_tree_for_commit(&repository, &base_commit);
    let src_tree = literal_git_oid(&repository, "HEAD:src");
    (workspace, repository, base_commit, base_tree, src_tree)
}

impl PartialGitFixture {
    fn with_missing_subtree() -> Self {
        // Fixture F3-R36: parent tree has an OID for a subtree
        // which the object database cannot supply. The root and `src` remain
        // literal readable Git objects.
        let (workspace, repository, base_commit, base_tree, src_tree) =
            init_partial_fixture_repository();
        let missing_object_oid = "0123456789012345678901234567890123456789".to_owned();
        let root_tree = git_output_with_input(
            &repository,
            &["mktree", "--missing"],
            format!("040000 tree {src_tree}\tsrc\n040000 tree {missing_object_oid}\tmissing\n")
                .as_bytes(),
        );
        let target_commit = git_output_with_input(
            &repository,
            &["commit-tree", &root_tree, "-p", &base_commit],
            b"target with missing subtree\n",
        );
        let target_tree = literal_tree_for_commit(&repository, &target_commit);
        assert_eq!(target_tree, root_tree);
        Self {
            request: fixture_request(
                &workspace,
                repository,
                base_commit,
                base_tree,
                target_commit,
                target_tree,
            ),
            _workspace: workspace,
            missing_object_oid,
        }
    }

    fn with_missing_blob() -> Self {
        // The missing `.ts` blob has a readable parent tree. Future A0 must
        // preserve its empty InterruptedPrefix rather than fabricate a full
        // source hash or fail the entire root admission.
        let (workspace, repository, base_commit, base_tree, _src_tree) =
            init_partial_fixture_repository();
        let good_blob = literal_git_oid(&repository, "HEAD:src/good.ts");
        let missing_object_oid = "fedcba9876543210fedcba9876543210fedcba98".to_owned();
        let target_src_tree = git_output_with_input(
            &repository,
            &["mktree", "--missing"],
            format!(
                "100644 blob {good_blob}\tgood.ts\n100644 blob {missing_object_oid}\tmissing.ts\n"
            )
            .as_bytes(),
        );
        let target_tree = git_output_with_input(
            &repository,
            &["mktree"],
            format!("040000 tree {target_src_tree}\tsrc\n").as_bytes(),
        );
        let target_commit = git_output_with_input(
            &repository,
            &["commit-tree", &target_tree, "-p", &base_commit],
            b"target with missing blob\n",
        );
        Self {
            request: fixture_request(
                &workspace,
                repository,
                base_commit,
                base_tree,
                target_commit,
                target_tree,
            ),
            _workspace: workspace,
            missing_object_oid,
        }
    }
}

struct NonRegularGitFixture {
    _workspace: tempfile::TempDir,
    request: SourceAdmissionRequestV1,
    symlink_oid: String,
    submodule_oid: String,
}

impl NonRegularGitFixture {
    fn with_symlink_and_submodule() -> Self {
        let workspace = tempfile::tempdir().expect("temporary non-regular A0 workspace");
        let repository = workspace.path().join("repository");
        fs::create_dir(&repository).expect("create non-regular fixture repository");
        git_output(&repository, &["init", "--quiet"]);
        git_output(&repository, &["config", "user.email", "i2@example.invalid"]);
        git_output(&repository, &["config", "user.name", "I2 acceptance"]);
        fs::create_dir_all(repository.join("src")).expect("create non-regular source directory");
        fs::write(
            repository.join("src/app.ts"),
            "export function value(){return 1}\n",
        )
        .expect("write non-regular base source");
        git_output(&repository, &["add", "."]);
        git_output(&repository, &["commit", "--quiet", "-m", "base"]);
        let base_commit = literal_git_oid(&repository, "HEAD");
        let base_tree = literal_tree_for_commit(&repository, &base_commit);

        fs::write(
            repository.join("src/app.ts"),
            "export function value(){return 2}\n",
        )
        .expect("write non-regular target source");
        std::os::unix::fs::symlink("src", repository.join("link-to-src"))
            .expect("create fixture symlink");
        git_output(&repository, &["add", "."]);
        let submodule_oid = "1111111111111111111111111111111111111111".to_owned();
        let cache_info = format!("160000,{submodule_oid},vendor/submodule");
        git_output(
            &repository,
            &["update-index", "--add", "--cacheinfo", &cache_info],
        );
        git_output(&repository, &["commit", "--quiet", "-m", "target"]);
        let target_commit = literal_git_oid(&repository, "HEAD");
        let target_tree = literal_tree_for_commit(&repository, &target_commit);
        let symlink_oid = literal_git_oid(&repository, "HEAD:link-to-src");
        assert_eq!(
            literal_git_oid(&repository, "HEAD:vendor/submodule"),
            submodule_oid,
        );

        Self {
            request: fixture_request(
                &workspace,
                repository,
                base_commit,
                base_tree,
                target_commit,
                target_tree,
            ),
            _workspace: workspace,
            symlink_oid,
            submodule_oid,
        }
    }
}

fn assert_source_mismatch(error: SourceInputError, projection: SourceProjectionV1) {
    match error {
        SourceInputError::SourceMismatch(mismatch) => assert_eq!(mismatch.projection, projection),
        other => panic!("expected A0 SourceMismatch for {projection:?}, got {other:?}"),
    }
}

#[test]
fn ac15_a0_ignores_a_replace_ref_and_rebuilds_the_literal_target_tree() {
    // The expected OIDs are literal Git object
    // values collected with replacement objects disabled; the builder is only
    // the raw-submission stimulus and A0 rereads those objects.
    let fixture = A0GitFixture::with_replace_ref();
    let context = admit_reconstruction_context(fixture.submitted())
        .expect("A0 must rebuild the literal target rather than follow refs/replace");
    assert_eq!(context.base_commit_oid(), fixture.base_commit);
    assert_eq!(context.base_tree_oid(), fixture.base_tree);
    assert_eq!(context.target_commit_oid(), fixture.target_commit);
    assert_eq!(context.target_tree_oid(), fixture.target_tree);
    assert_eq!(context.registry_binding(), &typescript_registry_binding());
    assert_eq!(context.bounds(), &fixture.request.bounds);
    assert!(
        !context.snapshot_binding().as_str().is_empty(),
        "A0 exposes a non-empty source-derived snapshot binding"
    );
}

#[test]
fn a0_rejects_a_target_inventory_oid_claim_from_the_submitter() {
    // The design requires complete inventory comparison, including
    // the source-observed object OID rather than a caller's replacement value.
    let fixture = A0GitFixture::with_replace_ref();
    let mut submitted = fixture.submitted();
    let app = submitted
        .material
        .target_inventory
        .iter_mut()
        .find(|entry| entry.path == "src/app.ts")
        .expect("builder projects the literal target app.ts inventory row");
    assert_eq!(app.object_oid, fixture.target_blob);
    app.object_oid = fixture.base_blob;
    assert_source_mismatch(
        admit_reconstruction_context(submitted)
            .expect_err("A0 rejects one target inventory OID substitution"),
        SourceProjectionV1::Inventory,
    );
}

#[test]
fn a0_rejects_a_tree_oid_that_does_not_belong_to_the_selected_commit() {
    // The target commit's literal tree is the expected value;
    // a replacement tree supplied in the raw request is not the selected tree.
    let fixture = A0GitFixture::with_replace_ref();
    let mut submitted = fixture.submitted();
    submitted.request.target_tree_oid = fixture.replacement_tree.clone();
    match admit_reconstruction_context(submitted) {
        Err(SourceInputError::CommitTreeMismatch {
            side: SourceSnapshotSideV1::Target,
            expected,
            submitted,
        }) => {
            assert_eq!(expected, fixture.target_tree);
            assert_eq!(submitted, fixture.replacement_tree);
        }
        other => panic!("expected target CommitTreeMismatch, got {other:?}"),
    }
}

#[test]
fn a0_rejects_the_submitters_inventory_complete_claim() {
    // Enumeration completion is an A0 observation,
    // not a declaration in the raw submission DTO.
    let fixture = A0GitFixture::with_replace_ref();
    let mut submitted = fixture.submitted();
    assert!(
        submitted.material.inventory_complete,
        "the complete fixture tree has no traversal obstruction"
    );
    submitted.material.inventory_complete = false;
    assert_source_mismatch(
        admit_reconstruction_context(submitted)
            .expect_err("A0 rejects one inventory_complete claim substitution"),
        SourceProjectionV1::InventoryComplete,
    );
}

#[test]
fn v1a_a0_ignores_tree_and_blob_replace_refs_in_material_projections() {
    // These expectations come from literal
    // object reads in the fixture, not from the raw builder's output.
    let fixture = A0GitFixture::with_replace_ref();
    let submitted = fixture.submitted();
    let app = submitted
        .material
        .target_inventory
        .iter()
        .find(|entry| entry.path == "src/app.ts")
        .expect("literal target has src/app.ts inventory evidence");
    assert_eq!(app.object_oid, fixture.target_blob);
    assert_eq!(app.mode, 0o100644);
    assert_eq!(app.entry_kind, SourceInventoryEntryKindV1::Blob);
    assert_eq!(app.profile, SourceProfileClaimV1::Included);
    assert_eq!(app.outcome, SourceFileOutcome::Parsed);
    match &app.read {
        SourceReadClaimV1::Complete {
            byte_count,
            source_hash,
        } => {
            assert_eq!(*byte_count, 34);
            assert_eq!(
                source_hash,
                &SourceHash::from_source_bytes(b"export function value(){return 2}\n"),
            );
        }
        other => panic!("expected complete literal app.ts read, got {other:?}"),
    }
    let source = submitted
        .material
        .target_read_material
        .iter()
        .find(|material| material.path == "src/app.ts")
        .expect("literal target has complete app.ts read material");
    assert_eq!(source.extent, SourceReadExtentV1::FullBlob);
    assert_eq!(source.bytes, b"export function value(){return 2}\n");
    let src_tree = submitted
        .material
        .target_tree_entries
        .iter()
        .find(|entry| entry.path == "src")
        .expect("literal target has src tree evidence");
    assert_eq!(src_tree.object_oid, fixture.target_src_tree);
    assert_eq!(src_tree.mode, 0o040000);
    assert_eq!(src_tree.parent_tree_oid, fixture.target_tree);
    assert_ne!(fixture.target_blob, fixture.replacement_blob);
    let context = admit_reconstruction_context(submitted)
        .expect("A0 rebuilds literal tree and blob material despite replacement refs");
    assert_eq!(context.target_tree_oid(), fixture.target_tree);
}

#[test]
fn v1a_a0_rejects_a_changed_repository_identity_claim() {
    // git.target-root-set@1 rule 4. The fixture's matching
    // value is independently derived from literal roots. Change just one root
    // to another full lower-case OID so this remains a canonical request.
    let fixture = A0GitFixture::with_replace_ref();
    let mut submitted = fixture.submitted();
    let original = submitted.request.repository_identity.clone();
    submitted.request.repository_identity = canonical_other_repository_identity(&original);
    assert_ne!(submitted.request.repository_identity, original);
    assert_eq!(
        admit_reconstruction_context(submitted),
        Err(SourceInputError::RepositoryIdentityMismatch),
    );
}

#[test]
fn v1a_a0_rejects_a_noncanonical_repository_identity_claim() {
    // git.target-root-set@1 rule 2 and rule 4. This is deliberately nonempty:
    // rejection comes from identity syntax, not the older empty-string gate.
    let fixture = A0GitFixture::with_replace_ref();
    let mut submitted = fixture.submitted();
    submitted.request.repository_identity = "git.target-root-set@1:sha1:not-an-oid".to_owned();
    assert_eq!(
        admit_reconstruction_context(submitted),
        Err(SourceInputError::InvalidRequest),
    );
}

#[test]
fn v1a_a0_rejects_an_unavailable_target_root_identity() {
    // git.target-root-set@1 rule 6. The request was derived while the target
    // ancestor existed, then that ancestor is removed. A0 must not fall back
    // to the caller's otherwise canonical claim.
    let fixture = IdentityUnavailableGitFixture::with_missing_target_ancestor();
    assert_eq!(
        admit_reconstruction_context(fixture.submitted()),
        Err(SourceInputError::RepositoryIdentityUnavailable),
    );
}

#[test]
fn v1a_a0_accepts_a_sorted_multiple_literal_root_identity() {
    // git.target-root-set@1 rules 1, 2, and 9. The merge has two unrelated
    // literal parent histories; the fixture records both roots in byte order.
    let fixture = MultiRootGitFixture::with_unrelated_history_merge();
    let expected = format!(
        "git.target-root-set@1:{}:{}",
        fixture.object_format,
        fixture.root_oids.join(",")
    );
    assert_eq!(fixture.root_oids.len(), 2, "fixture has two distinct roots");
    assert_eq!(fixture.request.repository_identity, expected);
    admit_reconstruction_context(fixture.submitted())
        .expect("A0 admits a canonical two-root identity");
}

#[test]
fn v1a_a0_rejects_a_changed_request_registry_binding() {
    // This is a request-gate mutation: target_basis.binding is
    // deliberately left unchanged, so the required error is RegistryMismatch
    // rather than the later BasisBinding SourceMismatch.
    let fixture = A0GitFixture::with_replace_ref();
    let mut submitted = fixture.submitted();
    let original = submitted.request.registry_binding.registry_hash.clone();
    submitted.request.registry_binding.registry_hash =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_owned();
    assert_ne!(submitted.request.registry_binding.registry_hash, original);
    assert_eq!(
        admit_reconstruction_context(submitted),
        Err(SourceInputError::RegistryMismatch),
    );
}

#[test]
#[ignore = "slice V1a-2: after V0"]
fn v1a_source_literal_syntax_projection_has_all_five_roles() {
    // `src/main.ts` has all five roles; the dependency
    // file supplies Callable, Surface, and Scope. The expected projection is
    // a direct, sorted projection of the two source literals.
    let fixture = A0GitFixture::with_call_target();
    let submitted = fixture.submitted();
    assert_eq!(
        submitted.material.target_basis.syntax,
        vec![
            SourceReviewSyntaxV1 {
                file_path: "src/dependency.ts".to_owned(),
                role: SourceSyntaxRole::Callable,
            },
            SourceReviewSyntaxV1 {
                file_path: "src/dependency.ts".to_owned(),
                role: SourceSyntaxRole::Surface,
            },
            SourceReviewSyntaxV1 {
                file_path: "src/dependency.ts".to_owned(),
                role: SourceSyntaxRole::Scope,
            },
            SourceReviewSyntaxV1 {
                file_path: "src/main.ts".to_owned(),
                role: SourceSyntaxRole::Callable,
            },
            SourceReviewSyntaxV1 {
                file_path: "src/main.ts".to_owned(),
                role: SourceSyntaxRole::Call,
            },
            SourceReviewSyntaxV1 {
                file_path: "src/main.ts".to_owned(),
                role: SourceSyntaxRole::Binding,
            },
            SourceReviewSyntaxV1 {
                file_path: "src/main.ts".to_owned(),
                role: SourceSyntaxRole::Surface,
            },
            SourceReviewSyntaxV1 {
                file_path: "src/main.ts".to_owned(),
                role: SourceSyntaxRole::Scope,
            },
        ],
    );
}

#[test]
#[ignore = "slice V1a-2: after V0"]
fn v1a_a0_rejects_a_target_with_call_syntax_until_v1b() {
    // Any parsed Call-role record makes C non-empty or
    // potentially non-empty, so A0 must fail closed before creating a context.
    let fixture = A0GitFixture::with_call_target();
    assert_eq!(
        admit_reconstruction_context(fixture.submitted()),
        Err(SourceInputError::ExactPairReconstructionUnsupported),
    );
}

#[test]
fn v1a_call_free_target_admits_and_exposes_literal_accessor_values() {
    // The target has Binding, Surface, Callable, and
    // Scope but no Call, so C is empty and the seven public accessors remain
    // the only context observation surface.
    let fixture = A0GitFixture::with_call_free_binding_target();
    let context = admit_reconstruction_context(fixture.submitted())
        .expect("a call-free target is admissible during V1a");
    assert_eq!(context.base_commit_oid(), fixture.base_commit);
    assert_eq!(context.base_tree_oid(), fixture.base_tree);
    assert_eq!(context.target_commit_oid(), fixture.target_commit);
    assert_eq!(context.target_tree_oid(), fixture.target_tree);
    assert_eq!(context.registry_binding(), &typescript_registry_binding());
    assert_eq!(context.bounds(), &fixture.request.bounds);
    assert!(!context.snapshot_binding().as_str().is_empty());
}

#[test]
fn v1a_parse_failed_file_keeps_full_read_material_without_syntax_projection() {
    // The invalid literal is still a full Git
    // blob and remains material, but it contributes no parsed syntax row.
    let fixture = A0GitFixture::with_parse_failed_target();
    let submitted = fixture.submitted();
    let malformed = submitted
        .material
        .target_inventory
        .iter()
        .find(|entry| entry.path == "src/main.ts")
        .expect("malformed file stays in target inventory");
    assert_eq!(malformed.outcome, SourceFileOutcome::ParseFailed);
    assert!(matches!(malformed.read, SourceReadClaimV1::Complete { .. }));
    let material = submitted
        .material
        .target_read_material
        .iter()
        .find(|material| material.path == "src/main.ts")
        .expect("parse failure retains full source bytes");
    assert_eq!(material.extent, SourceReadExtentV1::FullBlob);
    assert_eq!(material.bytes, b"export function {\n");
    assert!(
        submitted
            .material
            .target_basis
            .syntax
            .iter()
            .all(|syntax| syntax.file_path != "src/main.ts")
    );
    admit_reconstruction_context(submitted)
        .expect("parse failure is source material, not an A0 reconstruction failure");
}

#[test]
fn v1a_snapshot_binding_changes_with_each_admitted_snapshot_pair() {
    // The three admissions use one repository,
    // identity, registry binding, and bounds. The base variant changes only
    // base commit/tree; the target variant changes only target commit/tree.
    // The public wrapper has no specified serialization, so this checks its
    // binding-change property rather than an implementation wire literal.
    let fixture = A0GitFixture::with_replace_ref();
    let literal_context = admit_reconstruction_context(fixture.submitted())
        .expect("literal target admission succeeds");
    let mut alternate_base_request = fixture.request.clone();
    alternate_base_request.base_commit_oid = fixture.replacement_commit.clone();
    alternate_base_request.base_tree_oid = fixture.replacement_tree.clone();
    let alternate_base_context = admit_reconstruction_context(
        build_source_admission_submission_from_git(alternate_base_request)
            .expect("alternate base raw projection is independently rebuilt"),
    )
    .expect("alternate base admission succeeds");
    let mut replacement_request = fixture.request.clone();
    replacement_request.target_commit_oid = fixture.replacement_commit.clone();
    replacement_request.target_tree_oid = fixture.replacement_tree.clone();
    let replacement_context = admit_reconstruction_context(
        build_source_admission_submission_from_git(replacement_request)
            .expect("replacement target raw projection is independently rebuilt"),
    )
    .expect("replacement target admission succeeds");
    assert_ne!(
        literal_context.base_commit_oid(),
        alternate_base_context.base_commit_oid()
    );
    assert_ne!(
        literal_context.base_tree_oid(),
        alternate_base_context.base_tree_oid()
    );
    assert_eq!(
        literal_context.target_commit_oid(),
        alternate_base_context.target_commit_oid()
    );
    assert_eq!(
        literal_context.target_tree_oid(),
        alternate_base_context.target_tree_oid()
    );
    assert_ne!(
        literal_context.snapshot_binding(),
        alternate_base_context.snapshot_binding(),
    );
    assert_ne!(
        literal_context.target_commit_oid(),
        replacement_context.target_commit_oid()
    );
    assert_ne!(
        literal_context.target_tree_oid(),
        replacement_context.target_tree_oid()
    );
    assert_ne!(
        literal_context.snapshot_binding(),
        replacement_context.snapshot_binding(),
    );
}

#[test]
fn v1a_snapshot_binding_changes_between_admitted_root_sets() {
    // git.target-root-set@1 rules 5 and 9. Both admissions are
    // valid and their target root sets differ; snapshot binding changes with
    // the two independently derived repository identities.
    let one_root = A0GitFixture::with_replace_ref();
    let two_roots = MultiRootGitFixture::with_unrelated_history_merge();
    assert_ne!(
        one_root.request.repository_identity, two_roots.request.repository_identity,
        "the two admitted repositories have different root-set identities"
    );
    let one_root_context = admit_reconstruction_context(one_root.submitted())
        .expect("A0 admits the single-root identity");
    let two_roots_context = admit_reconstruction_context(two_roots.submitted())
        .expect("A0 admits the two-root identity");
    assert_ne!(
        one_root_context.snapshot_binding(),
        two_roots_context.snapshot_binding(),
        "snapshot binding commits the admitted repository identity"
    );
}

#[test]
fn v1a_missing_child_tree_is_admitted_as_partial_with_its_ancestor() {
    // The assertion is deliberately on the
    // builder result first: source-backed partial material must exist before
    // A0 can compare it in a second literal Git read.
    let fixture = PartialGitFixture::with_missing_subtree();
    let built = build_source_admission_submission_from_git(fixture.request.clone());
    assert!(
        built.is_ok(),
        "a missing discovered subtree is partial material, not root failure: {built:?}"
    );
    let submitted = built.expect("assertion above establishes partial raw material");
    assert!(!submitted.material.inventory_complete);
    assert!(submitted.material.target_tree_entries.iter().any(|entry| {
        entry.path == "missing" && entry.object_oid == fixture.missing_object_oid
    }));
    assert!(
        submitted
            .material
            .unexpanded_ancestors
            .iter()
            .any(|ancestor| {
                ancestor.path == "missing"
                    && ancestor.object_oid == fixture.missing_object_oid
                    && ancestor.cause
                        == SourceAncestorCauseV1::TreeReadFailed(SourceIoFailureV1::MissingObject)
            })
    );
    admit_reconstruction_context(submitted)
        .expect("A0 rereads and admits the same persistent partial observation");
}

#[test]
fn v1a_missing_blob_preserves_an_interrupted_prefix_instead_of_failing_root_admission() {
    // The missing blob has a readable parent tree,
    // so it is target read evidence, not a false Absent or an invalid root.
    let fixture = PartialGitFixture::with_missing_blob();
    let built = build_source_admission_submission_from_git(fixture.request.clone());
    assert!(
        built.is_ok(),
        "a missing child blob is partial read material, not root failure: {built:?}"
    );
    let submitted = built.expect("assertion above establishes partial raw material");
    let missing = submitted
        .material
        .target_inventory
        .iter()
        .find(|entry| entry.path == "src/missing.ts")
        .expect("literal target tree retains missing.ts in inventory");
    assert_eq!(missing.object_oid, fixture.missing_object_oid);
    assert_eq!(missing.entry_kind, SourceInventoryEntryKindV1::Blob);
    assert!(matches!(
        missing.read,
        SourceReadClaimV1::Failed {
            byte_count: 0,
            cause: SourceIoFailureV1::MissingObject,
            ..
        }
    ));
    let prefix = submitted
        .material
        .target_read_material
        .iter()
        .find(|material| material.path == "src/missing.ts")
        .expect("failed read retains an empty interrupted prefix material row");
    assert_eq!(prefix.extent, SourceReadExtentV1::InterruptedPrefix);
    assert!(prefix.bytes.is_empty());
    // `inventory_complete` records tree enumeration only; an
    // unreadable child blob is partial read material, not an incomplete tree.
    assert!(
        submitted.material.inventory_complete,
        "a missing child blob must not make the tree inventory incomplete"
    );
    admit_reconstruction_context(submitted)
        .expect("A0 rereads and admits the same missing-blob partial observation");
}

#[test]
fn v1a_symlink_and_submodule_stay_in_inventory_and_unexpanded_ancestors() {
    // Both entries are source facts and ancestor
    // obstructions; neither may disappear merely because traversal stops.
    let fixture = NonRegularGitFixture::with_symlink_and_submodule();
    let submitted = build_source_admission_submission_from_git(fixture.request.clone())
        .expect("raw builder reads non-regular target entries");
    assert!(submitted.material.target_inventory.iter().any(|entry| {
        entry.path == "link-to-src"
            && entry.object_oid == fixture.symlink_oid
            && entry.entry_kind == SourceInventoryEntryKindV1::Symlink
    }));
    assert!(submitted.material.target_inventory.iter().any(|entry| {
        entry.path == "vendor/submodule"
            && entry.object_oid == fixture.submodule_oid
            && entry.entry_kind == SourceInventoryEntryKindV1::Submodule
    }));
    assert!(
        submitted
            .material
            .unexpanded_ancestors
            .iter()
            .any(|ancestor| {
                ancestor.path == "link-to-src"
                    && ancestor.object_oid == fixture.symlink_oid
                    && ancestor.cause == SourceAncestorCauseV1::Symlink
            })
    );
    assert!(
        submitted
            .material
            .unexpanded_ancestors
            .iter()
            .any(|ancestor| {
                ancestor.path == "vendor/submodule"
                    && ancestor.object_oid == fixture.submodule_oid
                    && ancestor.cause == SourceAncestorCauseV1::Submodule
            })
    );
    admit_reconstruction_context(submitted)
        .expect("A0 compares the source-derived non-regular projection");
}

/// Independently serializes an acceptance preimage; it must not call I2's
/// production identity or obligation materializers.
fn acceptance_canonical_key(value: Value) -> String {
    String::from_utf8(
        canonical_json(&value).expect("independent acceptance canonical key preimage"),
    )
    .expect("canonical JSON is UTF-8")
}

#[cfg(reviewgraphen_unimplemented_contracts)]
fn acceptance_source_file_key(path: &str) -> String {
    acceptance_canonical_key(json!({
        "basis_file_key": path,
        "domain": "source_review_identity.v1",
        "kind": "source_file",
    }))
}

#[cfg(reviewgraphen_unimplemented_contracts)]
fn acceptance_declaration_key(path: &str, start: u64, end: u64) -> String {
    acceptance_canonical_key(json!({
        "declaration_range": {"end": end, "start": start},
        "domain": "source_review_identity.v1",
        "file_key": acceptance_source_file_key(path),
        "kind": "declaration",
    }))
}

#[cfg(reviewgraphen_unimplemented_contracts)]
fn acceptance_caller_key(path: &str, start: u64, end: u64) -> String {
    acceptance_canonical_key(json!({
        "declaration_key": acceptance_declaration_key(path, start, end),
        "domain": "source_review_identity.v1",
        "kind": "caller",
    }))
}

#[cfg(reviewgraphen_unimplemented_contracts)]
fn acceptance_pair_key(caller_start: u64, callee_start: u64) -> String {
    acceptance_canonical_key(json!({
        "callee_key": acceptance_declaration_key("src/api.ts", callee_start, callee_start + 9),
        "caller_key": acceptance_caller_key("src/client.ts", caller_start, caller_start + 9),
        "domain": "source_review_identity.v1",
        "kind": "declaration_pair",
    }))
}

#[cfg(reviewgraphen_unimplemented_contracts)]
fn caller_id(start: u64) -> CallerId {
    CallerId::from_declaration(DeclarationId::from_source(
        file_id("src/client.ts"),
        range(start, start + 9),
    ))
}

#[cfg(reviewgraphen_unimplemented_contracts)]
fn callee_id(start: u64) -> DeclarationId {
    DeclarationId::from_source(file_id("src/api.ts"), range(start, start + 9))
}

#[cfg(reviewgraphen_unimplemented_contracts)]
fn callsite_id(caller: CallerId, start: u64) -> CallsiteId {
    CallsiteId::from_source(
        file_id("src/client.ts"),
        range(start, start + 3),
        Some(caller),
    )
}

#[cfg(reviewgraphen_unimplemented_contracts)]
fn acceptance_pair_key_with_ranges(
    caller_start: u64,
    caller_end: u64,
    callee_start: u64,
    callee_end: u64,
) -> String {
    acceptance_canonical_key(json!({
        "callee_key": acceptance_declaration_key("src/api.ts", callee_start, callee_end),
        "caller_key": acceptance_caller_key("src/client.ts", caller_start, caller_end),
        "domain": "source_review_identity.v1",
        "kind": "declaration_pair",
    }))
}

#[cfg(reviewgraphen_unimplemented_contracts)]
fn resolved_with_ranges(
    caller_start: u64,
    caller_end: u64,
    callee_start: u64,
    callee_end: u64,
    callsite_start: u64,
) -> ResolvedPair {
    let caller = CallerId::from_declaration(DeclarationId::from_source(
        file_id("src/client.ts"),
        range(caller_start, caller_end),
    ));
    let callee = DeclarationId::from_source(file_id("src/api.ts"), range(callee_start, callee_end));
    ResolvedPair {
        caller_id: caller.clone(),
        callee_id: callee.clone(),
        callsite_key: callsite_id(caller, callsite_start),
        resolution_kind: ResolutionKind::SyntacticUnique,
        callsite_reasons: ReasonSet::new(Vec::<CallReason>::new()),
        change_witness_refs: BTreeSet::from([ChangeWitnessRef {
            declaration_id: callee.clone(),
            witness_range: range(callee_start, callee_end),
        }]),
        visibility_witness_refs: BTreeSet::from([ChangeWitnessRef {
            declaration_id: callee,
            witness_range: range(callee_start, callee_end),
        }]),
        changed: true,
        callee_public: true,
    }
}

#[cfg(reviewgraphen_unimplemented_contracts)]
fn resolved(
    changed: bool,
    callee_public: bool,
    caller_start: u64,
    callee_start: u64,
    callsite_start: u64,
) -> ResolvedPair {
    let caller = caller_id(caller_start);
    let callee = callee_id(callee_start);
    ResolvedPair {
        caller_id: caller.clone(),
        callee_id: callee.clone(),
        callsite_key: callsite_id(caller, callsite_start),
        resolution_kind: ResolutionKind::SyntacticUnique,
        callsite_reasons: ReasonSet::new(Vec::<CallReason>::new()),
        change_witness_refs: BTreeSet::from([ChangeWitnessRef {
            declaration_id: callee.clone(),
            witness_range: range(callee_start, callee_start + 9),
        }]),
        visibility_witness_refs: BTreeSet::from([ChangeWitnessRef {
            declaration_id: callee,
            witness_range: range(callee_start, callee_start + 9),
        }]),
        changed,
        callee_public,
    }
}

#[cfg(reviewgraphen_unimplemented_contracts)]
#[ignore = "slice V5: A4"]
#[test]
fn k2_pair_aggregation_is_order_independent_and_keeps_every_ineligible_reason() {
    let first = aggregate_pairs(vec![
        resolved(false, false, 10, 50, 1),
        resolved(false, false, 10, 50, 2),
    ])
    .expect("distinct callsites aggregate");
    let second = aggregate_pairs(vec![
        resolved(false, false, 10, 50, 2),
        resolved(false, false, 10, 50, 1),
    ])
    .expect("distinct callsites aggregate");

    let expected_callsites =
        BTreeSet::from([callsite_id(caller_id(10), 1), callsite_id(caller_id(10), 2)]);
    for result in [&first, &second] {
        assert_eq!(result.len(), 1);
        assert!(!result[0].eligible);
        assert_eq!(
            result[0]
                .callsite_reasons
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>(),
            expected_callsites
        );
    }
    assert_eq!(first, second);

    assert!(
        aggregate_pairs(vec![
            resolved(false, false, 10, 50, 1),
            resolved(true, false, 10, 50, 2),
        ])
        .is_err(),
        "one declaration pair cannot aggregate conflicting source attributes"
    );
}

#[cfg(reviewgraphen_unimplemented_contracts)]
#[ignore = "slice V5: A4"]
#[test]
fn k7_d_partial_false_rejects_extra_d_and_gap_targets() {
    // A6 is the only full-closure
    // admission; this witness prevents a raw partition helper from replacing
    // that boundary. The executable extra-D/foreign-G mutations are attached
    // to the A0→A3→A4→A5→D1 path once the skeleton bodies exist.
    let _a6: fn(
        SourceValidated<ObligationSet>,
        ObligationClosureSubmission,
    ) -> Result<SourceValidated<ObligationSet>, AccountingMismatch> = validate_obligation_closure;
    let _ = admitted_sources();
}

#[cfg(reviewgraphen_unimplemented_contracts)]
#[ignore = "slice V5: A4"]
#[test]
fn k8_pair_digest_is_a_sorted_set_literal_and_registry_field_swaps_reject() {
    let ordered_pairs = aggregate_pairs(vec![
        resolved(true, true, 10, 50, 1),
        resolved(true, true, 20, 50, 2),
    ])
    .expect("distinct callsites aggregate");
    let ordered = d_partition(&ordered_pairs).expect("raw D partition");
    let reordered_pairs = aggregate_pairs(vec![
        resolved(true, true, 20, 50, 2),
        resolved(true, true, 10, 50, 1),
    ])
    .expect("distinct callsites aggregate");
    let reordered = d_partition(&reordered_pairs).expect("raw D partition");
    let expected_pair_keys =
        BTreeSet::from([acceptance_pair_key(10, 50), acceptance_pair_key(20, 50)])
            .into_iter()
            .collect::<Vec<_>>();
    let expected_digest_preimage = json!({
        "declaration_pair_keys": expected_pair_keys,
        "domain": "source_review_d_partition.v1",
    });
    let expected_digest = ContentHash::sha256(
        &canonical_json(&expected_digest_preimage)
            .expect("independent canonical D partition digest preimage"),
    );
    assert_eq!(ordered.digest, expected_digest);
    assert_eq!(reordered.digest, ordered.digest);

    let mut binding = typescript_registry_binding();
    std::mem::swap(&mut binding.registry_hash, &mut binding.arm_hash);
    assert!(
        validate_ingestion_registry_binding(binding).is_err(),
        "registry fields are positional, not a bag of known strings"
    );
}

#[ignore = "slice V7: D1"]
#[test]
fn k13_obligation_preimage_and_gap_rule_fields_match_the_frozen_fixture() {
    let registry: Value = serde_json::from_str(include_str!(
        "../../reviewgraphen-cli/tests/fixtures/typescript-v1/registry.r1.json"
    ))
    .expect("frozen registry JSON");
    for arm in TS_RULE_ARMS {
        let fixture_rule = registry["arms"][0]["rules"]
            .as_array()
            .expect("rules")
            .iter()
            .find(|rule| rule["rule_id"] == arm.rule_id.wire_literal())
            .expect("frozen rule");
        assert_eq!(
            arm.target_kind.wire_literal(),
            fixture_rule["target_kind"].as_str().expect("target_kind")
        );
        assert_eq!(
            arm.coverage_layer.wire_literal(),
            fixture_rule["coverage_layer"]
                .as_str()
                .expect("coverage_layer")
        );
    }
    assert_eq!(
        rule_arm(GAP_RULE_ID).coverage_layer.wire_literal(),
        "snapshot_gap",
        "the G arm retains the fixture coverage_layer"
    );

    // The design classifies full preimage/ID snapshots as non-literal until the
    // source witness encoder and registry bytes are independently available.
    // D1's exact capability-only signature is checked in AC14; this fixture
    // keeps the frozen arm literals as the independent oracle.
    let _ = admitted_sources();
}

#[test]
fn v2_a1_admits_ac2_unresolved_call_payload_and_rejects_changed_envelope_fields() {
    // The design fixes this exact AC2 fixture. The source has no declaration
    // for f and no competing obstruction, so only unresolved_name is present.
    let context = admitted_unresolved_call_context();
    let (submitted, expected) =
        source_rebuilt_call_payload_case(&context, A1_UNRESOLVED_CALL_SOURCE);
    let call_start = A1_UNRESOLVED_CALL_SOURCE
        .rfind("f()")
        .expect("AC2 literal has its direct call") as u64;
    let accepted = validate_payload_from_source(&context, submitted.clone())
        .expect("A1 admits the raw rebuilt AC2 call draft");
    let accepted = accepted.as_ref();
    assert_eq!(accepted.role, SyntaxRole::Call);
    assert_eq!(accepted.range, range(call_start, call_start + 3));
    assert_eq!(
        accepted.outcome,
        TypeScriptOutcomeV1::Call(ResolutionOutcomeV1::Unresolved)
    );
    assert_eq!(
        accepted.reasons,
        TypeScriptReasonsV1::Call(ReasonSet::new([CallReason::UnresolvedName]))
    );
    assert_eq!(
        accepted.primary_reason,
        Some(TypeScriptPrimaryReasonV1::Call(CallReason::UnresolvedName))
    );
    assert_eq!(accepted.kind, expected.kind);
    assert_eq!(accepted.refs, expected.refs);
    assert_eq!(accepted.descriptor_id, expected.descriptor_id);
    assert_eq!(accepted.descriptor_hash, expected.descriptor_hash);
    let TypeScriptPayloadData::Call(data) = &accepted.data else {
        panic!("AC2 accepted payload has CallDataV1")
    };
    let TypeScriptPayloadData::Call(expected_data) = &expected.data else {
        panic!("AC2 raw rebuild has CallDataV1")
    };
    let caller_range = range(0, A1_UNRESOLVED_CALL_SOURCE.len() as u64 - 1);
    assert_eq!(
        data.caller_id,
        Some(CallerId::from_declaration(DeclarationId::from_source(
            file_id("src/main.ts"),
            caller_range
        )))
    );
    assert_eq!(data.callee_id, None);
    assert_eq!(data.resolution_kind, None);
    assert_eq!(data.binding_key, None, "bare f has no import binding");
    assert_eq!(
        data.candidate_witness_keys,
        expected_data.candidate_witness_keys
    );
    assert_eq!(data.evaluated_stages, expected_data.evaluated_stages);

    let mut changed_range = submitted.clone();
    changed_range.draft.range = range(call_start + 1, call_start + 3);
    assert_a1_source_mismatch(&context, changed_range, "call range");

    let mut changed_role = submitted.clone();
    changed_role.draft.role = SyntaxRole::Scope;
    assert_a1_source_mismatch(&context, changed_role, "call role");

    let mut changed_outcome = submitted.clone();
    changed_outcome.draft.outcome = TypeScriptOutcomeV1::Call(ResolutionOutcomeV1::Resolved);
    assert_a1_source_mismatch(&context, changed_outcome, "call outcome");

    let mut changed_reasons = submitted;
    changed_reasons.draft.reasons = TypeScriptReasonsV1::Call(ReasonSet::new([]));
    changed_reasons.draft.primary_reason = None;
    assert_a1_source_mismatch(&context, changed_reasons, "call reasons and primary");
}

#[test]
fn v2_a1_admits_resolved_local_call_payload_with_source_endpoint_identities() {
    // The design admits this sole top-level local f() as syntactic_unique.
    let context = admitted_resolved_local_call_context();
    let (submitted, expected) =
        source_rebuilt_call_payload_case(&context, A1_RESOLVED_LOCAL_CALL_SOURCE);
    let call_start = A1_RESOLVED_LOCAL_CALL_SOURCE
        .rfind("f()")
        .expect("resolved-local literal has its direct call") as u64;
    let callee_end = A1_RESOLVED_LOCAL_CALL_SOURCE
        .find('\n')
        .expect("first local f declaration ends at newline") as u64;
    let caller_start = A1_RESOLVED_LOCAL_CALL_SOURCE
        .find("export function g")
        .expect("literal has g caller") as u64;
    let accepted = validate_payload_from_source(&context, submitted)
        .expect("A1 admits the raw rebuilt resolved local call draft");
    let accepted = accepted.as_ref();
    assert_eq!(accepted.role, SyntaxRole::Call);
    assert_eq!(accepted.range, range(call_start, call_start + 3));
    assert_eq!(
        accepted.outcome,
        TypeScriptOutcomeV1::Call(ResolutionOutcomeV1::Resolved)
    );
    assert_eq!(
        accepted.reasons,
        TypeScriptReasonsV1::Call(ReasonSet::new([]))
    );
    assert_eq!(accepted.primary_reason, None);
    assert_eq!(accepted.kind, expected.kind);
    assert_eq!(accepted.refs, expected.refs);
    assert_eq!(accepted.descriptor_id, expected.descriptor_id);
    assert_eq!(accepted.descriptor_hash, expected.descriptor_hash);
    let TypeScriptPayloadData::Call(data) = &accepted.data else {
        panic!("resolved local accepted payload has CallDataV1")
    };
    let TypeScriptPayloadData::Call(expected_data) = &expected.data else {
        panic!("resolved local raw rebuild has CallDataV1")
    };
    assert_eq!(
        data.caller_id,
        Some(CallerId::from_declaration(DeclarationId::from_source(
            file_id("src/main.ts"),
            range(caller_start, A1_RESOLVED_LOCAL_CALL_SOURCE.len() as u64 - 1)
        )))
    );
    assert_eq!(
        data.callee_id,
        Some(DeclarationId::from_source(
            file_id("src/main.ts"),
            range(0, callee_end)
        ))
    );
    assert_eq!(data.resolution_kind, Some(ResolutionKind::SyntacticUnique));
    assert_eq!(data.binding_key, None, "local f has no import binding");
    assert_eq!(
        data.candidate_witness_keys,
        expected_data.candidate_witness_keys
    );
    assert_eq!(data.evaluated_stages, expected_data.evaluated_stages);
}

#[test]
fn v2callfix_ac2_local_call_records_a_nonempty_evaluated_stage() {
    // The design requires an unevaluated later stage to remain witnessed, and
    // :291 makes evaluated_stages required. This literal has no import binding,
    // so any retained stage belongs to the local-call path rather than lookup.
    let context = admitted_resolved_local_call_context();
    let (_, payload) = source_rebuilt_call_payload_case(&context, A1_RESOLVED_LOCAL_CALL_SOURCE);
    let TypeScriptPayloadData::Call(data) = &payload.data else {
        panic!("local fixture rebuild has CallDataV1")
    };
    assert_eq!(data.binding_key, None, "local f has no import binding");
    assert!(
        !data.evaluated_stages.is_empty(),
        "a local call must retain its evaluated local stage"
    );
}

#[ignore = "slice V2-callfix: relative-import binding resolution is not implemented yet"]
#[test]
fn v2callfix_a1_relative_binding_resolves_the_admitted_exact_pair() {
    // The design makes this a static named relative-import exact pair:
    // one admitted candidate exports its one named runtime callable. The
    // endpoint, candidate witness, and ordered stage expectations below are
    // constructed from that literal contract, not from the production resolver.
    let context = admitted_relative_call_context();
    let binding = typescript_registry_binding();
    let binding_start = A1_RELATIVE_CALL_SOURCE
        .find("import { dependency }")
        .expect("literal has the named-import form")
        + "import { ".len();
    let binding_range = range(
        binding_start as u64,
        (binding_start + "dependency".len()) as u64,
    );
    let expected_binding_key =
        reviewgraphen_ingest::typescript::payload::PayloadBindingKeyV1::parse_wire(
            &binding,
            "dependency",
        )
        .expect("the literal named binding is registered");
    let expected_candidate_witnesses =
        vec![SourceWitnessKeyV1::Syntax(SyntaxKeyV1::derive_from_source(
            &binding,
            context.snapshot_binding(),
            &file_id("src/main.ts"),
            SourceSyntaxRole::Binding,
            &TypeScriptSyntaxKind::parse_wire("import_specifier")
                .expect("the literal named import has a registered syntax kind"),
            binding_range,
        ))];
    let expected_callee = DeclarationId::from_source(
        file_id("src/dependency.ts"),
        source_span(
            A1_RELATIVE_CALLEE_SOURCE,
            "function dependency(){return 1;}",
        ),
    );
    let expected_stages = [
        "c.syntax",
        "c.caller",
        "c.import_form",
        "c.local_binding",
        "c.specifier",
        "c.candidates",
        "c.export_binding",
        "c.shadow",
        "c.writes",
        "c.resolution",
    ]
    .into_iter()
    .map(|wire| {
        CallStageV1::parse_wire(&binding, wire)
            .expect("the frozen Call-stage literal is registered")
    })
    .collect::<Vec<_>>();
    let payload = source_rebuilt_relative_call_payloads(&context)
        .into_iter()
        .find(|payload| payload.role == SyntaxRole::Call)
        .expect("relative literal has one call payload");
    let submitted = PayloadSubmission {
        locator: file_id("src/main.ts"),
        draft: encode_payload_draft(&payload),
    };
    let accepted = validate_payload_from_source(&context, submitted)
        .expect("A1 compares the relative call submission to its source rebuild");
    let TypeScriptPayloadData::Call(data) = &accepted.as_ref().data else {
        panic!("relative accepted payload has CallDataV1")
    };
    assert_eq!(
        accepted.as_ref().outcome,
        TypeScriptOutcomeV1::Call(ResolutionOutcomeV1::Resolved)
    );
    assert_eq!(
        accepted.as_ref().reasons,
        TypeScriptReasonsV1::Call(ReasonSet::new([]))
    );
    assert_eq!(accepted.as_ref().primary_reason, None);
    assert_ne!(
        data.binding_key.as_ref(),
        None,
        "the source has a static named import binding"
    );
    assert_eq!(data.binding_key.as_ref(), Some(&expected_binding_key));
    assert_eq!(data.callee_id.as_ref(), Some(&expected_callee));
    assert_eq!(
        data.resolution_kind,
        Some(ResolutionKind::SyntacticUniqueRelativeImportV1)
    );
    assert_eq!(data.candidate_witness_keys, expected_candidate_witnesses);
    assert_eq!(data.evaluated_stages, expected_stages);
}

#[ignore = "slice V2-callfix: relative-import binding resolution is not implemented yet"]
#[test]
fn v2callfix_encoder_keeps_relative_call_binding_and_surface_data_structured() {
    // The encoder permits only a field-for-field copy. These
    // payloads are encoder inputs; assertions below inspect the raw draft, not
    // a second rebuild, so a Debug-string or fixed-empty-array encoder fails.
    let context = admitted_relative_call_context();
    let payloads = source_rebuilt_relative_call_payloads(&context);
    let mut call = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Call)
        .cloned()
        .expect("relative literal has one call payload");
    let binding = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Binding)
        .expect("relative literal has one binding payload");
    let surface = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Surface)
        .expect("relative literal has one surface payload");
    let (candidate_witness_count, evaluated_stage_count) = {
        let TypeScriptPayloadData::Call(call_data) = &mut call.data else {
            panic!("relative fixture rebuild has CallDataV1")
        };
        let candidate = call_data
            .candidate_witness_keys
            .first()
            .cloned()
            .expect("resolved relative call retains a candidate witness");
        let stage = call_data
            .evaluated_stages
            .first()
            .cloned()
            .expect("resolved relative call retains an evaluated stage");
        call_data.candidate_witness_keys.push(candidate);
        call_data.evaluated_stages.push(stage);
        (
            call_data.candidate_witness_keys.len(),
            call_data.evaluated_stages.len(),
        )
    };

    let encoded_call = encode_payload_draft(&call);
    assert_eq!(
        encoded_call.data["candidate_witness_keys"]
            .as_array()
            .map(Vec::len),
        Some(candidate_witness_count)
    );
    assert_eq!(
        encoded_call.data["evaluated_stages"]
            .as_array()
            .map(Vec::len),
        Some(evaluated_stage_count)
    );

    let encoded_binding = encode_payload_draft(binding);
    assert!(
        encoded_binding.data.is_object(),
        "binding data stays an object"
    );
    let TypeScriptPayloadData::Binding(binding_data) = &binding.data else {
        panic!("relative fixture rebuild has BindingDataV1")
    };
    assert_eq!(
        encoded_binding.data["candidate_paths"]
            .as_array()
            .map(Vec::len),
        Some(binding_data.candidate_paths.len())
    );

    let encoded_surface = encode_payload_draft(surface);
    assert!(
        encoded_surface.data.is_object(),
        "surface data stays an object"
    );
}

#[ignore = "slice S0: unignore after registry literal freeze"]
#[test]
fn v2callfix_closed_vocabulary_variants_round_trip_or_reject_in_their_declared_domain() {
    for reason in [
        CallReason::ParseFailure,
        CallReason::UnsupportedSyntax,
        CallReason::UnsupportedCaller,
        CallReason::DynamicDispatch,
        CallReason::RelativeSpecifierUnsupported,
        CallReason::RelativeTargetUnread,
        CallReason::RelativeTargetAmbiguous,
        CallReason::RelativeTargetExcluded,
        CallReason::RelativeTargetMissing,
        CallReason::ImportBindingAmbiguous,
        CallReason::TypeOnlyBinding,
        CallReason::ExportBindingUnsupported,
        CallReason::ImportResolutionUnavailable,
        CallReason::ShadowedBinding,
        CallReason::WrittenBinding,
        CallReason::UnresolvedName,
    ] {
        assert_eq!(CallReason::parse_wire(reason.wire_literal()), Ok(reason));
    }
    for reason in [
        CallReason::UnchangedCallee,
        CallReason::NonPublicCallee,
        CallReason::ExcludedEndpoint,
    ] {
        assert!(CallReason::parse_wire(reason.wire_literal()).is_err());
    }
    for resolution in [
        ResolutionKind::SyntacticUnique,
        ResolutionKind::SyntacticUniqueRelativeImportV1,
    ] {
        assert_eq!(
            ResolutionKind::parse_wire(resolution.wire_literal()),
            Ok(resolution)
        );
    }
    for target in [TargetKind::Node, TargetKind::Relation, TargetKind::Subgraph] {
        assert_eq!(TargetKind::parse_wire(target.wire_literal()), Ok(target));
    }
    for layer in [
        CoverageLayer::SingleLayer,
        CoverageLayer::TwoLayer,
        CoverageLayer::SnapshotGap,
    ] {
        assert_eq!(CoverageLayer::parse_wire(layer.wire_literal()), Ok(layer));
    }
    for rule in [
        RuleId::ChangedPublicCallee,
        RuleId::PublicFunctionContract,
        RuleId::CapabilityGapOrigin,
    ] {
        assert_eq!(RuleId::parse_wire(rule.wire_literal()), Ok(rule));
    }
    for property in [
        PropertyId::CalleeContractReview,
        PropertyId::PublicFunctionContractReview,
        PropertyId::CapabilityGap,
    ] {
        assert_eq!(
            PropertyId::parse_wire(property.wire_literal()),
            Ok(property)
        );
    }
}

#[cfg(reviewgraphen_unimplemented_contracts)]
#[ignore = "slice V5: A4"]
#[test]
fn ac9_duplicate_callsite_is_an_accounting_mismatch_independent_of_input_order() {
    // The design requires one resolution per callsite key. A duplicate
    // cannot be hidden by map insertion or made order-dependent.
    let first = resolved(true, true, 10, 50, 1);
    let mut conflicting = resolved(false, false, 10, 50, 1);
    conflicting.callsite_reasons = ReasonSet::new([CallReason::ParseFailure]);
    let forward = aggregate_pairs(vec![first.clone(), conflicting.clone()])
        .expect_err("duplicate callsite key is an accounting mismatch");
    let reverse = aggregate_pairs(vec![conflicting, first])
        .expect_err("reversing a duplicate callsite remains an accounting mismatch");
    assert_eq!(
        forward, reverse,
        "duplicate rejection does not depend on arrival order"
    );
}

#[cfg(reviewgraphen_unimplemented_contracts)]
#[ignore = "slice V8: A6"]
#[test]
fn ac10_s6_rejects_extra_records_targets_and_overlapping_partitions() {
    // The three raw mutations (duplicate record, foreign D
    // target, planned/deferred overlap) now belong to A6 after D1. A6 is
    // witnessed here as the only entry able to return `SourceValidated`.
    let _a6: fn(
        SourceValidated<ObligationSet>,
        ObligationClosureSubmission,
    ) -> Result<SourceValidated<ObligationSet>, AccountingMismatch> = validate_obligation_closure;
    let _ = admitted_sources();
}

#[cfg(reviewgraphen_unimplemented_contracts)]
#[ignore = "slice V7: D1"]
#[test]
fn ac12_canonical_id_lists_sort_by_key_bytes_not_numeric_range_order() {
    // The design fixes sorted canonical ID strings.  Numeric SourceRange Ord
    // would order end=99 before end=100, but canonical JSON key bytes order
    // the string containing 100 before the string containing 99.
    let pairs = aggregate_pairs(vec![
        resolved_with_ranges(10, 19, 50, 99, 1),
        resolved_with_ranges(20, 29, 50, 100, 2),
    ])
    .expect("distinct callsites aggregate");
    let partition = d_partition(&pairs).expect("raw D partition");
    let mut canonical_keys = vec![
        acceptance_pair_key_with_ranges(10, 19, 50, 99),
        acceptance_pair_key_with_ranges(20, 29, 50, 100),
    ];
    canonical_keys.sort();
    let expected_preimage = json!({
        "declaration_pair_keys": canonical_keys,
        "domain": "source_review_d_partition.v1",
    });
    assert_eq!(partition.canonical_digest_preimage(), expected_preimage);
    assert_eq!(
        partition.digest,
        ContentHash::sha256(
            &canonical_json(&expected_preimage)
                .expect("independent canonical key-byte ordering preimage"),
        )
    );
}

/// Test-only raw request fixture. Its future implementation creates a literal
/// Git repository and supplies the request fields; it returns no material.
#[cfg(reviewgraphen_unimplemented_contracts)]
fn source_admission_request_fixture() -> SourceAdmissionRequestV1 {
    todo!("acceptance fixture must select literal Git source for A0")
}

/// The one A0 path used by all admission cases: raw builder first, then A0.
#[cfg(reviewgraphen_unimplemented_contracts)]
fn admitted_context_from_source_fixture() -> ReconstructionContextV1 {
    let submitted = build_source_admission_submission_from_git(source_admission_request_fixture())
        .expect("raw builder produces the submitted A0 projection");
    admit_reconstruction_context(submitted)
        .expect("A0 independently rebuilds the submitted literal source projection")
}

fn a1_raw_payload_sources_for(source: &str) -> AdmittedSourceBundleV1 {
    let basis = SourceReviewBasisV1::new(
        typescript_registry_binding(),
        vec![SourceReviewFileV1 {
            path: "src/main.ts".to_owned(),
            language: "typescript".to_owned(),
            outcome: SourceFileOutcome::Parsed,
        }],
        vec![
            SourceReviewSyntaxV1 {
                file_path: "src/main.ts".to_owned(),
                role: SourceSyntaxRole::Callable,
            },
            SourceReviewSyntaxV1 {
                file_path: "src/main.ts".to_owned(),
                role: SourceSyntaxRole::Surface,
            },
            SourceReviewSyntaxV1 {
                file_path: "src/main.ts".to_owned(),
                role: SourceSyntaxRole::Scope,
            },
        ],
    )
    .expect("A1 fixture has the source roles implied by its literal");
    AdmittedSourceBundleV1::new(
        &basis,
        vec![AdmittedSourceFileV1 {
            file_id: file_id("src/main.ts"),
            bytes: source.as_bytes().to_vec(),
            source_hash: SourceHash::from_source_bytes(source.as_bytes()),
        }],
    )
    .expect("A1 fixture admits its literal source bytes")
}

fn a1_raw_call_payload_sources_for(source: &str) -> AdmittedSourceBundleV1 {
    let basis = SourceReviewBasisV1::new(
        typescript_registry_binding(),
        vec![SourceReviewFileV1 {
            path: "src/main.ts".to_owned(),
            language: "typescript".to_owned(),
            outcome: SourceFileOutcome::Parsed,
        }],
        vec![
            SourceReviewSyntaxV1 {
                file_path: "src/main.ts".to_owned(),
                role: SourceSyntaxRole::Callable,
            },
            SourceReviewSyntaxV1 {
                file_path: "src/main.ts".to_owned(),
                role: SourceSyntaxRole::Call,
            },
            SourceReviewSyntaxV1 {
                file_path: "src/main.ts".to_owned(),
                role: SourceSyntaxRole::Surface,
            },
            SourceReviewSyntaxV1 {
                file_path: "src/main.ts".to_owned(),
                role: SourceSyntaxRole::Scope,
            },
        ],
    )
    .expect("A1 call fixture has the source roles implied by its literal");
    AdmittedSourceBundleV1::new(
        &basis,
        vec![AdmittedSourceFileV1 {
            file_id: file_id("src/main.ts"),
            bytes: source.as_bytes().to_vec(),
            source_hash: SourceHash::from_source_bytes(source.as_bytes()),
        }],
    )
    .expect("A1 call fixture admits its literal source bytes")
}

fn a1_raw_relative_call_payload_sources() -> AdmittedSourceBundleV1 {
    let basis = SourceReviewBasisV1::new(
        typescript_registry_binding(),
        vec![
            SourceReviewFileV1 {
                path: "src/dependency.ts".to_owned(),
                language: "typescript".to_owned(),
                outcome: SourceFileOutcome::Parsed,
            },
            SourceReviewFileV1 {
                path: "src/main.ts".to_owned(),
                language: "typescript".to_owned(),
                outcome: SourceFileOutcome::Parsed,
            },
        ],
        vec![
            SourceReviewSyntaxV1 {
                file_path: "src/dependency.ts".to_owned(),
                role: SourceSyntaxRole::Callable,
            },
            SourceReviewSyntaxV1 {
                file_path: "src/dependency.ts".to_owned(),
                role: SourceSyntaxRole::Surface,
            },
            SourceReviewSyntaxV1 {
                file_path: "src/dependency.ts".to_owned(),
                role: SourceSyntaxRole::Scope,
            },
            SourceReviewSyntaxV1 {
                file_path: "src/main.ts".to_owned(),
                role: SourceSyntaxRole::Callable,
            },
            SourceReviewSyntaxV1 {
                file_path: "src/main.ts".to_owned(),
                role: SourceSyntaxRole::Call,
            },
            SourceReviewSyntaxV1 {
                file_path: "src/main.ts".to_owned(),
                role: SourceSyntaxRole::Binding,
            },
            SourceReviewSyntaxV1 {
                file_path: "src/main.ts".to_owned(),
                role: SourceSyntaxRole::Surface,
            },
            SourceReviewSyntaxV1 {
                file_path: "src/main.ts".to_owned(),
                role: SourceSyntaxRole::Scope,
            },
        ],
    )
    .expect("relative A1 fixture has the roles implied by its two literals");
    AdmittedSourceBundleV1::new(
        &basis,
        vec![
            AdmittedSourceFileV1 {
                file_id: file_id("src/dependency.ts"),
                bytes: A1_RELATIVE_CALLEE_SOURCE.as_bytes().to_vec(),
                source_hash: SourceHash::from_source_bytes(A1_RELATIVE_CALLEE_SOURCE.as_bytes()),
            },
            AdmittedSourceFileV1 {
                file_id: file_id("src/main.ts"),
                bytes: A1_RELATIVE_CALL_SOURCE.as_bytes().to_vec(),
                source_hash: SourceHash::from_source_bytes(A1_RELATIVE_CALL_SOURCE.as_bytes()),
            },
        ],
    )
    .expect("relative A1 fixture admits its literal source bytes")
}

fn source_rebuilt_relative_call_payloads(
    context: &ReconstructionContextV1,
) -> Vec<TypeScriptPayload> {
    let sources = a1_raw_relative_call_payload_sources();
    rebuild_payload_from_source(
        &sources,
        context.registry_binding(),
        context.snapshot_binding(),
        file_id("src/main.ts"),
    )
    .expect("raw rebuild produces the relative-call fixture payloads")
}

fn source_rebuilt_payloads(
    context: &ReconstructionContextV1,
    source: &str,
) -> Vec<TypeScriptPayload> {
    let sources = a1_raw_payload_sources_for(source);
    rebuild_payload_from_source(
        &sources,
        context.registry_binding(),
        context.snapshot_binding(),
        file_id("src/main.ts"),
    )
    .expect("raw rebuild produces source payloads for the A1 fixture")
}

fn source_rebuilt_call_payload_case(
    context: &ReconstructionContextV1,
    source: &str,
) -> (PayloadSubmission, TypeScriptPayload) {
    let sources = a1_raw_call_payload_sources_for(source);
    let payload = rebuild_payload_from_source(
        &sources,
        context.registry_binding(),
        context.snapshot_binding(),
        file_id("src/main.ts"),
    )
    .expect("raw rebuild produces source call payloads for the A1 fixture")
    .into_iter()
    .find(|payload| payload.role == SyntaxRole::Call)
    .expect("fixture has its call payload");
    let submitted = PayloadSubmission {
        locator: file_id("src/main.ts"),
        draft: encode_payload_draft(&payload),
    };
    (submitted, payload)
}

/// Builds the submission stimulus from raw source reconstruction.  Expected
/// literal values are deliberately asserted by the callers below, not copied
/// from this payload.
fn source_rebuilt_payload_case(
    context: &ReconstructionContextV1,
    role: SyntaxRole,
) -> (PayloadSubmission, TypeScriptPayload) {
    let payload = source_rebuilt_payloads(context, A1_CALLABLE_SCOPE_SOURCE)
        .into_iter()
        .find(|payload| payload.role == role)
        .unwrap_or_else(|| panic!("fixture has {role:?} payload"));
    let submitted = PayloadSubmission {
        locator: file_id("src/main.ts"),
        draft: encode_payload_draft(&payload),
    };
    (submitted, payload)
}

fn admitted_callable_scope_context() -> ReconstructionContextV1 {
    let fixture = A0GitFixture::with_callable_scope_target();
    admit_reconstruction_context(fixture.submitted())
        .expect("A0 admits the call-free A1 callable and scope fixture")
}

fn admitted_key_relation_context() -> ReconstructionContextV1 {
    let fixture = A0GitFixture::with_key_relation_target();
    admit_reconstruction_context(fixture.submitted())
        .expect("A0 admits the two-member source-key relation fixture")
}

fn admitted_unresolved_call_context() -> ReconstructionContextV1 {
    let fixture = A0GitFixture::with_unresolved_call_target();
    admit_reconstruction_context(fixture.submitted())
        .expect("A0 admits the unresolved-call A1 fixture")
}

fn admitted_resolved_local_call_context() -> ReconstructionContextV1 {
    let fixture = A0GitFixture::with_resolved_local_call_target();
    admit_reconstruction_context(fixture.submitted())
        .expect("A0 admits the resolved-local-call A1 fixture")
}

fn admitted_relative_call_context() -> ReconstructionContextV1 {
    let fixture = A0GitFixture::with_call_target();
    admit_reconstruction_context(fixture.submitted())
        .expect("A0 admits the relative-call A1 fixture")
}

fn key_relation_member_ranges() -> [SourceRange; 2] {
    let alpha_end = A1_KEY_RELATION_SOURCE
        .find('\n')
        .expect("alpha declaration ends at the first source newline") as u64;
    [
        range(0, alpha_end),
        range(alpha_end + 1, A1_KEY_RELATION_SOURCE.len() as u64 - 1),
    ]
}

fn assert_a1_source_mismatch(
    context: &ReconstructionContextV1,
    submitted: PayloadSubmission,
    changed_field: &str,
) {
    assert!(
        matches!(
            validate_payload_from_source(context, submitted),
            Err(PayloadError::SourceMismatch(_))
        ),
        "A1 rejects the changed {changed_field} field as a source mismatch"
    );
}

fn source_span(source: &str, fragment: &str) -> SourceRange {
    let start = source
        .find(fragment)
        .unwrap_or_else(|| panic!("fixture has source fragment {fragment}"));
    assert_eq!(
        source.matches(fragment).count(),
        1,
        "fixture fragment {fragment} has one source range"
    );
    range(start as u64, (start + fragment.len()) as u64)
}

fn source_role_for_payload_role(role: &SyntaxRole) -> SourceSyntaxRole {
    match role {
        SyntaxRole::Callable => SourceSyntaxRole::Callable,
        SyntaxRole::Call => SourceSyntaxRole::Call,
        SyntaxRole::Binding => SourceSyntaxRole::Binding,
        SyntaxRole::Surface => SourceSyntaxRole::Surface,
        SyntaxRole::Scope => SourceSyntaxRole::Scope,
    }
}

fn expected_scope_member_key(
    context: &ReconstructionContextV1,
    payloads: &[TypeScriptPayload],
    expected_role: &SyntaxRole,
    expected_range: &SourceRange,
) -> SyntaxKeyV1 {
    let payload = payloads
        .iter()
        .find(|payload| &payload.role == expected_role && &payload.range == expected_range)
        .unwrap_or_else(|| {
            panic!("fixture has its expected {expected_role:?} record at {expected_range:?}")
        });
    SyntaxKeyV1::derive_from_source(
        context.registry_binding(),
        context.snapshot_binding(),
        &file_id("src/main.ts"),
        source_role_for_payload_role(expected_role),
        &payload.kind,
        *expected_range,
    )
}

fn source_mismatch_detail(
    context: &ReconstructionContextV1,
    submitted: PayloadSubmission,
) -> AccountingMismatch {
    match validate_payload_from_source(context, submitted) {
        Err(PayloadError::SourceMismatch(mismatch)) => mismatch,
        other => panic!("A1 returns SourceMismatch with structured detail, got {other:?}"),
    }
}

#[cfg(reviewgraphen_unimplemented_contracts)]
fn source_rebuilt_catalog_case() -> (
    Vec<reviewgraphen_ingest::source_review::extraction_report::SyntaxRecord>,
    Vec<reviewgraphen_ingest::source_review::extraction_report::SyntaxRecord>,
) {
    todo!("acceptance fixture must rebuild A2 catalogue case from source")
}

#[cfg(reviewgraphen_unimplemented_contracts)]
fn source_tampered_catalog_submission()
-> Vec<reviewgraphen_ingest::source_review::extraction_report::SyntaxRecord> {
    todo!("acceptance fixture must alter exactly one A2 catalogue field after source rebuild")
}

#[cfg(reviewgraphen_unimplemented_contracts)]
fn source_rebuilt_extraction_case() -> (ExtractionReport, ExtractionReport) {
    todo!("acceptance fixture must rebuild A3 extraction case from source")
}

#[cfg(reviewgraphen_unimplemented_contracts)]
fn source_tampered_extraction_submission() -> ExtractionReport {
    todo!("acceptance fixture must alter exactly one A3 extraction field after source rebuild")
}

#[cfg(reviewgraphen_unimplemented_contracts)]
fn source_rebuilt_ingestion_case() -> (IngestionSubmission, IngestionReport) {
    todo!("acceptance fixture must rebuild A4 ingestion case from source")
}

#[cfg(reviewgraphen_unimplemented_contracts)]
fn source_tampered_ingestion_submission() -> IngestionSubmission {
    todo!("acceptance fixture must alter exactly one A4 ingestion field after source rebuild")
}

#[cfg(reviewgraphen_unimplemented_contracts)]
fn source_rebuilt_synthesis_case() -> (SynthesizeInput, SynthesizeInput) {
    todo!("acceptance fixture must rebuild A5 synthesis case from source")
}

#[cfg(reviewgraphen_unimplemented_contracts)]
fn source_tampered_synthesis_submission() -> SynthesizeInput {
    todo!("acceptance fixture must alter exactly one A5 synthesis field after source rebuild")
}

#[cfg(reviewgraphen_unimplemented_contracts)]
fn source_validated_synthesis_input_for_d1() -> SourceValidated<SynthesizeInput> {
    todo!("acceptance fixture must obtain A5 output from source before D1")
}

#[cfg(reviewgraphen_unimplemented_contracts)]
fn source_rebuilt_closure_submission() -> ObligationClosureSubmission {
    todo!("acceptance fixture must rebuild the A6 closure submission from source")
}

#[cfg(reviewgraphen_unimplemented_contracts)]
fn source_tampered_closure_submission() -> ObligationClosureSubmission {
    todo!("acceptance fixture must alter exactly one A6 closure field after source rebuild")
}

#[test]
fn v2_a1_admits_source_rebuilt_callable_payload_with_independent_literals() {
    // These ranges come from the source literal's function
    // declaration, not from either production reconstruction result.
    let context = admitted_callable_scope_context();
    let (submitted, expected) = source_rebuilt_payload_case(&context, SyntaxRole::Callable);
    let callable_start = A1_CALLABLE_SCOPE_SOURCE
        .find("function value")
        .expect("fixture has the function declaration") as u64;
    let callable_range = range(callable_start, A1_CALLABLE_SCOPE_SOURCE.len() as u64 - 1);
    let accepted = validate_payload_from_source(&context, submitted)
        .expect("A1 accepts the callable draft made by the raw encoder");
    let accepted = accepted.as_ref();
    assert_eq!(accepted.role, SyntaxRole::Callable);
    assert_eq!(accepted.range, callable_range);
    assert_eq!(
        accepted.outcome,
        TypeScriptOutcomeV1::Callable(CallableOutcomeV1::EligiblePublic)
    );
    assert!(matches!(
        &accepted.reasons,
        TypeScriptReasonsV1::Callable(reasons) if reasons.is_empty()
    ));
    assert_eq!(accepted.primary_reason, None);
    assert_eq!(accepted.kind, expected.kind);
    assert_eq!(accepted.refs, expected.refs);
    assert_eq!(accepted.descriptor_id, expected.descriptor_id);
    assert_eq!(accepted.descriptor_hash, expected.descriptor_hash);
    let TypeScriptPayloadData::Callable(accepted_data) = &accepted.data else {
        panic!("callable payload has CallableDataV1")
    };
    let TypeScriptPayloadData::Callable(expected_data) = &expected.data else {
        panic!("raw callable payload has CallableDataV1")
    };
    assert_eq!(accepted_data.binding_key, expected_data.binding_key);
    assert_eq!(accepted_data.declaration_range, callable_range);
    assert_eq!(
        accepted_data.implementation_range,
        expected_data.implementation_range
    );
    assert_eq!(
        accepted_data.visibility_value,
        reviewgraphen_ingest::typescript::payload::VisibilityValueV1::Exported
    );
    assert_eq!(
        accepted_data.binding_obstruction_keys,
        expected_data.binding_obstruction_keys
    );
}

#[test]
fn v2_a1_admits_source_rebuilt_scope_payload_with_independent_literals() {
    // The program node's byte range covers
    // the immutable whole-file literal, including its trailing newline.
    let context = admitted_callable_scope_context();
    let (submitted, expected) = source_rebuilt_payload_case(&context, SyntaxRole::Scope);
    let scope_range = range(0, A1_CALLABLE_SCOPE_SOURCE.len() as u64);
    let accepted = validate_payload_from_source(&context, submitted)
        .expect("A1 accepts the scope draft made by the raw encoder");
    let accepted = accepted.as_ref();
    assert_eq!(accepted.role, SyntaxRole::Scope);
    assert_eq!(accepted.range, scope_range);
    assert_eq!(
        accepted.outcome,
        TypeScriptOutcomeV1::Scope(RecordOutcomeV1::Recorded)
    );
    assert!(matches!(
        &accepted.reasons,
        TypeScriptReasonsV1::Scope(reasons) if reasons.is_empty()
    ));
    assert_eq!(accepted.primary_reason, None);
    assert_eq!(accepted.kind, expected.kind);
    assert_eq!(accepted.refs, expected.refs);
    assert_eq!(accepted.descriptor_id, expected.descriptor_id);
    assert_eq!(accepted.descriptor_hash, expected.descriptor_hash);
    let TypeScriptPayloadData::Scope(accepted_data) = &accepted.data else {
        panic!("scope payload has ScopeDataV1")
    };
    let TypeScriptPayloadData::Scope(expected_data) = &expected.data else {
        panic!("raw scope payload has ScopeDataV1")
    };
    assert_eq!(accepted_data.scope_kind, ScopeKindV1::FileLexical);
    assert_eq!(accepted_data.member_keys, expected_data.member_keys);
}

#[test]
fn v2_a1_rejects_each_changed_callable_envelope_field_as_source_mismatch() {
    // The encoder copies its input.  Each mutation happens after encoding, so
    // a normalizing/defaulting encoder cannot erase the A1 negative stimulus.
    let context = admitted_callable_scope_context();
    let (submitted, _) = source_rebuilt_payload_case(&context, SyntaxRole::Callable);

    let mut changed_range = submitted.clone();
    changed_range.draft.range = range(1, A1_CALLABLE_SCOPE_SOURCE.len() as u64 - 1);
    assert_a1_source_mismatch(&context, changed_range, "range");

    let mut changed_role = submitted.clone();
    changed_role.draft.role = SyntaxRole::Scope;
    assert_a1_source_mismatch(&context, changed_role, "role");

    let mut changed_outcome = submitted.clone();
    changed_outcome.draft.outcome = TypeScriptOutcomeV1::Callable(CallableOutcomeV1::NonPublic);
    assert_a1_source_mismatch(&context, changed_outcome, "outcome");

    let changed_reason =
        CallableReasonV1::parse_wire(&typescript_registry_binding(), "not_runtime_callable")
            .expect("the design fixes this callable reason literal");
    let changed_reasons =
        CallableReasonSetV1::new(&typescript_registry_binding(), vec![changed_reason])
            .expect("fixture reason is registered for the callable role");
    let changed_primary = changed_reasons
        .primary()
        .cloned()
        .map(TypeScriptPrimaryReasonV1::Callable);
    let mut changed_reasons_submission = submitted;
    changed_reasons_submission.draft.reasons = TypeScriptReasonsV1::Callable(changed_reasons);
    changed_reasons_submission.draft.primary_reason = changed_primary;
    assert_a1_source_mismatch(
        &context,
        changed_reasons_submission,
        "reasons and their derived primary",
    );
}

#[test]
fn v2_syntax_key_relations_and_scope_member_order_are_source_bound() {
    // Syntax-key wire values remain unfrozen until S0. These assertions check
    // only derivation relations and the alpha-then-beta source-member order.
    let context = admitted_key_relation_context();
    let payloads = source_rebuilt_payloads(&context, A1_KEY_RELATION_SOURCE);
    let scope = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Scope)
        .expect("fixture has one file-lexical scope payload");
    let [alpha_range, beta_range] = key_relation_member_ranges();
    let alpha = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Callable && payload.range == alpha_range)
        .expect("alpha is the first top-level source member");
    let beta = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Callable && payload.range == beta_range)
        .expect("beta is the second top-level source member");
    let file_id = file_id("src/main.ts");
    let baseline_key = SyntaxKeyV1::derive_from_source(
        context.registry_binding(),
        context.snapshot_binding(),
        &file_id,
        SourceSyntaxRole::Scope,
        &scope.kind,
        scope.range,
    );
    assert_eq!(
        baseline_key,
        SyntaxKeyV1::derive_from_source(
            context.registry_binding(),
            context.snapshot_binding(),
            &file_id,
            SourceSyntaxRole::Scope,
            &scope.kind,
            scope.range,
        )
    );
    assert_ne!(
        baseline_key,
        SyntaxKeyV1::derive_from_source(
            context.registry_binding(),
            context.snapshot_binding(),
            &file_id,
            SourceSyntaxRole::Scope,
            &scope.kind,
            range(1, A1_KEY_RELATION_SOURCE.len() as u64),
        )
    );
    assert_ne!(
        baseline_key,
        SyntaxKeyV1::derive_from_source(
            context.registry_binding(),
            context.snapshot_binding(),
            &file_id,
            SourceSyntaxRole::Callable,
            &scope.kind,
            scope.range,
        )
    );
    assert_ne!(scope.kind, alpha.kind);
    assert_ne!(
        baseline_key,
        SyntaxKeyV1::derive_from_source(
            context.registry_binding(),
            context.snapshot_binding(),
            &file_id,
            SourceSyntaxRole::Scope,
            &alpha.kind,
            scope.range,
        )
    );
    let mut other_binding = context.registry_binding().clone();
    other_binding.registry_hash =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_owned();
    assert_ne!(&other_binding, context.registry_binding());
    assert_ne!(
        baseline_key,
        SyntaxKeyV1::derive_from_source(
            &other_binding,
            context.snapshot_binding(),
            &file_id,
            SourceSyntaxRole::Scope,
            &scope.kind,
            scope.range,
        )
    );
    let other_snapshot = SnapshotBinding::from_admitted_binding("i2-v2-foreign-snapshot@1");
    assert_ne!(&other_snapshot, context.snapshot_binding());
    assert_ne!(
        baseline_key,
        SyntaxKeyV1::derive_from_source(
            context.registry_binding(),
            &other_snapshot,
            &file_id,
            SourceSyntaxRole::Scope,
            &scope.kind,
            scope.range,
        )
    );

    let expected_member_keys = vec![
        SyntaxKeyV1::derive_from_source(
            context.registry_binding(),
            context.snapshot_binding(),
            &file_id,
            SourceSyntaxRole::Callable,
            &alpha.kind,
            alpha.range,
        ),
        SyntaxKeyV1::derive_from_source(
            context.registry_binding(),
            context.snapshot_binding(),
            &file_id,
            SourceSyntaxRole::Callable,
            &beta.kind,
            beta.range,
        ),
    ];
    let submitted = PayloadSubmission {
        locator: file_id.clone(),
        draft: encode_payload_draft(scope),
    };
    let accepted = validate_payload_from_source(&context, submitted)
        .expect("A1 accepts the source-rebuilt two-member scope payload");
    let TypeScriptPayloadData::Scope(scope_data) = &accepted.as_ref().data else {
        panic!("accepted scope payload has ScopeDataV1")
    };
    assert_eq!(scope_data.member_keys.len(), 2);
    assert_eq!(scope_data.member_keys, expected_member_keys);
}

#[test]
fn v2_a1_rejects_scope_member_key_derived_from_a_foreign_snapshot() {
    // The payload is source-rebuilt with A1's context; only one encoded scope
    // member key is replaced with the same source key under another snapshot.
    let context = admitted_callable_scope_context();
    let (_, callable) = source_rebuilt_payload_case(&context, SyntaxRole::Callable);
    let (_, mut scope) = source_rebuilt_payload_case(&context, SyntaxRole::Scope);
    let foreign_snapshot = SnapshotBinding::from_admitted_binding("i2-v2-foreign-snapshot@1");
    let foreign_key = SyntaxKeyV1::derive_from_source(
        context.registry_binding(),
        &foreign_snapshot,
        &file_id("src/main.ts"),
        SourceSyntaxRole::Callable,
        &callable.kind,
        callable.range,
    );
    let TypeScriptPayloadData::Scope(scope_data) = &mut scope.data else {
        panic!("raw scope payload has ScopeDataV1")
    };
    assert_eq!(scope_data.member_keys.len(), 1);
    assert_ne!(scope_data.member_keys[0], foreign_key);
    scope_data.member_keys[0] = foreign_key;
    let submitted = PayloadSubmission {
        locator: file_id("src/main.ts"),
        draft: encode_payload_draft(&scope),
    };
    assert_a1_source_mismatch(
        &context,
        submitted,
        "scope member key from a foreign snapshot",
    );
}

#[test]
fn v2fix_scope_member_keys_cover_the_top_level_role_table() {
    // Each row is a top-level source member; a class
    // member is deliberately nested and therefore contributes no file-scope
    // member key.  Direct exports have both surface and callable records, but
    // the file scope contains the callable member rather than the surface.
    let fixtures = vec![
        (
            "export function",
            "export function f(){}\n",
            vec![(
                SyntaxRole::Callable,
                source_span("export function f(){}\n", "function f(){}"),
            )],
        ),
        (
            "function",
            "function f(){}\n",
            vec![(
                SyntaxRole::Callable,
                source_span("function f(){}\n", "function f(){}"),
            )],
        ),
        (
            "const callable",
            "const f = () => 1;\n",
            vec![(
                SyntaxRole::Callable,
                source_span("const f = () => 1;\n", "f = () => 1"),
            )],
        ),
        (
            "export const callable",
            "export const f = () => 1;\n",
            vec![(
                SyntaxRole::Callable,
                source_span("export const f = () => 1;\n", "f = () => 1"),
            )],
        ),
        (
            "class declaration and nested member",
            "class C { method(){} }\n",
            Vec::new(),
        ),
        (
            "anonymous default callable",
            "export default function(){}\n",
            vec![(
                SyntaxRole::Callable,
                source_span("export default function(){}\n", "function(){}"),
            )],
        ),
        (
            "same-range export surface and callable",
            "export function f(){}\n",
            vec![(
                SyntaxRole::Callable,
                source_span("export function f(){}\n", "function f(){}"),
            )],
        ),
        (
            "re-export only",
            "export { value } from \"./other\";\n",
            vec![(
                SyntaxRole::Surface,
                source_span(
                    "export { value } from \"./other\";\n",
                    "export { value } from \"./other\";",
                ),
            )],
        ),
        (
            "source order across callable const and re-export",
            "function first(){}\nconst second = () => 1;\nexport { value } from \"./other\";\n",
            vec![
                (
                    SyntaxRole::Callable,
                    source_span(
                        "function first(){}\nconst second = () => 1;\nexport { value } from \"./other\";\n",
                        "function first(){}",
                    ),
                ),
                (
                    SyntaxRole::Callable,
                    source_span(
                        "function first(){}\nconst second = () => 1;\nexport { value } from \"./other\";\n",
                        "second = () => 1",
                    ),
                ),
                (
                    SyntaxRole::Surface,
                    source_span(
                        "function first(){}\nconst second = () => 1;\nexport { value } from \"./other\";\n",
                        "export { value } from \"./other\";",
                    ),
                ),
            ],
        ),
    ];

    for (label, source, expected_members) in fixtures {
        let fixture = A0GitFixture::with_target_main(source);
        let context = admit_reconstruction_context(fixture.submitted())
            .expect("A0 admits each call-free member-key source fixture");
        let payloads = source_rebuilt_payloads(&context, source);
        let scope = payloads
            .iter()
            .find(|payload| payload.role == SyntaxRole::Scope)
            .expect("each parsed file has its file-lexical scope record");
        let expected_member_keys = expected_members
            .iter()
            .map(|(role, member_range)| {
                expected_scope_member_key(&context, &payloads, role, member_range)
            })
            .collect::<Vec<_>>();
        let TypeScriptPayloadData::Scope(scope_data) = &scope.data else {
            panic!("{label} has ScopeDataV1")
        };
        assert_eq!(
            scope_data.member_keys.len(),
            expected_member_keys.len(),
            "{label}: scope member count follows the top-level source members"
        );
        assert_eq!(
            scope_data.member_keys, expected_member_keys,
            "{label}: scope member keys retain their role and source order"
        );

        if label == "same-range export surface and callable" {
            // One direct export yields one Surface record over the
            // whole export statement and one Callable record over the
            // declaration, so the member key must come from the role rather
            // than a byte range that both records could share.
            let callable_range = source_span(source, "function f(){}");
            let surface_range = source_span(source, "export function f(){}");
            assert_eq!(
                payloads
                    .iter()
                    .filter(|payload| payload.role == SyntaxRole::Callable
                        && payload.range == callable_range)
                    .count(),
                1,
                "the direct export keeps one callable record at the declaration"
            );
            assert_eq!(
                payloads
                    .iter()
                    .filter(|payload| payload.role == SyntaxRole::Surface
                        && payload.range == surface_range)
                    .count(),
                1,
                "the direct export keeps one surface record over the export statement"
            );
        }
    }
}

#[test]
fn v2fix_containers_do_not_promote_namespace_or_enum_members_to_file_scope() {
    // The design limits retained functions to top-level callables and
    // makes the initial scope file_lexical.  A namespace-contained function
    // and an enum member are not file-level members or public callables.
    for (label, source) in [
        (
            "namespace-contained declaration",
            "namespace Internal { export function nested(){} }\n",
        ),
        ("enum declaration and member", "enum Token { Value }\n"),
    ] {
        let fixture = A0GitFixture::with_target_main(source);
        let context = admit_reconstruction_context(fixture.submitted())
            .expect("A0 admits each container fixture");
        let payloads = source_rebuilt_payloads(&context, source);
        let scope = payloads
            .iter()
            .find(|payload| payload.role == SyntaxRole::Scope)
            .expect("each parsed file has its file-lexical scope record");
        let TypeScriptPayloadData::Scope(scope_data) = &scope.data else {
            panic!("{label} has ScopeDataV1")
        };
        assert!(
            scope_data.member_keys.is_empty(),
            "{label} does not enter the file scope member list"
        );
        assert!(
            !payloads.iter().any(|payload| {
                payload.role == SyntaxRole::Callable
                    && payload.outcome
                        == TypeScriptOutcomeV1::Callable(CallableOutcomeV1::EligiblePublic)
            }),
            "{label} does not become a top-level public callable"
        );
    }
}

#[test]
fn v2fix_snapshot_id_hashes_the_canonical_binding_and_changes_with_it() {
    // The snapshot ID is not the caller's raw binding string.
    let canonical_binding = acceptance_canonical_key(json!({
        "base_tree": "base-tree-a",
        "domain": "source_review_snapshot.v1",
        "target_tree": "target-tree-a",
        "tuple_hash": "tuple-a",
    }));
    let snapshot = SnapshotBinding::from_admitted_binding(&canonical_binding);
    let same_snapshot = SnapshotBinding::from_admitted_binding(&canonical_binding);
    assert_eq!(
        snapshot, same_snapshot,
        "one canonical binding has one snapshot ID"
    );
    assert_ne!(
        snapshot.as_str(),
        canonical_binding,
        "snapshot ID is the canonical binding hash, not its raw serialization"
    );
    assert_eq!(
        snapshot.as_str(),
        ContentHash::sha256(canonical_binding.as_bytes()).as_str(),
        "fixture computes the canonical snapshot-binding hash independently"
    );

    let changed_binding = canonical_binding.replacen("target-tree-a", "target-tree-b", 1);
    let changed_snapshot = SnapshotBinding::from_admitted_binding(&changed_binding);
    assert_ne!(
        snapshot, changed_snapshot,
        "a changed binding produces another snapshot ID"
    );

    let context = admitted_callable_scope_context();
    let payloads = source_rebuilt_payloads(&context, A1_CALLABLE_SCOPE_SOURCE);
    let scope = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Scope)
        .expect("fixture has its scope record");
    let source_file = file_id("src/main.ts");
    let key_for = |binding: &SnapshotBinding| {
        SyntaxKeyV1::derive_from_source(
            context.registry_binding(),
            binding,
            &source_file,
            SourceSyntaxRole::Scope,
            &scope.kind,
            scope.range,
        )
    };
    assert_eq!(key_for(&snapshot), key_for(&same_snapshot));
    assert_ne!(key_for(&snapshot), key_for(&changed_snapshot));
}

#[test]
fn v2fix_callable_visibility_refs_name_the_direct_export_surface_key() {
    // The callable holds no duplicated export
    // payload field; its direct-export witness is the Surface syntax key.
    let source = "export function value(){}\n";
    let fixture = A0GitFixture::with_target_main(source);
    let context = admit_reconstruction_context(fixture.submitted())
        .expect("A0 admits the direct-export visibility fixture");
    let payloads = source_rebuilt_payloads(&context, source);
    let callable_range = source_span(source, "function value(){}");
    let surface_range = source_span(source, "export function value(){}");
    let callable = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Callable && payload.range == callable_range)
        .expect("source has its direct-export callable");
    let surface = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Surface && payload.range == surface_range)
        .expect("source has its direct-export surface");
    let expected_surface_key = SyntaxKeyV1::derive_from_source(
        context.registry_binding(),
        context.snapshot_binding(),
        &file_id("src/main.ts"),
        SourceSyntaxRole::Surface,
        &surface.kind,
        surface_range,
    );
    let TypeScriptPayloadData::Callable(callable_data) = &callable.data else {
        panic!("callable payload has CallableDataV1")
    };
    assert!(
        !callable.refs.visibility_witness_refs.is_empty(),
        "an exported callable has at least one visibility witness"
    );
    assert!(
        callable
            .refs
            .visibility_witness_refs
            .contains(&expected_surface_key),
        "visibility witness refs contain the direct-export Surface key"
    );
    assert!(
        callable_data.binding_obstruction_keys.is_empty(),
        "the direct-export witness belongs in common refs, not callable data"
    );
}

#[test]
fn v2fix_source_mismatch_distinguishes_the_changed_field() {
    // A range and outcome mutation must retain different
    // structured mismatch evidence rather than collapse to one fixed string.
    let context = admitted_callable_scope_context();
    let (submitted, _) = source_rebuilt_payload_case(&context, SyntaxRole::Callable);
    let mut changed_range = submitted.clone();
    changed_range.draft.range = range(1, A1_CALLABLE_SCOPE_SOURCE.len() as u64 - 1);
    let range_detail = source_mismatch_detail(&context, changed_range);
    assert!(
        !range_detail.missing.is_empty() || !range_detail.extra.is_empty(),
        "range mismatch names source-derived difference entries"
    );

    let mut changed_outcome = submitted;
    changed_outcome.draft.outcome = TypeScriptOutcomeV1::Callable(CallableOutcomeV1::NonPublic);
    let outcome_detail = source_mismatch_detail(&context, changed_outcome);
    assert!(
        !outcome_detail.missing.is_empty() || !outcome_detail.extra.is_empty(),
        "outcome mismatch names source-derived difference entries"
    );
    assert_ne!(
        range_detail, outcome_detail,
        "SourceMismatch retains which independently changed field disagreed"
    );
}

/// Reads a fixed file from the fixture's immutable target commit. This is kept
/// local to the two-file binding authority case so its bytes do not inherit an
/// A0 projection or the shared main-only A1 fixture.
fn literal_target_file_bytes(root: &Path, target_commit: &str, path: &str) -> Vec<u8> {
    let inherited_path = std::env::var_os("PATH");
    let revision = format!("{target_commit}:{path}");
    let mut command = Command::new("git");
    command
        .env_clear()
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("LC_ALL", "C")
        .args(["--no-replace-objects", "show", &revision]);
    if let Some(path) = inherited_path {
        command.env("PATH", path);
    }
    let output = command
        .output()
        .expect("read literal target file from the two-file fixture");
    assert!(
        output.status.success(),
        "literal target Git file read succeeds for {revision}"
    );
    output.stdout
}

/// The finite source authority for the M2 Binding rows.  Every caller and
/// optional dependency byte sequence is named by the invoking test; this
/// helper only applies the common Parsed-basis admission rule.
fn binding_major2_sources(
    main_source: &str,
    dependency_source: Option<&str>,
) -> AdmittedSourceBundleV1 {
    let mut files = vec![SourceReviewFileV1 {
        path: "src/main.ts".to_owned(),
        language: "typescript".to_owned(),
        outcome: SourceFileOutcome::Parsed,
    }];
    if dependency_source.is_some() {
        files.push(SourceReviewFileV1 {
            path: "src/dependency.ts".to_owned(),
            language: "typescript".to_owned(),
            outcome: SourceFileOutcome::Parsed,
        });
    }
    let basis = SourceReviewBasisV1::new(typescript_registry_binding(), files, Vec::new())
        .expect("each major-2 source row fixes its Parsed basis rows");
    let mut admitted = vec![AdmittedSourceFileV1 {
        file_id: file_id("src/main.ts"),
        bytes: main_source.as_bytes().to_vec(),
        source_hash: SourceHash::from_source_bytes(main_source.as_bytes()),
    }];
    if let Some(dependency_source) = dependency_source {
        admitted.push(AdmittedSourceFileV1 {
            file_id: file_id("src/dependency.ts"),
            bytes: dependency_source.as_bytes().to_vec(),
            source_hash: SourceHash::from_source_bytes(dependency_source.as_bytes()),
        });
    }
    AdmittedSourceBundleV1::new(&basis, admitted)
        .expect("each major-2 source row supplies exactly its admitted bytes")
}

fn binding_major2_payloads(sources: &AdmittedSourceBundleV1) -> Vec<TypeScriptPayload> {
    rebuild_payload_from_source(
        sources,
        &typescript_registry_binding(),
        &SnapshotBinding::from_admitted_binding("v2_binding_major2_acceptance@1"),
        file_id("src/main.ts"),
    )
    .expect("the admitted major-2 caller source rebuilds")
}

fn binding_major2_stages(wires: &[&str]) -> Vec<BindingStageV1> {
    wires
        .iter()
        .map(|wire| {
            BindingStageV1::parse_wire(&typescript_registry_binding(), wire)
                .expect("the fixed Binding stage is registered")
        })
        .collect()
}

fn binding_major2_reasons(wires: &[&str]) -> BindingReasonSetV1 {
    BindingReasonSetV1::new(
        &typescript_registry_binding(),
        wires
            .iter()
            .map(|wire| {
                BindingReasonV1::parse_wire(&typescript_registry_binding(), wire)
                    .expect("the fixed Binding reason is registered")
            })
            .collect(),
    )
    .expect("the fixed Binding reason set is registered")
}

fn binding_major2_dependency_candidates() -> Vec<CandidatePathV1> {
    vec![
        CandidatePathV1 {
            path: "src/dependency".to_owned(),
            file_key: None,
            rejection_witness: false,
            unexpanded_ancestor_key: None,
        },
        CandidatePathV1 {
            path: "src/dependency.ts".to_owned(),
            file_key: Some(file_id("src/dependency.ts")),
            rejection_witness: false,
            unexpanded_ancestor_key: None,
        },
        CandidatePathV1 {
            path: "src/dependency.tsx".to_owned(),
            file_key: None,
            rejection_witness: false,
            unexpanded_ancestor_key: None,
        },
        CandidatePathV1 {
            path: "src/dependency/index.ts".to_owned(),
            file_key: None,
            rejection_witness: false,
            unexpanded_ancestor_key: None,
        },
        CandidatePathV1 {
            path: "src/dependency/index.tsx".to_owned(),
            file_key: None,
            rejection_witness: false,
            unexpanded_ancestor_key: None,
        },
    ]
}

/// This is deliberately the first assertion after every source rebuild.  Its
/// denominator is fixed by the calling test's literal static imports, never by
/// the producer/collector being tested.
fn assert_binding_major2_conservation(
    payloads: &[TypeScriptPayload],
    expected: &[(SourceRange, &str, PayloadImportKindV1)],
) {
    let bindings = payloads
        .iter()
        .filter(|payload| payload.role == SyntaxRole::Binding)
        .collect::<Vec<_>>();
    assert_eq!(
        bindings.len(),
        expected.len(),
        "every independently fixed static-import Binding is retained exactly once"
    );
    for (payload, (expected_range, expected_local_name, expected_kind)) in
        bindings.into_iter().zip(expected)
    {
        let TypeScriptPayloadData::Binding(data) = &payload.data else {
            panic!("a Binding role has BindingDataV1")
        };
        assert_eq!(
            payload.range, *expected_range,
            "Binding keeps its fixed source range"
        );
        assert_eq!(
            data.local_name, *expected_local_name,
            "Binding keeps its fixed local name"
        );
        assert_eq!(
            data.import_kind, *expected_kind,
            "Binding keeps its fixed import form"
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn assert_binding_major2_fields(
    binding: &TypeScriptPayload,
    expected_range: SourceRange,
    expected_local_name: &str,
    expected_kind: PayloadImportKindV1,
    expected_syntax_kind: &str,
    expected_export_slot: Option<&str>,
    expected_specifier: &str,
    expected_specifier_range: SourceRange,
    expected_candidates: Vec<CandidatePathV1>,
    expected_outcome: ResolutionOutcomeV1,
    expected_reason_wires: &[&str],
    expected_stage_wires: &[&str],
    expected_endpoint: Option<DeclarationId>,
) {
    let TypeScriptPayloadData::Binding(data) = &binding.data else {
        panic!("the conserved Binding role has BindingDataV1")
    };
    assert_eq!(binding.role, SyntaxRole::Binding);
    assert_eq!(
        binding.kind,
        reviewgraphen_core::source_review::reasons::TypeScriptSyntaxKind::parse_wire(
            expected_syntax_kind,
        )
        .expect("the caller-fixed import form has a registered syntax kind"),
    );
    assert_eq!(binding.range, expected_range);
    assert_eq!(data.local_name, expected_local_name);
    assert_eq!(data.import_kind, expected_kind);
    assert_eq!(
        data.export_slot.as_ref().map(|slot| slot.wire_literal()),
        expected_export_slot
    );
    assert_eq!(data.specifier, expected_specifier);
    assert_eq!(data.specifier_range, expected_specifier_range);
    assert_eq!(data.candidate_paths, expected_candidates);
    assert_eq!(
        binding.outcome,
        TypeScriptOutcomeV1::Binding(expected_outcome)
    );
    let expected_reasons = binding_major2_reasons(expected_reason_wires);
    assert_eq!(
        binding.reasons,
        TypeScriptReasonsV1::Binding(expected_reasons.clone())
    );
    assert_eq!(
        binding.primary_reason,
        expected_reasons
            .primary()
            .cloned()
            .map(TypeScriptPrimaryReasonV1::Binding)
    );
    assert_eq!(
        data.evaluated_stages,
        binding_major2_stages(expected_stage_wires)
    );
    assert_eq!(data.resolved_function_id, expected_endpoint);
}

fn assert_binding_major2_fixed_nonbinding_rows(
    payloads: &[TypeScriptPayload],
    source: &str,
    expected_surface_range: Option<SourceRange>,
) {
    let surfaces = payloads
        .iter()
        .filter(|payload| payload.role == SyntaxRole::Surface)
        .collect::<Vec<_>>();
    assert_eq!(
        surfaces.len(),
        usize::from(expected_surface_range.is_some())
    );
    if let Some(expected_surface_range) = expected_surface_range {
        let surface = surfaces
            .into_iter()
            .next()
            .expect("the literal export keeps its one Surface row");
        assert_eq!(surface.range, expected_surface_range);
        assert_eq!(
            surface.outcome,
            TypeScriptOutcomeV1::Surface(RecordOutcomeV1::Recorded)
        );
        assert!(matches!(
            &surface.reasons,
            TypeScriptReasonsV1::Surface(reasons) if reasons.is_empty()
        ));
        assert_eq!(surface.primary_reason, None);
        let TypeScriptPayloadData::Surface(data) = &surface.data else {
            panic!("the fixed Surface row has SurfaceDataV1")
        };
        assert_eq!(
            data.export_slots
                .iter()
                .map(|slot| slot.wire_literal())
                .collect::<Vec<_>>(),
            vec!["value"]
        );
        assert_eq!(data.local_names, vec!["value"]);
        assert_eq!(data.target_specifier, None);
    }

    let scopes = payloads
        .iter()
        .filter(|payload| payload.role == SyntaxRole::Scope)
        .collect::<Vec<_>>();
    assert_eq!(scopes.len(), 1);
    let scope = scopes[0];
    assert_eq!(scope.range, range(0, source.len() as u64));
    assert_eq!(
        scope.outcome,
        TypeScriptOutcomeV1::Scope(RecordOutcomeV1::Recorded)
    );
    assert!(matches!(
        &scope.reasons,
        TypeScriptReasonsV1::Scope(reasons) if reasons.is_empty()
    ));
    assert_eq!(scope.primary_reason, None);
    let TypeScriptPayloadData::Scope(data) = &scope.data else {
        panic!("the fixed file scope has ScopeDataV1")
    };
    assert_eq!(data.scope_kind, ScopeKindV1::FileLexical);
    assert_eq!(
        data.member_keys.len(),
        1 + usize::from(expected_surface_range.is_some())
    );
    assert_eq!(
        payloads
            .iter()
            .filter(|payload| payload.role != SyntaxRole::Binding)
            .count(),
        1 + usize::from(expected_surface_range.is_some())
    );
}

#[test]
fn v2_binding_major2_local_conflict_is_retained_at_local_uniqueness() {
    let source = "import { localConflict } from \"./dependency\";\nconst localConflict = 1;\n";
    let dependency_source = "export function localConflict(){return 1;}\n";
    let sources = binding_major2_sources(source, Some(dependency_source));
    let payloads = binding_major2_payloads(&sources);
    let local_range = range(
        "import { ".len() as u64,
        ("import { ".len() + "localConflict".len()) as u64,
    );
    assert_binding_major2_conservation(
        &payloads,
        &[(local_range, "localConflict", PayloadImportKindV1::Named)],
    );
    let binding = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Binding)
        .expect("the conserved local-conflict Binding is present");
    assert_binding_major2_fields(
        binding,
        local_range,
        "localConflict",
        PayloadImportKindV1::Named,
        "import_specifier",
        Some("localConflict"),
        "./dependency",
        source_span(source, "\"./dependency\""),
        Vec::new(),
        ResolutionOutcomeV1::Unresolved,
        &["import_binding_ambiguous"],
        &["b.form", "b.local_uniqueness"],
        None,
    );
}

#[test]
fn v2_binding_major2_caller_write_is_retained_through_writes() {
    let source = "import { callerWrite } from \"./dependency\";\ncallerWrite = () => 1;\n";
    let dependency_source = "export function callerWrite(){return 1;}\n";
    let sources = binding_major2_sources(source, Some(dependency_source));
    let payloads = binding_major2_payloads(&sources);
    let local_range = range(
        "import { ".len() as u64,
        ("import { ".len() + "callerWrite".len()) as u64,
    );
    assert_binding_major2_conservation(
        &payloads,
        &[(local_range, "callerWrite", PayloadImportKindV1::Named)],
    );
    let binding = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Binding)
        .expect("the conserved caller-write Binding is present");
    assert_binding_major2_fields(
        binding,
        local_range,
        "callerWrite",
        PayloadImportKindV1::Named,
        "import_specifier",
        Some("callerWrite"),
        "./dependency",
        source_span(source, "\"./dependency\""),
        binding_major2_dependency_candidates(),
        ResolutionOutcomeV1::Unresolved,
        &["written_binding"],
        &[
            "b.form",
            "b.local_uniqueness",
            "b.specifier",
            "b.candidates",
            "b.export_binding",
            "b.writes",
        ],
        None,
    );
}

#[test]
fn v2_binding_major2_callee_write_is_retained_through_writes() {
    let source = "import { calleeWrite } from \"./dependency\";\nexport const value = 1;\n";
    let dependency_source = "export function calleeWrite(){return 1;}\ncalleeWrite = () => 2;\n";
    let sources = binding_major2_sources(source, Some(dependency_source));
    let payloads = binding_major2_payloads(&sources);
    let local_range = range(
        "import { ".len() as u64,
        ("import { ".len() + "calleeWrite".len()) as u64,
    );
    assert_binding_major2_conservation(
        &payloads,
        &[(local_range, "calleeWrite", PayloadImportKindV1::Named)],
    );
    let binding = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Binding)
        .expect("the conserved callee-write Binding is present");
    assert_binding_major2_fields(
        binding,
        local_range,
        "calleeWrite",
        PayloadImportKindV1::Named,
        "import_specifier",
        Some("calleeWrite"),
        "./dependency",
        source_span(source, "\"./dependency\""),
        binding_major2_dependency_candidates(),
        ResolutionOutcomeV1::Unresolved,
        &["written_binding"],
        &[
            "b.form",
            "b.local_uniqueness",
            "b.specifier",
            "b.candidates",
            "b.export_binding",
            "b.writes",
        ],
        None,
    );
}

#[test]
fn v2_binding_major2_default_resolved_reaches_result() {
    let source = "import defaultCallable from \"./dependency\";\nexport const value = 1;\n";
    let dependency_source = "export default function defaultCallable(){return 1;}\n";
    let sources = binding_major2_sources(source, Some(dependency_source));
    let payloads = binding_major2_payloads(&sources);
    let local_range = source_span(source, "defaultCallable");
    assert_binding_major2_conservation(
        &payloads,
        &[(local_range, "defaultCallable", PayloadImportKindV1::Default)],
    );
    let binding = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Binding)
        .expect("the conserved default-import Binding is present");
    assert_binding_major2_fields(
        binding,
        local_range,
        "defaultCallable",
        PayloadImportKindV1::Default,
        "identifier",
        Some("default"),
        "./dependency",
        source_span(source, "\"./dependency\""),
        binding_major2_dependency_candidates(),
        ResolutionOutcomeV1::Resolved,
        &[],
        &[
            "b.form",
            "b.local_uniqueness",
            "b.specifier",
            "b.candidates",
            "b.export_binding",
            "b.writes",
            "b.result",
        ],
        Some(DeclarationId::from_source(
            file_id("src/dependency.ts"),
            source_span(dependency_source, "function defaultCallable(){return 1;}"),
        )),
    );
}

#[test]
fn v2_binding_major2_type_only_is_retained_at_form() {
    let source = "import type { TypeOnly } from \"./dependency\";\nexport const value = 1;\n";
    let dependency_source = "export type TypeOnly = number;\n";
    let sources = binding_major2_sources(source, Some(dependency_source));
    let payloads = binding_major2_payloads(&sources);
    let local_range = source_span(source, "TypeOnly");
    assert_binding_major2_conservation(
        &payloads,
        &[(local_range, "TypeOnly", PayloadImportKindV1::TypeOnly)],
    );
    let binding = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Binding)
        .expect("the conserved type-only Binding is present");
    assert_binding_major2_fields(
        binding,
        local_range,
        "TypeOnly",
        PayloadImportKindV1::TypeOnly,
        "import_specifier",
        Some("TypeOnly"),
        "./dependency",
        source_span(source, "\"./dependency\""),
        Vec::new(),
        ResolutionOutcomeV1::Unresolved,
        &["type_only_binding"],
        &["b.form"],
        None,
    );
}

#[test]
fn v2_binding_major2_namespace_is_retained_at_form() {
    let source = "import * as namespaceBinding from \"./dependency\";\nexport const value = 1;\n";
    let dependency_source = "export function dependency(){return 1;}\n";
    let sources = binding_major2_sources(source, Some(dependency_source));
    let payloads = binding_major2_payloads(&sources);
    let local_range = source_span(source, "* as namespaceBinding");
    assert_binding_major2_conservation(
        &payloads,
        &[(
            local_range,
            "namespaceBinding",
            PayloadImportKindV1::Namespace,
        )],
    );
    let binding = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Binding)
        .expect("the conserved namespace Binding is present");
    assert_binding_major2_fields(
        binding,
        local_range,
        "namespaceBinding",
        PayloadImportKindV1::Namespace,
        "namespace_import",
        None,
        "./dependency",
        source_span(source, "\"./dependency\""),
        Vec::new(),
        ResolutionOutcomeV1::Unresolved,
        &["import_resolution_unavailable"],
        &["b.form"],
        None,
    );
}

#[test]
fn v2_binding_major2_root_escaping_relative_specifier_stops_at_specifier() {
    let source = "import { rootEscape } from \"../../dependency\";\nexport const value = 1;\n";
    let sources = binding_major2_sources(source, None);
    let payloads = binding_major2_payloads(&sources);
    let local_range = source_span(source, "rootEscape");
    assert_binding_major2_conservation(
        &payloads,
        &[(local_range, "rootEscape", PayloadImportKindV1::Named)],
    );
    let binding = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Binding)
        .expect("the conserved root-escaping Binding is present");
    assert_binding_major2_fields(
        binding,
        local_range,
        "rootEscape",
        PayloadImportKindV1::Named,
        "import_specifier",
        Some("rootEscape"),
        "../../dependency",
        source_span(source, "\"../../dependency\""),
        Vec::new(),
        ResolutionOutcomeV1::Unresolved,
        &["relative_specifier_unsupported"],
        &["b.form", "b.local_uniqueness", "b.specifier"],
        None,
    );
}

#[test]
fn v2_binding_major2_export_slot_failure_is_retained_at_export_binding() {
    let source = "import { missingExport } from \"./dependency\";\nexport const value = 1;\n";
    let dependency_source = "export function anotherExport(){return 1;}\n";
    let sources = binding_major2_sources(source, Some(dependency_source));
    let payloads = binding_major2_payloads(&sources);
    let local_range = source_span(source, "missingExport");
    assert_binding_major2_conservation(
        &payloads,
        &[(local_range, "missingExport", PayloadImportKindV1::Named)],
    );
    let binding = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Binding)
        .expect("the conserved export-slot Binding is present");
    assert_binding_major2_fields(
        binding,
        local_range,
        "missingExport",
        PayloadImportKindV1::Named,
        "import_specifier",
        Some("missingExport"),
        "./dependency",
        source_span(source, "\"./dependency\""),
        binding_major2_dependency_candidates(),
        ResolutionOutcomeV1::Unresolved,
        &["export_binding_unsupported"],
        &[
            "b.form",
            "b.local_uniqueness",
            "b.specifier",
            "b.candidates",
            "b.export_binding",
        ],
        None,
    );
}

#[test]
fn v2_binding_scope_caller_update_is_retained_through_writes() {
    let source = "import { callerUpdate } from \"./dependency\";\ncallerUpdate++;\n";
    let dependency_source = "export function callerUpdate(){return 1;}\n";
    let sources = binding_major2_sources(source, Some(dependency_source));
    let payloads = binding_major2_payloads(&sources);
    let local_range = range(
        "import { ".len() as u64,
        ("import { ".len() + "callerUpdate".len()) as u64,
    );
    assert_binding_major2_conservation(
        &payloads,
        &[(local_range, "callerUpdate", PayloadImportKindV1::Named)],
    );
    let binding = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Binding)
        .expect("the conserved caller-update Binding is present");
    assert_binding_major2_fields(
        binding,
        local_range,
        "callerUpdate",
        PayloadImportKindV1::Named,
        "import_specifier",
        Some("callerUpdate"),
        "./dependency",
        source_span(source, "\"./dependency\""),
        binding_major2_dependency_candidates(),
        ResolutionOutcomeV1::Unresolved,
        &["written_binding"],
        &[
            "b.form",
            "b.local_uniqueness",
            "b.specifier",
            "b.candidates",
            "b.export_binding",
            "b.writes",
        ],
        None,
    );
}

#[test]
fn v2_binding_scope_callee_update_is_retained_through_writes() {
    let source = "import { calleeUpdate } from \"./dependency\";\nexport const value = 1;\n";
    let dependency_source = "export function calleeUpdate(){return 1;}\n++calleeUpdate;\n";
    let sources = binding_major2_sources(source, Some(dependency_source));
    let payloads = binding_major2_payloads(&sources);
    let local_range = source_span(source, "calleeUpdate");
    assert_binding_major2_conservation(
        &payloads,
        &[(local_range, "calleeUpdate", PayloadImportKindV1::Named)],
    );
    let binding = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Binding)
        .expect("the conserved callee-update Binding is present");
    assert_binding_major2_fields(
        binding,
        local_range,
        "calleeUpdate",
        PayloadImportKindV1::Named,
        "import_specifier",
        Some("calleeUpdate"),
        "./dependency",
        source_span(source, "\"./dependency\""),
        binding_major2_dependency_candidates(),
        ResolutionOutcomeV1::Unresolved,
        &["written_binding"],
        &[
            "b.form",
            "b.local_uniqueness",
            "b.specifier",
            "b.candidates",
            "b.export_binding",
            "b.writes",
        ],
        None,
    );
}

#[test]
fn v2_binding_scope_caller_lexical_shadow_remains_resolved() {
    let source = "import { callerShadow } from \"./dependency\";\nfunction nested(callerShadow){callerShadow = 1;}\n";
    let dependency_source = "export function callerShadow(){return 1;}\n";
    let sources = binding_major2_sources(source, Some(dependency_source));
    let payloads = binding_major2_payloads(&sources);
    let local_range = range(
        "import { ".len() as u64,
        ("import { ".len() + "callerShadow".len()) as u64,
    );
    assert_binding_major2_conservation(
        &payloads,
        &[(local_range, "callerShadow", PayloadImportKindV1::Named)],
    );
    let binding = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Binding)
        .expect("the conserved caller-shadow Binding is present");
    assert_binding_major2_fields(
        binding,
        local_range,
        "callerShadow",
        PayloadImportKindV1::Named,
        "import_specifier",
        Some("callerShadow"),
        "./dependency",
        source_span(source, "\"./dependency\""),
        binding_major2_dependency_candidates(),
        ResolutionOutcomeV1::Resolved,
        &[],
        &[
            "b.form",
            "b.local_uniqueness",
            "b.specifier",
            "b.candidates",
            "b.export_binding",
            "b.writes",
            "b.result",
        ],
        Some(DeclarationId::from_source(
            file_id("src/dependency.ts"),
            source_span(dependency_source, "function callerShadow(){return 1;}"),
        )),
    );
}

#[test]
fn v2_binding_scope_callee_lexical_shadow_remains_resolved() {
    let source = "import { calleeShadow } from \"./dependency\";\nexport const value = 1;\n";
    let dependency_source = "export function calleeShadow(){return 1;}\nfunction nested(calleeShadow){calleeShadow = 1;}\n";
    let sources = binding_major2_sources(source, Some(dependency_source));
    let payloads = binding_major2_payloads(&sources);
    let local_range = source_span(source, "calleeShadow");
    assert_binding_major2_conservation(
        &payloads,
        &[(local_range, "calleeShadow", PayloadImportKindV1::Named)],
    );
    let binding = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Binding)
        .expect("the conserved callee-shadow Binding is present");
    assert_binding_major2_fields(
        binding,
        local_range,
        "calleeShadow",
        PayloadImportKindV1::Named,
        "import_specifier",
        Some("calleeShadow"),
        "./dependency",
        source_span(source, "\"./dependency\""),
        binding_major2_dependency_candidates(),
        ResolutionOutcomeV1::Resolved,
        &[],
        &[
            "b.form",
            "b.local_uniqueness",
            "b.specifier",
            "b.candidates",
            "b.export_binding",
            "b.writes",
            "b.result",
        ],
        Some(DeclarationId::from_source(
            file_id("src/dependency.ts"),
            source_span(dependency_source, "function calleeShadow(){return 1;}"),
        )),
    );
}

#[test]
fn v2_binding_scope2_caller_single_parameter_arrow_shadow_remains_resolved() {
    let source = "import { x } from \"./dependency\";\nx => { x = replacement; };\n";
    let dependency_source = "export function x(){return 1;}\n";
    let sources = binding_major2_sources(source, Some(dependency_source));
    let payloads = binding_major2_payloads(&sources);
    let local_range = range("import { ".len() as u64, ("import { ".len() + 1) as u64);
    assert_binding_major2_conservation(
        &payloads,
        &[(local_range, "x", PayloadImportKindV1::Named)],
    );
    let binding = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Binding)
        .expect("the conserved caller single-parameter arrow Binding is present");
    assert_binding_major2_fields(
        binding,
        local_range,
        "x",
        PayloadImportKindV1::Named,
        "import_specifier",
        Some("x"),
        "./dependency",
        source_span(source, "\"./dependency\""),
        binding_major2_dependency_candidates(),
        ResolutionOutcomeV1::Resolved,
        &[],
        &[
            "b.form",
            "b.local_uniqueness",
            "b.specifier",
            "b.candidates",
            "b.export_binding",
            "b.writes",
            "b.result",
        ],
        Some(DeclarationId::from_source(
            file_id("src/dependency.ts"),
            source_span(dependency_source, "function x(){return 1;}"),
        )),
    );
    assert_binding_major2_fixed_nonbinding_rows(&payloads, source, None);
}

#[test]
fn v2_binding_scope2_callee_single_parameter_arrow_shadow_remains_resolved() {
    let source = "import { x } from \"./dependency\";\nexport const value = 1;\n";
    let dependency_source = "export function x(){return 1;}\nx => { x = replacement; };\n";
    let sources = binding_major2_sources(source, Some(dependency_source));
    let payloads = binding_major2_payloads(&sources);
    let local_range = range("import { ".len() as u64, ("import { ".len() + 1) as u64);
    assert_binding_major2_conservation(
        &payloads,
        &[(local_range, "x", PayloadImportKindV1::Named)],
    );
    let binding = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Binding)
        .expect("the conserved callee single-parameter arrow Binding is present");
    assert_binding_major2_fields(
        binding,
        local_range,
        "x",
        PayloadImportKindV1::Named,
        "import_specifier",
        Some("x"),
        "./dependency",
        source_span(source, "\"./dependency\""),
        binding_major2_dependency_candidates(),
        ResolutionOutcomeV1::Resolved,
        &[],
        &[
            "b.form",
            "b.local_uniqueness",
            "b.specifier",
            "b.candidates",
            "b.export_binding",
            "b.writes",
            "b.result",
        ],
        Some(DeclarationId::from_source(
            file_id("src/dependency.ts"),
            source_span(dependency_source, "function x(){return 1;}"),
        )),
    );
    assert_binding_major2_fixed_nonbinding_rows(
        &payloads,
        source,
        Some(source_span(source, "export const value = 1;")),
    );
}

#[test]
fn v2_binding_scope2_caller_method_parameter_shadow_remains_resolved() {
    let source = "import { x } from \"./dependency\";\n({ method(x) { x = replacement; } });\n";
    let dependency_source = "export function x(){return 1;}\n";
    let sources = binding_major2_sources(source, Some(dependency_source));
    let payloads = binding_major2_payloads(&sources);
    let local_range = range("import { ".len() as u64, ("import { ".len() + 1) as u64);
    assert_binding_major2_conservation(
        &payloads,
        &[(local_range, "x", PayloadImportKindV1::Named)],
    );
    let binding = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Binding)
        .expect("the conserved caller method-parameter Binding is present");
    assert_binding_major2_fields(
        binding,
        local_range,
        "x",
        PayloadImportKindV1::Named,
        "import_specifier",
        Some("x"),
        "./dependency",
        source_span(source, "\"./dependency\""),
        binding_major2_dependency_candidates(),
        ResolutionOutcomeV1::Resolved,
        &[],
        &[
            "b.form",
            "b.local_uniqueness",
            "b.specifier",
            "b.candidates",
            "b.export_binding",
            "b.writes",
            "b.result",
        ],
        Some(DeclarationId::from_source(
            file_id("src/dependency.ts"),
            source_span(dependency_source, "function x(){return 1;}"),
        )),
    );
    assert_binding_major2_fixed_nonbinding_rows(&payloads, source, None);
}

#[test]
fn v2_binding_scope2_callee_method_parameter_shadow_remains_resolved() {
    let source = "import { x } from \"./dependency\";\nexport const value = 1;\n";
    let dependency_source =
        "export function x(){return 1;}\n({ method(x) { x = replacement; } });\n";
    let sources = binding_major2_sources(source, Some(dependency_source));
    let payloads = binding_major2_payloads(&sources);
    let local_range = range("import { ".len() as u64, ("import { ".len() + 1) as u64);
    assert_binding_major2_conservation(
        &payloads,
        &[(local_range, "x", PayloadImportKindV1::Named)],
    );
    let binding = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Binding)
        .expect("the conserved callee method-parameter Binding is present");
    assert_binding_major2_fields(
        binding,
        local_range,
        "x",
        PayloadImportKindV1::Named,
        "import_specifier",
        Some("x"),
        "./dependency",
        source_span(source, "\"./dependency\""),
        binding_major2_dependency_candidates(),
        ResolutionOutcomeV1::Resolved,
        &[],
        &[
            "b.form",
            "b.local_uniqueness",
            "b.specifier",
            "b.candidates",
            "b.export_binding",
            "b.writes",
            "b.result",
        ],
        Some(DeclarationId::from_source(
            file_id("src/dependency.ts"),
            source_span(dependency_source, "function x(){return 1;}"),
        )),
    );
    assert_binding_major2_fixed_nonbinding_rows(
        &payloads,
        source,
        Some(source_span(source, "export const value = 1;")),
    );
}

#[test]
fn v2_binding_scope2_caller_default_initializer_reference_with_write_is_unresolved() {
    let source = "import { x } from \"./dependency\";\nif (true) { function nested(arg = x) { x = replacement; } }\n";
    let dependency_source = "export function x(){return 1;}\n";
    let sources = binding_major2_sources(source, Some(dependency_source));
    let payloads = binding_major2_payloads(&sources);
    let local_range = range("import { ".len() as u64, ("import { ".len() + 1) as u64);
    assert_binding_major2_conservation(
        &payloads,
        &[(local_range, "x", PayloadImportKindV1::Named)],
    );
    let binding = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Binding)
        .expect("the conserved caller default-initializer Binding is present");
    assert_binding_major2_fields(
        binding,
        local_range,
        "x",
        PayloadImportKindV1::Named,
        "import_specifier",
        Some("x"),
        "./dependency",
        source_span(source, "\"./dependency\""),
        binding_major2_dependency_candidates(),
        ResolutionOutcomeV1::Unresolved,
        &["written_binding"],
        &[
            "b.form",
            "b.local_uniqueness",
            "b.specifier",
            "b.candidates",
            "b.export_binding",
            "b.writes",
        ],
        None,
    );
    assert_binding_major2_fixed_nonbinding_rows(&payloads, source, None);
}

#[test]
fn v2_binding_scope2_callee_default_initializer_reference_with_write_is_unresolved() {
    let source = "import { x } from \"./dependency\";\nexport const value = 1;\n";
    let dependency_source = "export function x(){return 1;}\nif (true) { function nested(arg = x) { x = replacement; } }\n";
    let sources = binding_major2_sources(source, Some(dependency_source));
    let payloads = binding_major2_payloads(&sources);
    let local_range = range("import { ".len() as u64, ("import { ".len() + 1) as u64);
    assert_binding_major2_conservation(
        &payloads,
        &[(local_range, "x", PayloadImportKindV1::Named)],
    );
    let binding = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Binding)
        .expect("the conserved callee default-initializer Binding is present");
    assert_binding_major2_fields(
        binding,
        local_range,
        "x",
        PayloadImportKindV1::Named,
        "import_specifier",
        Some("x"),
        "./dependency",
        source_span(source, "\"./dependency\""),
        binding_major2_dependency_candidates(),
        ResolutionOutcomeV1::Unresolved,
        &["written_binding"],
        &[
            "b.form",
            "b.local_uniqueness",
            "b.specifier",
            "b.candidates",
            "b.export_binding",
            "b.writes",
        ],
        None,
    );
    assert_binding_major2_fixed_nonbinding_rows(
        &payloads,
        source,
        Some(source_span(source, "export const value = 1;")),
    );
}

#[test]
fn v2_binding_producer_rebuilds_the_named_static_import_from_source() {
    // The literal has every initial role:
    // one named static import and two call-free direct exports. The expected
    // source fields below are calculated from that literal, not copied from a
    // draft.
    let source = "import { dependency } from \"./dependency\";\n\
export function first(){return 1;}\n\
export function second(){return 2;}\n";
    let dependency_source = "export function dependency(){return 1;}\n";
    let fixture = A0GitFixture::with_target_main(source);
    let main_bytes = literal_target_file_bytes(
        &fixture.request.repository_admission_root,
        &fixture.target_commit,
        "src/main.ts",
    );
    let dependency_bytes = literal_target_file_bytes(
        &fixture.request.repository_admission_root,
        &fixture.target_commit,
        "src/dependency.ts",
    );
    assert_eq!(main_bytes, source.as_bytes());
    assert_eq!(dependency_bytes, dependency_source.as_bytes());

    let submitted = fixture.submitted();
    let dependency_inventory = submitted
        .material
        .target_inventory
        .iter()
        .find(|entry| entry.path == "src/dependency.ts")
        .expect("A0 target inventory includes the named-import dependency");
    assert_eq!(dependency_inventory.profile, SourceProfileClaimV1::Included);
    assert_eq!(dependency_inventory.outcome, SourceFileOutcome::Parsed);
    match &dependency_inventory.read {
        SourceReadClaimV1::Complete {
            byte_count,
            source_hash,
        } => {
            assert_eq!(*byte_count, dependency_bytes.len() as u64);
            assert_eq!(
                source_hash,
                &SourceHash::from_source_bytes(&dependency_bytes)
            );
        }
        other => panic!("dependency has a complete target read, got {other:?}"),
    }
    let dependency_material = submitted
        .material
        .target_read_material
        .iter()
        .find(|material| material.path == "src/dependency.ts")
        .expect("A0 target read material retains the named-import dependency");
    assert_eq!(dependency_material.extent, SourceReadExtentV1::FullBlob);
    assert_eq!(dependency_material.bytes, dependency_bytes);
    assert_eq!(
        dependency_material.hash,
        SourceHash::from_source_bytes(&dependency_bytes)
    );
    assert!(
        submitted
            .material
            .target_basis
            .files
            .iter()
            .any(|file| file.path == "src/dependency.ts"),
        "A0 target basis retains the dependency path"
    );

    let context = admit_reconstruction_context(submitted)
        .expect("A0 admits the named-binding producer fixture");
    let basis = SourceReviewBasisV1::new(
        typescript_registry_binding(),
        vec![
            SourceReviewFileV1 {
                path: "src/main.ts".to_owned(),
                language: "typescript".to_owned(),
                outcome: SourceFileOutcome::Parsed,
            },
            SourceReviewFileV1 {
                path: "src/dependency.ts".to_owned(),
                language: "typescript".to_owned(),
                outcome: SourceFileOutcome::Parsed,
            },
        ],
        Vec::new(),
    )
    .expect("two-file binding fixture has parsed source basis");
    let sources = AdmittedSourceBundleV1::new(
        &basis,
        vec![
            AdmittedSourceFileV1 {
                file_id: file_id("src/main.ts"),
                source_hash: SourceHash::from_source_bytes(&main_bytes),
                bytes: main_bytes,
            },
            AdmittedSourceFileV1 {
                file_id: file_id("src/dependency.ts"),
                source_hash: SourceHash::from_source_bytes(&dependency_bytes),
                bytes: dependency_bytes,
            },
        ],
    )
    .expect("two-file binding fixture admits immutable target bytes");
    let payloads = rebuild_payload_from_source(
        &sources,
        context.registry_binding(),
        context.snapshot_binding(),
        file_id("src/main.ts"),
    )
    .expect("raw rebuild reads the two-file named-binding fixture");

    assert_eq!(
        payloads.len(),
        6,
        "the call-free literal has six typed payloads"
    );
    for (role, expected_count) in [
        (SyntaxRole::Callable, 2),
        (SyntaxRole::Call, 0),
        (SyntaxRole::Binding, 1),
        (SyntaxRole::Surface, 2),
        (SyntaxRole::Scope, 1),
    ] {
        assert_eq!(
            payloads
                .iter()
                .filter(|payload| payload.role == role)
                .count(),
            expected_count,
            "the literal retains every {role:?} payload"
        );
    }

    let binding = payloads
        .iter()
        .find(|payload| payload.role == SyntaxRole::Binding)
        .expect("the named static import produces one Binding payload");
    let local_start = source
        .find("import { ")
        .expect("literal has the named-import prefix")
        + "import { ".len();
    let expected_local_range = range(
        local_start as u64,
        (local_start + "dependency".len()) as u64,
    );
    let TypeScriptPayloadData::Binding(binding_data) = &binding.data else {
        panic!("binding payload has BindingDataV1")
    };
    let expected_evaluated_stages = [
        "b.form",
        "b.local_uniqueness",
        "b.specifier",
        "b.candidates",
        "b.export_binding",
        "b.writes",
        "b.result",
    ]
    .into_iter()
    .map(|wire| {
        BindingStageV1::parse_wire(&typescript_registry_binding(), wire)
            .expect("the fixed Binding stage literal is registered")
    })
    .collect::<Vec<_>>();
    assert_eq!(binding.range, expected_local_range);
    assert_eq!(binding_data.import_kind, PayloadImportKindV1::Named);
    assert_eq!(binding_data.local_name, "dependency");
    assert_eq!(binding_data.specifier, "./dependency");
    assert_eq!(
        binding_data.specifier_range,
        source_span(source, "\"./dependency\"")
    );
    assert_eq!(
        binding.outcome,
        TypeScriptOutcomeV1::Binding(ResolutionOutcomeV1::Resolved)
    );
    assert_eq!(
        binding_data.evaluated_stages, expected_evaluated_stages,
        "the named static import completes every Binding stage in fixed order"
    );
    assert!(matches!(
        &binding.reasons,
        TypeScriptReasonsV1::Binding(reasons) if reasons.is_empty()
    ));
    assert_eq!(binding.primary_reason, None);
    // The design requires the complete, byte-sorted extensionless path
    // set. `p.js` and `p/index.js` are rejection witnesses only when present
    // in the Git tree, so this fixture has neither of those nonexistent rows.
    let expected_candidate_paths = vec![
        ("src/dependency".to_owned(), None),
        (
            "src/dependency.ts".to_owned(),
            Some(file_id("src/dependency.ts")),
        ),
        ("src/dependency.tsx".to_owned(), None),
        ("src/dependency/index.ts".to_owned(), None),
        ("src/dependency/index.tsx".to_owned(), None),
    ];
    assert_eq!(
        binding_data
            .candidate_paths
            .iter()
            .map(|candidate| (candidate.path.clone(), candidate.file_key.clone()))
            .collect::<Vec<_>>(),
        expected_candidate_paths,
        "candidate paths retain every normalized extensionless path with only the included file keyed"
    );
    assert!(binding_data.candidate_paths.iter().all(|candidate| {
        !candidate.rejection_witness && candidate.unexpanded_ancestor_key.is_none()
    }));
    let expected_dependency_id = DeclarationId::from_source(
        file_id("src/dependency.ts"),
        source_span(dependency_source, "function dependency(){return 1;}"),
    );
    assert_eq!(
        binding_data.resolved_function_id.as_ref(),
        Some(&expected_dependency_id),
        "the named export resolves to the independently ranged dependency callable"
    );
}

#[cfg(reviewgraphen_unimplemented_contracts)]
#[ignore = "slice V3: A2"]
#[test]
fn ac17_a2_accepts_the_source_rebuilt_catalogue_and_rejects_one_field_tamper() {
    let context = admitted_context_from_source_fixture();
    let (submitted, expected) = source_rebuilt_catalog_case();
    let accepted = validate_catalog_from_basis(&context, submitted)
        .expect("A2 accepts the complete source-rebuilt syntax catalogue");
    assert_eq!(accepted.as_ref(), &expected);
    let _mismatch: AccountingMismatch =
        validate_catalog_from_basis(&context, source_tampered_catalog_submission())
            .expect_err("A2 reports a typed source accounting mismatch");
}

#[cfg(reviewgraphen_unimplemented_contracts)]
#[ignore = "slice V4: A3"]
#[test]
fn ac18_a3_accepts_the_source_rebuilt_extraction_and_rejects_one_field_tamper() {
    let context = admitted_context_from_source_fixture();
    let (submitted, expected) = source_rebuilt_extraction_case();
    let accepted = validate_extraction_from_basis(&context, submitted)
        .expect("A3 accepts the complete source-rebuilt extraction report");
    assert_eq!(accepted.as_ref(), &expected);
    let _mismatch: AccountingMismatch =
        validate_extraction_from_basis(&context, source_tampered_extraction_submission())
            .expect_err("A3 reports a typed source accounting mismatch");
}

#[cfg(reviewgraphen_unimplemented_contracts)]
#[ignore = "slice V5: A4"]
#[test]
fn ac19_a4_accepts_the_source_rebuilt_ingestion_and_rejects_one_field_tamper() {
    let context = admitted_context_from_source_fixture();
    let extraction = validate_extraction_from_basis(&context, source_rebuilt_extraction_case().0)
        .expect("A3 supplies the source-validated extraction required by A4");
    let (submitted, expected) = source_rebuilt_ingestion_case();
    let accepted = validate_ingestion_from_sources(&context, &extraction, submitted)
        .expect("A4 accepts a source-rebuilt ingestion submission");
    assert_eq!(accepted.as_ref(), &expected);
    assert!(matches!(
        validate_ingestion_from_sources(
            &context,
            &extraction,
            source_tampered_ingestion_submission(),
        ),
        Err(IngestionError::AccountingMismatch(_))
    ));
}

#[cfg(reviewgraphen_unimplemented_contracts)]
#[ignore = "slice V6: A5"]
#[test]
fn ac20_a5_accepts_the_source_rebuilt_input_and_rejects_one_field_tamper() {
    let context = admitted_context_from_source_fixture();
    let extraction = validate_extraction_from_basis(&context, source_rebuilt_extraction_case().0)
        .expect("A3 supplies the source-validated extraction required by A5");
    let ingestion =
        validate_ingestion_from_sources(&context, &extraction, source_rebuilt_ingestion_case().0)
            .expect("A4 supplies the source-validated ingestion required by A5");
    let (submitted, expected) = source_rebuilt_synthesis_case();
    let accepted = validate_synthesis_input(&extraction, &ingestion, submitted)
        .expect("A5 accepts a source-rebuilt synthesis input");
    assert_eq!(accepted.as_ref(), &expected);
    let _mismatch: AccountingMismatch = validate_synthesis_input(
        &extraction,
        &ingestion,
        source_tampered_synthesis_submission(),
    )
    .expect_err("A5 reports a typed source accounting mismatch");
}

#[cfg(reviewgraphen_unimplemented_contracts)]
#[ignore = "slice V8: A6"]
#[test]
fn ac21_a6_and_d1_accept_source_rebuilt_closure_and_reject_one_field_tamper() {
    let expected = synthesize(source_validated_synthesis_input_for_d1())
        .expect("D1 derives the expected source-validated obligation set");
    let expected_value = expected.as_ref().clone();
    let accepted = validate_obligation_closure(expected, source_rebuilt_closure_submission())
        .expect("A6 accepts the complete source-rebuilt closure");
    assert_eq!(accepted.as_ref(), &expected_value);

    let expected_for_tamper = synthesize(source_validated_synthesis_input_for_d1())
        .expect("D1 derives an independent expected closure for the tamper case");
    let _mismatch: AccountingMismatch =
        validate_obligation_closure(expected_for_tamper, source_tampered_closure_submission())
            .expect_err("A6 reports a typed source accounting mismatch");
}
