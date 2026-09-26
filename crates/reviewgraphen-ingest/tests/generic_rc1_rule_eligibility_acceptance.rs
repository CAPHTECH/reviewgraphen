//! RC1-G5 acceptance oracle for generic rule eligibility.
//!
//! The source and rule literals below are frozen contract expectations. This
//! test deliberately does not derive an oracle from `MvpRulePack::rules()` or
//! from the synthesized output.

use reviewgraphen_core::MvpRulePack;
use reviewgraphen_ingest::{CapabilityState, IngestRequest, IngestionObstructionKind, ingest};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const IDENTITY: &str = "reviewgraphen.test/generic-rc1-rule-eligibility";
const CAPABILITY_GAP_RULE: &str = "capability_gap.origin_rule@1";
const CAPABILITY_GAP_PROPERTY: &str = "reviewgraphen.capability_gap";
const NODE_RULE: &str = "node.changed_public_symbol@2";
const CALL_CONTRACT_RULE: &str = "relation.changed_call_contract@1";
const EXTERNAL_SIDE_EFFECT_RULE: &str = "path.external_side_effect@1";
const PAYMENT_INVARIANT_RULE: &str = "invariant.payment_at_most_once@1";

struct ImmutableGitFixture {
    workspace: TempDir,
    repository: PathBuf,
    base: String,
    target: String,
}

impl ImmutableGitFixture {
    fn request(&self) -> IngestRequest {
        IngestRequest::new(
            self.workspace.path(),
            &self.repository,
            IDENTITY,
            &self.base,
            &self.target,
        )
    }
}

fn run_git<const N: usize>(repository: &Path, arguments: [&str; N]) {
    let status = Command::new("git")
        .current_dir(repository)
        .args(arguments)
        .status()
        .expect("fixture Git command starts");
    assert!(status.success(), "fixture Git command succeeds");
}

fn git_stdout<const N: usize>(repository: &Path, arguments: [&str; N]) -> String {
    let output = Command::new("git")
        .current_dir(repository)
        .args(arguments)
        .output()
        .expect("fixture Git output starts");
    assert!(output.status.success(), "fixture Git output succeeds");
    String::from_utf8(output.stdout)
        .expect("fixture Git output is UTF-8")
        .trim()
        .to_owned()
}

fn write(repository: &Path, path: &str, source: &str) {
    let path = repository.join(path);
    fs::create_dir_all(path.parent().expect("fixture file has a parent"))
        .expect("fixture parent exists");
    fs::write(path, source).expect("fixture source writes");
}

fn initialize_fixture() -> (TempDir, PathBuf) {
    let workspace = tempfile::tempdir().expect("temporary fixture workspace");
    let repository = workspace.path().join("fixture");
    fs::create_dir(&repository).expect("fixture repository directory");
    run_git(&repository, ["init", "--quiet"]);
    run_git(
        &repository,
        ["config", "user.email", "reviewgraphen@example.test"],
    );
    run_git(&repository, ["config", "user.name", "ReviewGraphen test"]);
    (workspace, repository)
}

fn commit(repository: &Path, message: &str) -> String {
    run_git(repository, ["add", "."]);
    run_git(repository, ["commit", "--quiet", "-m", message]);
    git_stdout(repository, ["rev-parse", "HEAD"])
}

fn immutable_async_change_fixture() -> ImmutableGitFixture {
    let (workspace, repository) = initialize_fixture();
    write(
        &repository,
        "src/lib.rs",
        "use std::sync::{Arc, Mutex};\n\
         pub async fn handler(shared: Arc<Mutex<u64>>) -> u64 {\n    \
         let value = load().await;\n    \
         drop(shared);\n    \
         value\n}\n\
         async fn load() -> u64 {\n    1\n}\n",
    );
    let base = commit(&repository, "base");
    write(
        &repository,
        "src/lib.rs",
        "use std::sync::{Arc, Mutex};\n\
         pub async fn handler(shared: Arc<Mutex<u64>>) -> u64 {\n    \
         let value = load().await;\n    \
         let next = value + 1;\n    \
         drop(shared);\n    \
         next\n}\n\
         async fn load() -> u64 {\n    1\n}\n",
    );
    let target = commit(&repository, "target");

    ImmutableGitFixture {
        workspace,
        repository,
        base,
        target,
    }
}

