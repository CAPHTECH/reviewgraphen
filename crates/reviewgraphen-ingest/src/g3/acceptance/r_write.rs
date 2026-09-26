//! R-write@2: admission-owned Git bytes, accepted write origin, full F4 path identity.

use crate::g3::rust::{RustFactRefV1, RustG3OutcomeV1, RustInclusiveLineColumnRange};
use crate::{IngestRequest, IngestWithSourcesResult, ingest_with_sources};
use reviewgraphen_core::{ContentHash, ProgramSpace, SnapshotSourceEntry, StableId};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use syn::spanned::Spanned;

const PATH: &str = "src/structural.rs";
const SOURCE: &str = "#[cfg(test)]\nmod structural {\n    fn callee() {}\n    #[test]\n    fn caller() {\n        let mut value = 0;\n        value = 1;\n        callee();\n    }\n}\n";
const SOURCE_SHA256: &str =
    "sha256:e22111d84a1611a511f1c7bccaeca7abb03c058ab51fc753511c75e3f087eadd";

fn range(span: proc_macro2::Span) -> RustInclusiveLineColumnRange {
    range_from_start_and_end(span, span)
}

fn range_from_start_and_end(
    start_span: proc_macro2::Span,
    end_span: proc_macro2::Span,
) -> RustInclusiveLineColumnRange {
    let start = start_span.start();
    let end = end_span.end();
    RustInclusiveLineColumnRange::new(
        start.line as u64,
        start.column as u64 + 1,
        end.line as u64,
        end.column as u64,
    )
    .expect("inclusive Rust span")
}

fn accepted_identity(
    artifact: &reviewgraphen_core::Artifact,
    entry: &SnapshotSourceEntry,
    kind: &str,
    label: &str,
) -> bool {
    artifact.kind == kind
        && artifact.label == label
        && artifact.language.as_deref() == Some("rust")
        && artifact.content_hash.as_ref() == Some(entry.content_hash())
        && artifact
            .location
            .as_ref()
            .is_some_and(|location| location.path == entry.path())
}

fn assert_accepted_location(
    artifact: &reviewgraphen_core::Artifact,
    entry: &SnapshotSourceEntry,
    accepted_full: RustInclusiveLineColumnRange,
) {
    assert!(
        artifact.location.as_ref().is_some_and(|location| {
            location.path == entry.path()
                && location.start_line == Some(accepted_full.start_line())
                && location.start_column == Some(accepted_full.start_column())
                && location.end_line == Some(accepted_full.end_line())
                && location.end_column == Some(accepted_full.end_column())
        }),
        "existing accepted Artifact location, not syntax-start join key"
    );
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join(".xdg"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
        .args(args)
        .output()
        .expect("git executes");
    assert!(
        output.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

struct Fixture {
    _workspace: tempfile::TempDir,
    workspace: PathBuf,
    root: PathBuf,
    base: String,
    target: String,
}

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        let temporary = tempfile::tempdir().expect("isolated Git workspace");
        let workspace = temporary.path().to_owned();
        let root = workspace.join("repo");
        fs::create_dir(&root).unwrap();
        git(&root, &["init", "--quiet", "--object-format=sha1"]);
        git(&root, &["config", "user.name", "R write acceptance"]);
        git(&root, &["config", "user.email", "test@example.invalid"]);
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join(PATH), "fn baseline() {}\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "--quiet", "-m", "base"]);
        let base = git(&root, &["rev-parse", "HEAD"]);
        for (path, source) in files {
            fs::write(root.join(path), source).unwrap();
        }
        git(&root, &["add", "."]);
        git(&root, &["commit", "--quiet", "-m", "target"]);
        let target = git(&root, &["rev-parse", "HEAD"]);
        Self {
            _workspace: temporary,
            workspace,
            root,
            base,
            target,
        }
    }

    fn ingest(&self) -> IngestWithSourcesResult {
        let request = IngestRequest::new(
            &self.workspace,
            &self.root,
            "reviewgraphen.acceptance/r-write@1",
            &self.base,
            &self.target,
        );
        ingest_with_sources(&request, 1 << 20).expect("actual source-retaining ingest")
    }
}

fn entry<'a>(
    result: &'a IngestWithSourcesResult,
    path: &str,
    bytes: &[u8],
) -> &'a SnapshotSourceEntry {
    let source = result
        .source_bundle
        .entries()
        .iter()
        .find(|source| source.path() == path)
        .unwrap();
    assert_eq!(source.bytes(), bytes);
    assert_eq!(source.content_hash(), &ContentHash::sha256(bytes));
    assert_eq!(
        result.source_bundle.snapshot_id(),
        result.program_space.snapshot_id()
    );
    assert_eq!(
        result
            .program_space
            .artifact(source.artifact_id())
            .unwrap()
            .kind,
        "file"
    );
    source
}

