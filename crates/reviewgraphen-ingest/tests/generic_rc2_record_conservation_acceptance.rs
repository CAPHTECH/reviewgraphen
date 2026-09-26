//! Cross-language conservation acceptance for source-observed direct calls.

use reviewgraphen_core::ContentHash;
use reviewgraphen_core::source_review::admitted_source::{
    AdmittedSourceBundleV1, AdmittedSourceFileV1,
};
use reviewgraphen_core::source_review::basis::{
    SourceFileOutcome, SourceReviewBasisV1, SourceReviewFileV1, SourceReviewSyntaxV1,
    SourceSyntaxRole,
};
use reviewgraphen_core::source_review::ids::{
    CanonicalFileKey, SnapshotBinding, SourceFileId, SourceHash,
};
use reviewgraphen_core::source_review::reasons::{
    CallReason, ReasonSet, ResolutionKind, ResolutionOutcomeV1,
};
use reviewgraphen_core::source_review::registry::typescript_registry_binding;
use reviewgraphen_ingest::typescript::native_syntax::{NativeSyntaxRecord, collect_native_syntax};
use reviewgraphen_ingest::typescript::payload::{
    SyntaxRole, TypeScriptOutcomeV1, TypeScriptPayloadData, TypeScriptPayloadSourceViewV1,
    TypeScriptReasonsV1, rebuild_payload_from_source,
};
use reviewgraphen_ingest::{
    CallKind, CallObstructionReason, IngestRequest, ingest_with_sources_v2,
};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const IDENTITY: &str = "reviewgraphen.test/generic-rc2-record-conservation";
const FIXTURE_GIT_DATE: &str = "2000-01-01T00:00:00Z";
const RUST_PATH: &str = "src/direct.rs";
const RUST_SOURCE: &str =
    "fn resolved() {}\npub fn caller() {\n    resolved();\n    missing();\n}\n";
const TYPESCRIPT_PATH: &str = "src/direct.ts";
const TYPESCRIPT_SOURCE: &str =
    "function resolved() {}\nexport function caller() {\n    resolved();\n    missing();\n}\n";

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RustKey {
    path: String,
    start_line: u64,
    end_line: u64,
    start_column: u64,
    end_column: u64,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct TypeScriptKey {
    file_id: SourceFileId,
    start: u64,
    end: u64,
}

struct RustRepository {
    _workspace: TempDir,
    workspace_root: PathBuf,
    root: PathBuf,
    base: String,
    target: String,
}

struct CompleteTargetView {
    sources: AdmittedSourceBundleV1,
}

impl TypeScriptPayloadSourceViewV1 for CompleteTargetView {
    fn source_bundle(&self) -> &AdmittedSourceBundleV1 {
        &self.sources
    }

    fn target_outcome_for_path(&self, canonical_path: &str) -> Option<SourceFileOutcome> {
        self.sources.target_outcome_for_path(canonical_path)
    }

    fn target_inventory_complete(&self) -> bool {
        true
    }
}

impl RustRepository {
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

fn rust_key(
    path: &str,
    start_line: u64,
    end_line: u64,
    start_column: u64,
    end_column: u64,
) -> RustKey {
    RustKey {
        path: path.to_owned(),
        start_line,
        end_line,
        start_column,
        end_column,
    }
}

fn typescript_file_id() -> SourceFileId {
    SourceFileId::from_basis_file_key(
        CanonicalFileKey::from_basis_path(TYPESCRIPT_PATH).expect("literal TypeScript path"),
    )
}

fn typescript_key(file_id: SourceFileId, start: u64, end: u64) -> TypeScriptKey {
    TypeScriptKey {
        file_id,
        start,
        end,
    }
}

fn rust_denominator() -> (RustKey, RustKey, BTreeSet<RustKey>) {
    let resolved = rust_key(RUST_PATH, 3, 3, 5, 14);
    let unresolved = rust_key(RUST_PATH, 4, 4, 5, 13);
    let denominator = BTreeSet::from([resolved.clone(), unresolved.clone()]);
    (resolved, unresolved, denominator)
}

fn typescript_denominator() -> (TypeScriptKey, TypeScriptKey, BTreeSet<TypeScriptKey>) {
    let file_id = typescript_file_id();
    let resolved = typescript_key(file_id.clone(), 54, 64);
    let unresolved = typescript_key(file_id, 70, 79);
    let denominator = BTreeSet::from([resolved.clone(), unresolved.clone()]);
    (resolved, unresolved, denominator)
}

fn exact_set<K: Clone + Ord>(values: &[K]) -> Result<BTreeSet<K>, &'static str> {
    let keys = values.iter().cloned().collect::<BTreeSet<_>>();
    if keys.len() == values.len() {
        Ok(keys)
    } else {
        Err("duplicate outcome key")
    }
}

fn check_partition<K: Clone + Ord>(
    denominator: &BTreeSet<K>,
    resolved: &[K],
    obstruction: &[K],
    expected_resolved: &BTreeSet<K>,
    expected_obstruction: &BTreeSet<K>,
) -> Result<(), &'static str> {
    let resolved = exact_set(resolved)?;
    let obstruction = exact_set(obstruction)?;
    if !resolved.is_subset(denominator) || !obstruction.is_subset(denominator) {
        return Err("outcome key is absent from the literal denominator");
    }
    if !resolved.is_disjoint(&obstruction) {
        return Err("resolved and obstruction outcomes overlap");
    }
    let union = resolved
        .union(&obstruction)
        .cloned()
        .collect::<BTreeSet<_>>();
    if &union != denominator {
        return Err("resolved and obstruction outcomes do not cover the denominator");
    }
    if &resolved != expected_resolved || &obstruction != expected_obstruction {
        return Err("outcome partition differs from the literal expectation");
    }
    Ok(())
}