fn immutable_mixed_file_fixture(async_handler: bool) -> ImmutableGitFixture {
    let (workspace, repository) = initialize_fixture();
    let base_handler = if async_handler {
        "pub async fn handler() -> u64 {\n    1\n}\n"
    } else {
        "pub fn handler() -> u64 {\n    1\n}\n"
    };
    let target_handler = if async_handler {
        "pub async fn handler() -> u64 {\n    2\n}\n"
    } else {
        "pub fn handler() -> u64 {\n    2\n}\n"
    };
    write(&repository, "src/lib.rs", base_handler);
    write(&repository, "src/broken.rs", "pub fn broken( {\n");
    let base = commit(&repository, "base");
    write(&repository, "src/lib.rs", target_handler);
    let target = commit(&repository, "target");

    ImmutableGitFixture {
        workspace,
        repository,
        base,
        target,
    }
}

fn handler(program: &reviewgraphen_core::ProgramSpace) -> &reviewgraphen_core::Artifact {
    program
        .artifacts()
        .iter()
        .find(|artifact| artifact.kind == "function" && artifact.label == "crate::handler")
        .expect("literal handler function exists")
}

fn assert_changed_handler(
    program: &reviewgraphen_core::ProgramSpace,
    handler: &reviewgraphen_core::Artifact,
) {
    assert!(
        program.relations().iter().any(|relation| {
            relation.kind == "contains"
                && relation.target_ids.contains(&handler.id)
                && program.artifact(&relation.source_id).is_some_and(|source| {
                    source.attributes.get("changed") == Some(&serde_json::json!(true))
                })
        }),
        "a changed change-family artifact contains the literal handler",
    );
}

fn assert_parse_failure_and_partial_capabilities(result: &reviewgraphen_ingest::IngestResult) {
    let broken_file = result
        .program_space
        .artifacts()
        .iter()
        .find(|artifact| artifact.kind == "file" && artifact.label == "src/broken.rs")
        .expect("literal broken file source exists");
    assert!(
        result
            .extraction_report
            .obstructions
            .iter()
            .any(|obstruction| {
                obstruction.kind == IngestionObstructionKind::ParseFailure
                    && obstruction.paths == BTreeSet::from(["src/broken.rs".to_owned()])
                    && obstruction.source_ids == BTreeSet::from([broken_file.id.clone()])
            }),
        "the unchanged broken Rust file remains a retained parse-failure obstruction bound to its exact path and source identity",
    );
    assert_eq!(
        result.extraction_report.capabilities["ast"],
        CapabilityState::Partial,
        "the parse failure makes ast partial",
    );
    assert_eq!(
        result.extraction_report.capabilities["concurrency_model"],
        CapabilityState::Partial,
        "the parse failure makes concurrency_model partial",
    );
}

fn has_origin_gap(bundle: &reviewgraphen_core::ObligationBundle, origin_rule: &str) -> bool {
    bundle.obligations().iter().any(|obligation| {
        obligation.version().rule() == CAPABILITY_GAP_RULE
            && obligation.property_id() == CAPABILITY_GAP_PROPERTY
            && obligation
                .applicability_reasons()
                .contains(&format!("origin_rule:{origin_rule}"))
    })
}