fn observed_assign(
    source: &SnapshotSourceEntry,
    module_name: Option<&str>,
) -> (
    RustInclusiveLineColumnRange,
    RustInclusiveLineColumnRange,
    RustInclusiveLineColumnRange,
    RustInclusiveLineColumnRange,
) {
    let tree = syn::parse_file(std::str::from_utf8(source.bytes()).unwrap()).unwrap();
    let items = if let Some(name) = module_name {
        let module = tree
            .items
            .iter()
            .find_map(|item| match item {
                syn::Item::Mod(module) if module.ident == name => Some(module),
                _ => None,
            })
            .expect("observed module");
        module.content.as_ref().expect("inline module").1.as_slice()
    } else {
        tree.items.as_slice()
    };
    let caller = items
        .iter()
        .find_map(|item| match item {
            syn::Item::Fn(function) if function.sig.ident == "caller" => Some(function),
            _ => None,
        })
        .expect("free caller");
    let assignment = caller
        .block
        .stmts
        .iter()
        .find_map(|statement| match statement {
            syn::Stmt::Expr(syn::Expr::Assign(assignment), _) => Some(assignment),
            _ => None,
        })
        .expect("actual ExprAssign, not compound/update");
    (
        range_from_start_and_end(caller.sig.fn_token.span, caller.span()),
        range(caller.span()),
        range(assignment.span()),
        range(assignment.left.span()),
    )
}