fn calibrate_local_set_checker() {
    let (rust_resolved, rust_unresolved, rust_denominator) = rust_denominator();
    let expected_rust_resolved = BTreeSet::from([rust_resolved.clone()]);
    let expected_rust_obstruction = BTreeSet::from([rust_unresolved.clone()]);

    assert!(
        check_partition(
            &rust_denominator,
            &[rust_resolved.clone(), rust_resolved.clone()],
            std::slice::from_ref(&rust_unresolved),
            &expected_rust_resolved,
            &expected_rust_obstruction,
        )
        .is_err(),
        "the local checker rejects a duplicate key"
    );
    assert!(
        check_partition(
            &rust_denominator,
            std::slice::from_ref(&rust_resolved),
            &[rust_resolved.clone(), rust_unresolved.clone()],
            &expected_rust_resolved,
            &expected_rust_obstruction,
        )
        .is_err(),
        "the local checker rejects resolved/obstruction overlap"
    );
    assert!(
        check_partition(
            &rust_denominator,
            std::slice::from_ref(&rust_resolved),
            &[],
            &expected_rust_resolved,
            &expected_rust_obstruction,
        )
        .is_err(),
        "the local checker rejects disappearance"
    );
    let shifted_rust = rust_key(RUST_PATH, 3, 3, 6, 14);
    assert!(
        check_partition(
            &rust_denominator,
            &[shifted_rust],
            std::slice::from_ref(&rust_unresolved),
            &expected_rust_resolved,
            &expected_rust_obstruction,
        )
        .is_err(),
        "the local checker rejects a shifted Rust source position"
    );

    let (typescript_resolved, typescript_unresolved, typescript_denominator) =
        typescript_denominator();
    let expected_typescript_resolved = BTreeSet::from([typescript_resolved.clone()]);
    let expected_typescript_obstruction = BTreeSet::from([typescript_unresolved.clone()]);
    let shifted_typescript = typescript_key(typescript_file_id(), 70, 80);
    assert!(
        check_partition(
            &typescript_denominator,
            &[typescript_resolved],
            &[shifted_typescript],
            &expected_typescript_resolved,
            &expected_typescript_obstruction,
        )
        .is_err(),
        "the local checker rejects a shifted TypeScript half-open range"
    );
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
    fs::create_dir_all(path.parent().expect("fixture source has a parent"))
        .expect("fixture source parent exists");
    fs::write(path, bytes).expect("fixture source writes");
}

fn rust_repository() -> RustRepository {
    let workspace = tempfile::tempdir().expect("temporary workspace");
    let workspace_root = workspace.path().to_owned();
    let root = workspace_root.join("fixture");
    fs::create_dir(&root).expect("fixture repository directory");
    command(&root, ["init", "--quiet", "--object-format=sha1"]);
    command(
        &root,
        ["config", "user.email", "reviewgraphen@example.test"],
    );
    command(&root, ["config", "user.name", "ReviewGraphen test"]);
    write(&root, RUST_PATH, "fn baseline() {}\n");
    command(&root, ["add", "."]);
    command(&root, ["commit", "--quiet", "-m", "base"]);
    let base = output(&root, ["rev-parse", "HEAD"]);
    write(&root, RUST_PATH, RUST_SOURCE);
    command(&root, ["add", "."]);
    command(&root, ["commit", "--quiet", "-m", "direct call partition"]);
    let target = output(&root, ["rev-parse", "HEAD"]);
    RustRepository {
        _workspace: workspace,
        workspace_root,
        root,
        base,
        target,
    }
}