#[test]
#[allow(clippy::nonminimal_bool)]
fn changed_public_async_symbol_does_not_activate_payment_rule_gaps() {
    let fixture = immutable_async_change_fixture();
    assert_ne!(
        fixture.base, fixture.target,
        "fixture commits are immutable and distinct",
    );
    assert_eq!(
        git_stdout(
            &fixture.repository,
            ["diff", "--name-only", &fixture.base, &fixture.target],
        ),
        "src/lib.rs",
        "the target changes only the public async fixture source",
    );

    let result = ingest(&fixture.request()).expect("normal Rust ingestion succeeds");
    let handler = handler(&result.program_space);
    assert_eq!(
        handler.attributes.get("public"),
        Some(&serde_json::json!(true)),
        "fixture declaration is public",
    );
    assert_eq!(
        handler.attributes.get("async"),
        Some(&serde_json::json!(true)),
        "fixture declaration is async",
    );
    assert_changed_handler(&result.program_space, handler);

    let bundle = MvpRulePack::synthesize(&result.program_space).expect("rule synthesis succeeds");
    let node_obligations = bundle
        .obligations()
        .iter()
        .filter(|obligation| {
            obligation.version().rule() == NODE_RULE
                && obligation.property_id() == "async.concurrent_reentry"
        })
        .collect::<Vec<_>>();
    assert_eq!(
        node_obligations.len(),
        1,
        "the changed public async symbol retains one substantive literal node obligation",
    );
    assert_eq!(
        node_obligations[0].target_refs(),
        std::slice::from_ref(&handler.id),
    );
    assert_eq!(node_obligations[0].applicability_status(), "applicable");

    let forbidden_rule_origins = [
        CALL_CONTRACT_RULE,
        EXTERNAL_SIDE_EFFECT_RULE,
        PAYMENT_INVARIANT_RULE,
    ];
    assert!(
        bundle.obligations().iter().all(|obligation| {
            !forbidden_rule_origins.contains(&obligation.version().rule())
                && !(obligation.version().rule() == CAPABILITY_GAP_RULE
                    && obligation.property_id() == CAPABILITY_GAP_PROPERTY
                    && obligation.applicability_reasons().iter().any(|reason| {
                        forbidden_rule_origins
                            .iter()
                            .any(|origin| *reason == format!("origin_rule:{origin}"))
                    }))
        }),
        "forbidden-rule-origin assertion: no double-submit source facts may activate a \
         payment rule or its capability gap",
    );
}

#[test]
fn only_a_concrete_mixed_file_node_trigger_may_emit_its_origin_gap() {
    let positive = immutable_mixed_file_fixture(true);
    let positive_result =
        ingest(&positive.request()).expect("positive normal Rust ingestion succeeds");
    let positive_handler = handler(&positive_result.program_space);
    assert_eq!(
        positive_handler.attributes.get("public"),
        Some(&serde_json::json!(true)),
        "positive handler is public",
    );
    assert_eq!(
        positive_handler.attributes.get("async"),
        Some(&serde_json::json!(true)),
        "positive handler independently records local concurrency",
    );
    assert_changed_handler(&positive_result.program_space, positive_handler);
    assert_parse_failure_and_partial_capabilities(&positive_result);

    let positive_bundle = MvpRulePack::synthesize(&positive_result.program_space)
        .expect("positive synthesis succeeds");
    let positive_nodes = positive_bundle
        .obligations()
        .iter()
        .filter(|obligation| {
            obligation.version().rule() == NODE_RULE
                && obligation.property_id() == "async.concurrent_reentry"
                && obligation.target_refs() == std::slice::from_ref(&positive_handler.id)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        positive_nodes.len(),
        1,
        "the independently observed positive trigger materializes one node obligation",
    );
    assert_eq!(positive_nodes[0].applicability_status(), "unknown");
    assert_eq!(
        positive_nodes[0].applicability_reasons(),
        &BTreeSet::from([
            "capability_partial:ast".to_owned(),
            "capability_partial:concurrency_model".to_owned(),
        ]),
        "the positive node obligation has exactly the two parse-failure capability-gap reasons",
    );
    assert!(
        has_origin_gap(&positive_bundle, NODE_RULE),
        "the concrete positive trigger retains its explicit node-origin capability gap",
    );

    let negative = immutable_mixed_file_fixture(false);
    let negative_result =
        ingest(&negative.request()).expect("negative normal Rust ingestion succeeds");
    let negative_handler = handler(&negative_result.program_space);
    assert_eq!(
        negative_handler.attributes.get("public"),
        Some(&serde_json::json!(true)),
        "negative handler remains public",
    );
    assert_eq!(
        negative_handler.attributes.get("async"),
        Some(&serde_json::json!(false)),
        "negative handler lacks the positive local-concurrency trigger fact",
    );
    assert_changed_handler(&negative_result.program_space, negative_handler);
    assert_parse_failure_and_partial_capabilities(&negative_result);

    let negative_bundle = MvpRulePack::synthesize(&negative_result.program_space)
        .expect("negative synthesis succeeds");
    assert!(
        negative_bundle.obligations().iter().all(|obligation| {
            obligation.version().rule() != NODE_RULE
                || obligation.property_id() != "async.concurrent_reentry"
        }),
        "the negative fixture has no concrete node obligation without local concurrency",
    );
    assert!(
        !has_origin_gap(&negative_bundle, NODE_RULE),
        "negative-origin-gap assertion: without the concrete node trigger, no node-origin \
         capability gap may be emitted",
    );
}