#[test]
fn test_origin_write_joins_existing_state_relation_not_sibling_function() {
    let fixture = Fixture::new(&[(PATH, SOURCE)]);
    let result = fixture.ingest();
    let source = entry(&result, PATH, SOURCE.as_bytes());
    assert_eq!(SOURCE.len(), 151);
    assert_eq!(source.content_hash().as_str(), SOURCE_SHA256);
    let (syntax_fn_range, raw_fn_span, occurrence, lhs) =
        observed_assign(source, Some("structural"));
    assert_eq!(
        syntax_fn_range,
        RustInclusiveLineColumnRange::new(5, 5, 9, 5).unwrap()
    );
    // The protected producer hashes the attr-inclusive function.span() into the Artifact ID.
    let accepted_fn_location = RustInclusiveLineColumnRange::new(4, 5, 9, 5).unwrap();
    assert_eq!(raw_fn_span, accepted_fn_location);
    assert_ne!(syntax_fn_range, accepted_fn_location);
    assert_eq!(
        occurrence,
        RustInclusiveLineColumnRange::new(7, 9, 7, 17).unwrap()
    );
    assert_eq!(lhs, RustInclusiveLineColumnRange::new(7, 9, 7, 13).unwrap());
    let program = &result.program_space;
    let tests = program
        .artifacts()
        .iter()
        .filter(|artifact| accepted_identity(artifact, source, "test", "caller"))
        .collect::<Vec<_>>();
    assert_eq!(tests.len(), 1);
    let test = tests[0];
    let functions = program
        .artifacts()
        .iter()
        .filter(|artifact| {
            accepted_identity(
                artifact,
                source,
                "function",
                "crate::structural::structural::caller",
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(functions.len(), 1);
    let function = functions[0];
    assert_accepted_location(test, source, accepted_fn_location);
    assert_accepted_location(function, source, accepted_fn_location);
    assert_eq!(program.artifact(&test.id), Some(test));
    assert_eq!(program.artifact(&function.id), Some(function));
    assert_ne!(test.id, function.id);
    let states = program
        .artifacts()
        .iter()
        .filter(|artifact| accepted_identity(artifact, source, "state", "value"))
        .collect::<Vec<_>>();
    assert_eq!(states.len(), 1, "accepted write state identity");
    let state = states[0];
    assert_accepted_location(state, source, occurrence);
    assert_eq!(program.artifact(&state.id), Some(state));
    let writes = program
        .relations()
        .iter()
        .filter(|relation| {
            relation.kind == "writes"
                && relation.directed
                && relation.source_id == test.id
                && relation.target_ids == BTreeSet::from([state.id.clone()])
        })
        .collect::<Vec<_>>();
    assert_eq!(writes.len(), 1, "existing test-origin relation");
    let relation = writes[0];
    assert_eq!(program.relation(&relation.id).unwrap(), relation);
    assert!(
        !program
            .relations()
            .iter()
            .any(|candidate| candidate.kind == "writes"
                && candidate.source_id == function.id
                && candidate.target_ids.contains(&state.id)),
        "test-origin cannot be silently swapped for the function Artifact"
    );
    assert!(
        !program
            .relations()
            .iter()
            .any(|candidate| candidate.kind == "writes"
                && candidate.source_id == state.id
                && candidate.target_ids.contains(&test.id)),
        "direction cannot be reversed"
    );
    let results = result
        .g3_observations()
        .expect("Git-bound write batch")
        .writes();
    assert_eq!(results.len(), 1, "one complete ExprAssign occurrence");
    let matching = results
        .iter()
        .filter(|result| result.occurrence() == occurrence && result.lhs_range() == lhs)
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1);
    let linked = matching[0];
    assert_eq!(linked.source().snapshot_id(), program.snapshot_id());
    assert_eq!(linked.source().file_id(), source.artifact_id());
    assert_eq!(linked.source().canonical_path(), source.path());
    assert_eq!(linked.source().source_hash(), source.content_hash());
    assert!(
        matches!(linked.outcome(), RustG3OutcomeV1::ExistingFact(RustFactRefV1::Relation {
        id, source_id, target_ids }) if id == &relation.id && source_id == &test.id
        && target_ids == &BTreeSet::from([state.id.clone()])),
        "test-origin accepted writes, not sibling function"
    );
}

#[test]
fn non_test_free_caller_uses_function_origin_not_test_origin() {
    const ORDINARY: &str = "fn caller() {\n    let mut value = 0;\n    value = 1;\n}\n";
    let fixture = Fixture::new(&[(PATH, ORDINARY)]);
    let result = fixture.ingest();
    let source = entry(&result, PATH, ORDINARY.as_bytes());
    let (syntax_fn_range, raw_fn_span, occurrence, lhs) = observed_assign(source, None);
    assert_eq!(
        syntax_fn_range, raw_fn_span,
        "ordinary function has no leading attributes"
    );
    let program = &result.program_space;
    let function = program
        .artifacts()
        .iter()
        .find(|artifact| {
            accepted_identity(artifact, source, "function", "crate::structural::caller")
        })
        .expect("ordinary accepted function");
    assert_accepted_location(function, source, syntax_fn_range);
    assert!(!program.artifacts().iter().any(|artifact| {
        artifact.kind == "test"
            && artifact
                .location
                .as_ref()
                .is_some_and(|loc| loc.path == source.path())
            && artifact.label == "caller"
    }));
    let writes = program
        .relations()
        .iter()
        .filter(|relation| {
            relation.kind == "writes"
                && relation.directed
                && relation.source_id == function.id
                && relation.target_ids.iter().any(|id| {
                    program.artifact(id).is_some_and(|artifact| {
                        artifact.kind == "state"
                            && artifact.label == "value"
                            && artifact.language.as_deref() == Some("rust")
                            && artifact.content_hash.as_ref() == Some(source.content_hash())
                            && artifact
                                .location
                                .as_ref()
                                .is_some_and(|loc| loc.path == source.path())
                    })
                })
        })
        .collect::<Vec<_>>();
    assert_eq!(writes.len(), 1);
    let links = result
        .g3_observations()
        .expect("ordinary Git-bound batch")
        .writes();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].source().file_id(), source.artifact_id());
    assert_eq!(links[0].source().source_hash(), source.content_hash());
    assert!(links.iter().any(|link| link.occurrence() == occurrence
        && link.lhs_range() == lhs
        && matches!(link.outcome(), RustG3OutcomeV1::ExistingFact(RustFactRefV1::Relation {
            id, source_id, .. }) if id == &writes[0].id && source_id == &function.id)));
}