fn typescript_view() -> CompleteTargetView {
    let basis = SourceReviewBasisV1::new(
        typescript_registry_binding(),
        vec![SourceReviewFileV1 {
            path: TYPESCRIPT_PATH.to_owned(),
            language: "typescript".to_owned(),
            outcome: SourceFileOutcome::Parsed,
        }],
        vec![
            SourceReviewSyntaxV1 {
                file_path: TYPESCRIPT_PATH.to_owned(),
                role: SourceSyntaxRole::Callable,
            },
            SourceReviewSyntaxV1 {
                file_path: TYPESCRIPT_PATH.to_owned(),
                role: SourceSyntaxRole::Call,
            },
        ],
    )
    .expect("one parsed literal TypeScript source file");
    let bytes = TYPESCRIPT_SOURCE.as_bytes().to_vec();
    let sources = AdmittedSourceBundleV1::new(
        &basis,
        vec![AdmittedSourceFileV1 {
            file_id: typescript_file_id(),
            source_hash: SourceHash::from_source_bytes(&bytes),
            bytes,
        }],
    )
    .expect("one hash-matching admitted TypeScript source file");
    CompleteTargetView { sources }
}

fn assert_rust_partition() {
    let repository = rust_repository();
    let result = ingest_with_sources_v2(&repository.request(), u64::MAX)
        .expect("Rust source-retaining v2 ingest succeeds");
    let source_entries = result.legacy.source_bundle.entries();
    assert_eq!(source_entries.len(), 1, "one immutable admitted Rust file");
    let source = &source_entries[0];
    assert_eq!(source.path(), RUST_PATH);
    assert_eq!(source.bytes(), RUST_SOURCE.as_bytes());
    assert_eq!(
        source.content_hash(),
        &ContentHash::sha256(RUST_SOURCE.as_bytes())
    );
    assert_eq!(
        source.cas_hash(),
        &ContentHash::sha256(RUST_SOURCE.as_bytes())
    );

    let (resolved_key, obstruction_key, denominator) = rust_denominator();
    let expected_resolved = BTreeSet::from([resolved_key.clone()]);
    let expected_obstruction = BTreeSet::from([obstruction_key.clone()]);
    let resolved_relations = result
        .legacy
        .program_space
        .relations()
        .iter()
        .filter(|relation| {
            relation.kind == "calls"
                && matches!(
                    relation.attributes.get("resolution"),
                    Some(Value::String(value)) if value == "syntactic_unique"
                )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        resolved_relations.len(),
        1,
        "the literal fixture has one syntactically unique direct-call fact"
    );
    let relation = resolved_relations[0];
    let resolved = result
        .legacy
        .resolved_direct_call_occurrences()
        .iter()
        .map(|occurrence| {
            assert_eq!(
                occurrence.relation_id(),
                &relation.id,
                "a resolved source occurrence joins its accepted calls relation by final ID"
            );
            assert_eq!(occurrence.path(), source.path());
            rust_key(
                occurrence.path(),
                occurrence.start_line(),
                occurrence.end_line(),
                occurrence.start_column(),
                occurrence.end_column(),
            )
        })
        .collect::<Vec<_>>();
    let obstruction = result
        .ingestion_report_v2
        .located_call_occurrences()
        .iter()
        .filter(|occurrence| occurrence.call_kind() == CallKind::Direct)
        .map(|occurrence| {
            assert_eq!(
                occurrence.reason(),
                CallObstructionReason::DirectTargetCountZero,
                "the literal missing direct call retains its closed typed reason"
            );
            let span = occurrence.span();
            assert_eq!(span.path, source.path());
            rust_key(
                &span.path,
                span.start_line,
                span.end_line,
                span.start_column,
                span.end_column,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        resolved.len(),
        1,
        "one resolved Rust carrier before set checking"
    );
    assert_eq!(
        obstruction.len(),
        1,
        "one direct Rust obstruction carrier before set checking"
    );
    check_partition(
        &denominator,
        &resolved,
        &obstruction,
        &expected_resolved,
        &expected_obstruction,
    )
    .expect("Rust resolved XOR typed-obstruction partition is exact and exhaustive");
}

fn assert_typescript_partition() {
    let view = typescript_view();
    let file_id = typescript_file_id();
    let source = view
        .source_bundle()
        .file(&file_id)
        .expect("the exact TypeScript file is admitted");
    assert_eq!(source.bytes, TYPESCRIPT_SOURCE.as_bytes());
    assert_eq!(
        source.source_hash,
        SourceHash::from_source_bytes(TYPESCRIPT_SOURCE.as_bytes())
    );
    assert_eq!(
        view.source_bundle().canonical_basis_path(&file_id),
        Some(TYPESCRIPT_PATH)
    );

    let (resolved_key, obstruction_key, denominator) = typescript_denominator();
    let expected_resolved = BTreeSet::from([resolved_key.clone()]);
    let expected_obstruction = BTreeSet::from([obstruction_key.clone()]);
    let observed = collect_native_syntax(file_id.clone(), &source.bytes)
        .into_iter()
        .filter_map(|record| match record {
            NativeSyntaxRecord::Call {
                file_id: observed_file_id,
                range,
                ..
            } => {
                assert_eq!(observed_file_id, file_id);
                Some(typescript_key(observed_file_id, range.start(), range.end()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let observed_set = exact_set(&observed).expect("source observation does not duplicate a call");
    assert_eq!(
        observed_set, denominator,
        "literal TypeScript source denominator"
    );

    let payloads = rebuild_payload_from_source(
        &view,
        &typescript_registry_binding(),
        &SnapshotBinding::from_admitted_binding("generic-rc2-record-conservation@1"),
        file_id.clone(),
    )
    .expect("TypeScript payloads rebuild from admitted bytes");
    let calls = payloads
        .into_iter()
        .filter(|payload| payload.role == SyntaxRole::Call)
        .collect::<Vec<_>>();
    assert_eq!(
        calls.len(),
        2,
        "one payload for each literal TypeScript call"
    );

    let mut resolved = Vec::new();
    let mut obstruction = Vec::new();
    for payload in calls {
        let key = typescript_key(file_id.clone(), payload.range.start(), payload.range.end());
        if key == resolved_key {
            assert_eq!(
                payload.outcome,
                TypeScriptOutcomeV1::Call(ResolutionOutcomeV1::Resolved)
            );
            assert_eq!(
                payload.reasons,
                TypeScriptReasonsV1::Call(ReasonSet::new([]))
            );
            let TypeScriptPayloadData::Call(data) = payload.data else {
                panic!("resolved source call retains CallDataV1");
            };
            assert!(data.callee_id.is_some(), "resolved call has a typed callee");
            assert_eq!(
                data.resolution_kind,
                Some(ResolutionKind::SyntacticUnique),
                "resolved local call retains its closed resolution kind"
            );
            resolved.push(key);
        } else if key == obstruction_key {
            assert_eq!(
                payload.outcome,
                TypeScriptOutcomeV1::Call(ResolutionOutcomeV1::Unresolved)
            );
            assert_eq!(
                payload.reasons,
                TypeScriptReasonsV1::Call(ReasonSet::new([CallReason::UnresolvedName]))
            );
            let TypeScriptPayloadData::Call(data) = payload.data else {
                panic!("unresolved source call retains CallDataV1");
            };
            assert!(data.callee_id.is_none(), "unresolved call has no callee");
            assert!(
                data.resolution_kind.is_none(),
                "unresolved call has no resolution kind"
            );
            obstruction.push(key);
        } else {
            match payload.outcome {
                TypeScriptOutcomeV1::Call(ResolutionOutcomeV1::Resolved) => resolved.push(key),
                TypeScriptOutcomeV1::Call(ResolutionOutcomeV1::Unresolved) => obstruction.push(key),
                _ => panic!("Call payload has a call outcome"),
            }
        }
    }
    check_partition(
        &denominator,
        &resolved,
        &obstruction,
        &expected_resolved,
        &expected_obstruction,
    )
    .expect("TypeScript resolved XOR typed-obstruction partition is exact and exhaustive");
}

#[test]
fn supported_direct_calls_are_conserved_across_current_rust_and_typescript_carriers() {
    calibrate_local_set_checker();
    assert_rust_partition();
    assert_typescript_partition();
}