fn expected_obstruction_id(
    program: &ProgramSpace,
    source: &SnapshotSourceEntry,
    occurrence: RustInclusiveLineColumnRange,
    lhs: RustInclusiveLineColumnRange,
    owner: RustInclusiveLineColumnRange,
) -> (StableId, StableId) {
    let as_json = |r: RustInclusiveLineColumnRange| {
        json!({
            "start_line": r.start_line(), "start_column": r.start_column(),
            "end_line": r.end_line(), "end_column": r.end_column(),
        })
    };
    let full = BTreeMap::from([
        ("version".to_owned(), json!(2)),
        ("contract".to_owned(), json!("R-write@2")),
        ("profile".to_owned(), json!("rust.g3-admission@2")),
        ("extractor".to_owned(), json!("syn.g3@2")),
        (
            "snapshot".to_owned(),
            json!({"kind":"rust_program","value":program.snapshot_id().as_str()}),
        ),
        ("registry".to_owned(), serde_json::Value::Null),
        ("file_id".to_owned(), json!(source.artifact_id().as_str())),
        ("canonical_source_path".to_owned(), json!(source.path())),
        (
            "source_hash".to_owned(),
            json!(source.content_hash().as_str()),
        ),
        ("coordinate".to_owned(), json!("rust_one_based_inclusive")),
        (
            "syntax".to_owned(),
            json!({"kind":"assignment", "occurrence":as_json(occurrence),
            "lhs":as_json(lhs), "lhs_kind":"field", "owner_syntax":as_json(owner),
            "owner_logical_name": format!("crate::{}::caller", source.path()
                .trim_start_matches("src/").trim_end_matches(".rs"))}),
        ),
    ]);
    let observation = StableId::derived("reviewgraphen.g3.rust.observation.v2", &full).unwrap();
    let mut obstruction = full;
    obstruction.insert("observation_id".to_owned(), json!(observation.as_str()));
    obstruction.insert("reason".to_owned(), json!("unsupported_assignment_lhs@2"));
    obstruction.insert("reason_detail".to_owned(), json!({"lhs_kind":"field"}));
    (
        observation,
        StableId::derived("reviewgraphen.g3.rust.obstruction.v2", &obstruction).unwrap(),
    )
}

#[test]
fn unsupported_field_lhs_is_local_obstruction_and_equal_bytes_other_path_has_distinct_id() {
    const UNSUPPORTED: &str = "fn caller() {\n    let mut s = 0;\n    s.field = 1;\n}\n";
    const OTHER_PATH: &str = "src/other.rs";
    let fixture = Fixture::new(&[(PATH, UNSUPPORTED), (OTHER_PATH, UNSUPPORTED)]);
    let result = fixture.ingest();
    let program = &result.program_space;
    let mut id_a = None;
    let mut ids = Vec::new();
    let batch = result
        .g3_observations()
        .expect("complete equal-byte Git-bound batch");
    assert_eq!(batch.writes().len(), 2);
    for path in [PATH, OTHER_PATH] {
        let source = entry(&result, path, UNSUPPORTED.as_bytes());
        let (syntax_fn_range, raw_fn_span, occurrence, lhs) = observed_assign(source, None);
        assert_eq!(
            syntax_fn_range, raw_fn_span,
            "unsupported free function has no attributes"
        );
        let tree = syn::parse_file(std::str::from_utf8(source.bytes()).unwrap()).unwrap();
        let function = match &tree.items[0] {
            syn::Item::Fn(function) => function,
            _ => panic!("free function"),
        };
        assert!(function.block.stmts.iter().any(|stmt| matches!(stmt,
            syn::Stmt::Expr(syn::Expr::Assign(assign), _) if matches!(assign.left.as_ref(), syn::Expr::Field(_)))));
        let results = batch
            .writes()
            .iter()
            .filter(|row| row.source().canonical_path() == path)
            .collect::<Vec<_>>();
        assert_eq!(
            results.len(),
            1,
            "one write per independently selected admitted path"
        );
        let result = results
            .iter()
            .find(|result| result.occurrence() == occurrence && result.lhs_range() == lhs)
            .expect("source-observed assignment is represented");
        assert_eq!(result.source().snapshot_id(), program.snapshot_id());
        assert_eq!(
            result.source().file_id(),
            source.artifact_id(),
            "actual path's file Artifact ID"
        );
        assert_eq!(result.source().canonical_path(), source.path());
        assert_eq!(result.source().source_hash(), source.content_hash());
        assert_eq!(format!("{:?}", result.lhs_kind()), "Field");
        match result.outcome() {
            RustG3OutcomeV1::Obstructed {
                obstruction_id,
                reason,
            } => {
                assert!(format!("{reason:?}").contains("UnsupportedAssignmentLhs"));
                let (expected_observation, expected) =
                    expected_obstruction_id(program, source, occurrence, lhs, syntax_fn_range);
                assert_eq!(
                    result.id(),
                    &expected_observation,
                    "full independent observation preimage"
                );
                assert_eq!(
                    obstruction_id, &expected,
                    "full F4 preimage includes canonical path independently of file ID"
                );
                assert!(
                    batch.obstruction(&expected).is_some(),
                    "typed obstruction retained in batch"
                );
                assert!(program.artifact(&expected).is_none());
                assert!(program.relation(&expected).is_none());
                if path == PATH {
                    id_a = Some(expected.clone());
                }
                ids.push(expected);
            }
            RustG3OutcomeV1::ExistingFact(_) => panic!("field syntax cannot mint accepted writes"),
        }
    }
    assert_eq!(ids.len(), 2);
    assert_eq!(id_a.as_ref(), Some(&ids[0]));
    assert_ne!(
        ids[0], ids[1],
        "full F4 preimages differ despite equal bytes/ranges"
    );
    const OVERLAP: &str = "fn caller() { let mut a = 0; let mut b = 0; a = b = 1; a += 1; }\n";
    let tree = syn::parse_file(OVERLAP).unwrap();
    struct Count {
        assignments: usize,
        compound: usize,
    }
    impl<'ast> syn::visit::Visit<'ast> for Count {
        fn visit_expr_assign(&mut self, node: &'ast syn::ExprAssign) {
            self.assignments += 1;
            syn::visit::visit_expr_assign(self, node);
        }
        fn visit_expr_binary(&mut self, node: &'ast syn::ExprBinary) {
            if matches!(node.op, syn::BinOp::AddAssign(_)) {
                self.compound += 1;
            }
            syn::visit::visit_expr_binary(self, node);
        }
    }
    let mut count = Count {
        assignments: 0,
        compound: 0,
    };
    syn::visit::Visit::visit_file(&mut count, &tree);
    assert_eq!((count.assignments, count.compound), (2, 1));
    let overlapping = Fixture::new(&[(PATH, OVERLAP)]).ingest();
    let overlap_batch = overlapping
        .g3_observations()
        .expect("overlap is a typed result, not a parse failure");
    assert_eq!(
        overlap_batch.writes().len(),
        2,
        "nested assignments both accounted"
    );
    assert!(overlap_batch.writes().iter().all(|row| matches!(row.outcome(),
        RustG3OutcomeV1::Obstructed { reason, .. } if format!("{reason:?}").contains("OverlappingOccurrence"))));
    assert!(
        overlap_batch
            .construct_partition()
            .exclusions()
            .iter()
            .any(|row| format!("{row:?}").contains("CompoundAssignment")),
        "parsed compound expression has an explicit exclusion witness outside ExprAssign denominator"
    );
}
