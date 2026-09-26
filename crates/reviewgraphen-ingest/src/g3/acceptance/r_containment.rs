//! R-containment@2: admission-bound direct syntax pairs and existing directed relation.
//! An accepted module Artifact's 1:1 placeholder is NOT a module syntax span.

use crate::g3::rust::{RustFactRefV1, RustG3OutcomeV1, RustInclusiveLineColumnRange};
use crate::{IngestRequest, ingest_with_sources};
use reviewgraphen_core::{ContentHash, Location, ProgramSpace, SnapshotSourceEntry};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use syn::spanned::Spanned;

const PATH: &str = "src/structural.rs";
const SOURCE: &str = "#[cfg(test)]\nmod structural {\n    fn callee() {}\n    #[test]\n    fn caller() {\n        let mut value = 0;\n        value = 1;\n        callee();\n    }\n}\n";
const SOURCE_SHA256: &str =
    "sha256:e22111d84a1611a511f1c7bccaeca7abb03c058ab51fc753511c75e3f087eadd";

fn range(sl: u64, sc: u64, el: u64, ec: u64) -> RustInclusiveLineColumnRange {
    RustInclusiveLineColumnRange::new(sl, sc, el, ec).expect("nonzero inclusive Rust range")
}

fn observed(span: proc_macro2::Span) -> RustInclusiveLineColumnRange {
    observed_from_start_and_end(span, span)
}

fn observed_from_start_and_end(
    start_span: proc_macro2::Span,
    end_span: proc_macro2::Span,
) -> RustInclusiveLineColumnRange {
    let start = start_span.start();
    let end = end_span.end();
    range(
        start.line as u64,
        start.column as u64 + 1,
        end.line as u64,
        end.column as u64,
    )
}

fn location_is(location: &Location, path: &str, span: RustInclusiveLineColumnRange) -> bool {
    location.path == path
        && location.start_line == Some(span.start_line())
        && location.start_column == Some(span.start_column())
        && location.end_line == Some(span.end_line())
        && location.end_column == Some(span.end_column())
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
        .expect("fixture git launches");
    assert!(
        output.status.success(),
        "fixture git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git output UTF-8")
        .trim()
        .to_owned()
}

struct Fixture {
    _workspace: tempfile::TempDir,
    root: PathBuf,
    workspace: PathBuf,
    base: String,
    target: String,
}

impl Fixture {
    fn new(source: &str) -> Self {
        let temporary = tempfile::tempdir().expect("isolated fixture workspace");
        let workspace = temporary.path().to_owned();
        let root = workspace.join("repo");
        fs::create_dir(&root).expect("fixture repository");
        git(&root, &["init", "--quiet", "--object-format=sha1"]);
        git(&root, &["config", "user.name", "R containment acceptance"]);
        git(&root, &["config", "user.email", "test@example.invalid"]);
        fs::create_dir(root.join("src")).expect("source directory");
        fs::write(root.join(PATH), "fn baseline() {}\n").expect("base source");
        git(&root, &["add", "."]);
        git(&root, &["commit", "--quiet", "-m", "base"]);
        let base = git(&root, &["rev-parse", "HEAD"]);
        fs::write(root.join(PATH), source).expect("target source");
        git(&root, &["add", "."]);
        git(&root, &["commit", "--quiet", "-m", "target"]);
        let target = git(&root, &["rev-parse", "HEAD"]);
        Self {
            _workspace: temporary,
            root,
            workspace,
            base,
            target,
        }
    }

    fn ingest(&self) -> crate::IngestWithSourcesResult {
        let request = IngestRequest::new(
            &self.workspace,
            &self.root,
            "reviewgraphen.acceptance/r-containment@1",
            &self.base,
            &self.target,
        );
        ingest_with_sources(&request, 1 << 20).expect("actual source-retaining Git ingest")
    }
}

fn source_entry<'a>(
    result: &'a crate::IngestWithSourcesResult,
    bytes: &[u8],
) -> &'a SnapshotSourceEntry {
    let entry = result
        .source_bundle
        .entries()
        .iter()
        .find(|entry| entry.path() == PATH)
        .expect("admitted source path");
    assert_eq!(entry.bytes(), bytes);
    assert_eq!(entry.content_hash(), &ContentHash::sha256(bytes));
    assert_eq!(entry.cas_hash(), entry.content_hash());
    assert_eq!(
        result.source_bundle.snapshot_id(),
        result.program_space.snapshot_id()
    );
    assert_eq!(
        result
            .program_space
            .artifact(entry.artifact_id())
            .expect("accepted file")
            .kind,
        "file"
    );
    entry
}

fn accepted_function<'a>(
    program: &'a ProgramSpace,
    entry: &SnapshotSourceEntry,
    label: &str,
    accepted_location: RustInclusiveLineColumnRange,
) -> &'a reviewgraphen_core::Artifact {
    let matches = program
        .artifacts()
        .iter()
        .filter(|artifact| {
            artifact.kind == "function"
                && artifact.language.as_deref() == Some("rust")
                && artifact.content_hash.as_ref() == Some(entry.content_hash())
                && artifact.label == label
                && artifact
                    .location
                    .as_ref()
                    .is_some_and(|loc| loc.path == entry.path())
        })
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 1, "one existing child function identity");
    assert!(
        matches[0].location.as_ref().is_some_and(|loc| location_is(
            loc,
            entry.path(),
            accepted_location
        )),
        "accepted attr-inclusive function location"
    );
    matches[0]
}

#[test]
fn inline_parent_source_range_and_direct_child_join_the_existing_contains_relation() {
    let fixture = Fixture::new(SOURCE);
    let result = fixture.ingest();
    let entry = source_entry(&result, SOURCE.as_bytes());
    assert_eq!(SOURCE.len(), 151);
    assert_eq!(entry.content_hash().as_str(), SOURCE_SHA256);

    // Independent syn observation over admitted bytes; never read the link or module Artifact
    // to derive the parent/child ranges. Direct ownership is the module's immediate item list.
    let tree = syn::parse_file(std::str::from_utf8(entry.bytes()).expect("UTF-8 source"))
        .expect("syn parses admitted bytes");
    let module = tree
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Mod(module) if module.ident == "structural" => Some(module),
            _ => None,
        })
        .expect("inline structural module");
    let children = &module.content.as_ref().expect("inline, not out-of-line").1;
    let callee = children
        .iter()
        .find_map(|item| match item {
            syn::Item::Fn(function) if function.sig.ident == "callee" => Some(function),
            _ => None,
        })
        .expect("directly owned free callee");
    let caller = children
        .iter()
        .find_map(|item| match item {
            syn::Item::Fn(function) if function.sig.ident == "caller" => Some(function),
            _ => None,
        })
        .expect("directly owned free caller");
    let raw_parent_span = observed(module.span());
    let raw_child_span = observed(caller.span());
    let parent_range = observed_from_start_and_end(module.mod_token.span, module.span());
    let child_range = observed_from_start_and_end(caller.sig.fn_token.span, caller.span());
    let callee_range = observed_from_start_and_end(callee.sig.fn_token.span, callee.span());
    assert_eq!(raw_parent_span, range(1, 1, 10, 1));
    assert_eq!(raw_child_span, range(4, 5, 9, 5));
    assert_eq!(parent_range, range(2, 1, 10, 1));
    assert_eq!(child_range, range(5, 5, 9, 5));
    assert_eq!(callee_range, range(3, 5, 3, 18));

    let program = &result.program_space;
    let candidates = program
        .artifacts()
        .iter()
        .filter(|artifact| {
            artifact.kind == "module"
                && artifact.language.as_deref() == Some("rust")
                && artifact.content_hash.as_ref() == Some(entry.content_hash())
                && artifact
                    .location
                    .as_ref()
                    .is_some_and(|loc| loc.path == entry.path())
                && artifact.label == "crate::structural::structural"
        })
        .collect::<Vec<_>>();
    // The exact module identity is the accepted logical suffix, not its placeholder location.
    assert_eq!(
        candidates.len(),
        1,
        "unique existing inline module identity"
    );
    let parent = candidates[0];
    assert_eq!(parent.label, "crate::structural::structural");
    assert!(
        parent.location.as_ref().is_some_and(|loc| location_is(
            loc,
            entry.path(),
            range(1, 1, 1, 1)
        )),
        "accepted module placeholder is not the source-observed span"
    );
    let child = accepted_function(
        program,
        entry,
        "crate::structural::structural::caller",
        raw_child_span,
    );
    let callee_artifact = accepted_function(
        program,
        entry,
        "crate::structural::structural::callee",
        observed(callee.span()),
    );
    assert_ne!(
        child_range, raw_child_span,
        "syntax start excludes attribute; accepted ID/location does not"
    );
    let expected_targets = BTreeSet::from([child.id.clone()]);
    let relations = program
        .relations()
        .iter()
        .filter(|relation| {
            relation.kind == "contains"
                && relation.directed
                && relation.source_id == parent.id
                && relation.target_ids == expected_targets
        })
        .collect::<Vec<_>>();
    assert_eq!(
        relations.len(),
        1,
        "one already accepted directed contains relation"
    );
    let relation = relations[0];
    assert_eq!(
        program.relation(&relation.id).expect("existing relation"),
        relation
    );
    assert_eq!(
        program.artifact(&parent.id).expect("existing parent"),
        parent
    );
    assert_eq!(program.artifact(&child.id).expect("existing child"), child);
    assert_eq!(program.artifact(&callee_artifact.id), Some(callee_artifact));

    let callee_targets = BTreeSet::from([callee_artifact.id.clone()]);
    let callee_relations = program
        .relations()
        .iter()
        .filter(|candidate| {
            candidate.kind == "contains"
                && candidate.directed
                && candidate.source_id == parent.id
                && candidate.target_ids == callee_targets
        })
        .collect::<Vec<_>>();
    let [callee_relation] = callee_relations.as_slice() else {
        panic!("unique preexisting directed module-to-callee relation")
    };
    assert_eq!(
        program.relation(&callee_relation.id),
        Some(*callee_relation)
    );

    let links = result
        .g3_observations()
        .expect("Git-bound containment batch")
        .containment();
    assert_eq!(
        links.len(),
        2,
        "both immediate free-function children are accounted for"
    );
    let expected_pairs = [
        (callee_range, *callee_relation, &callee_targets),
        (child_range, relation, &expected_targets),
    ];
    assert_eq!(
        links
            .iter()
            .map(|link| (link.parent_range(), link.child_range()))
            .collect::<BTreeSet<_>>(),
        expected_pairs
            .iter()
            .map(|(child, _, _)| (parent_range, *child))
            .collect::<BTreeSet<_>>(),
        "complete independent direct-pair set"
    );
    for (observed_child, accepted_relation, targets) in expected_pairs {
        let matching = links
            .iter()
            .filter(|link| {
                link.parent_range() == parent_range && link.child_range() == observed_child
            })
            .collect::<Vec<_>>();
        let [link] = matching.as_slice() else {
            panic!("one row per direct module child")
        };
        assert_eq!(link.source().snapshot_id(), program.snapshot_id());
        assert_eq!(link.source().file_id(), entry.artifact_id());
        assert_eq!(link.source().canonical_path(), entry.path());
        assert_eq!(link.source().source_hash(), entry.content_hash());
        assert!(
            matches!(link.outcome(), RustG3OutcomeV1::ExistingFact(RustFactRefV1::Relation {
            id, source_id, target_ids }) if id == &accepted_relation.id
            && source_id == &parent.id && target_ids == targets),
            "every direct child resolves its own preexisting directed contains, not Obstructed"
        );
    }
    assert!(
        !links
            .iter()
            .any(|link| link.parent_range() == child_range && link.child_range() == parent_range),
        "swapped endpoints must never be sealed"
    );
}

#[test]
fn parsed_out_of_line_root_and_nested_non_direct_children_do_not_invent_links() {
    const NEGATIVE: &str =
        "mod outer {\n    mod nested { fn inner() {} }\n}\nmod external;\nfn root() {}\n";
    let fixture = Fixture::new(NEGATIVE);
    let result = fixture.ingest();
    let entry = source_entry(&result, NEGATIVE.as_bytes());
    let tree = syn::parse_file(std::str::from_utf8(entry.bytes()).unwrap()).unwrap();
    let outer = match &tree.items[0] {
        syn::Item::Mod(module) => module,
        _ => panic!("outer mod"),
    };
    let nested = match &outer.content.as_ref().expect("outer inline").1[0] {
        syn::Item::Mod(module) => module,
        _ => panic!("nested mod"),
    };
    let inner = match &nested.content.as_ref().expect("nested inline").1[0] {
        syn::Item::Fn(function) => function,
        _ => panic!("inner free function"),
    };
    let out_of_line = match &tree.items[1] {
        syn::Item::Mod(module) => module,
        _ => panic!("out-of-line module"),
    };
    let root_fn = match &tree.items[2] {
        syn::Item::Fn(function) => function,
        _ => panic!("root function"),
    };
    assert!(out_of_line.content.is_none());
    assert!(
        outer
            .content
            .as_ref()
            .unwrap()
            .1
            .iter()
            .all(|item| !matches!(item, syn::Item::Fn(_)))
    );
    let outer_range = observed_from_start_and_end(outer.mod_token.span, outer.span());
    let nested_range = observed_from_start_and_end(nested.mod_token.span, nested.span());
    let inner_range = observed_from_start_and_end(inner.sig.fn_token.span, inner.span());
    let root_range = observed_from_start_and_end(root_fn.sig.fn_token.span, root_fn.span());
    let program = &result.program_space;
    let outer_artifact = program
        .artifacts()
        .iter()
        .find(|artifact| artifact.kind == "module" && artifact.label.ends_with("::outer"))
        .expect("accepted outer module");
    let nested_candidates = program
        .artifacts()
        .iter()
        .filter(|artifact| {
            artifact.kind == "module"
                && artifact.language.as_deref() == Some("rust")
                && artifact.label == "crate::structural::outer::nested"
                && artifact.content_hash.as_ref() == Some(entry.content_hash())
                && artifact
                    .location
                    .as_ref()
                    .is_some_and(|loc| loc.path == PATH)
        })
        .collect::<Vec<_>>();
    let [nested_artifact] = nested_candidates.as_slice() else {
        panic!("unique accepted nested module identity independently of token coordinates")
    };
    assert_eq!(
        program.artifact(&nested_artifact.id),
        Some(*nested_artifact)
    );
    let inner_artifact = accepted_function(
        program,
        entry,
        "crate::structural::outer::nested::inner",
        observed(inner.span()),
    );
    let inner_targets = BTreeSet::from([inner_artifact.id.clone()]);
    let directed = program
        .relations()
        .iter()
        .filter(|relation| {
            relation.kind == "contains"
                && relation.directed
                && relation.source_id == nested_artifact.id
                && relation.target_ids == inner_targets
        })
        .collect::<Vec<_>>();
    let [inner_relation] = directed.as_slice() else {
        panic!("unique existing directed nested-module-to-inner-function relation")
    };
    assert_eq!(program.relation(&inner_relation.id), Some(*inner_relation));
    assert!(
        !program
            .relations()
            .iter()
            .any(|relation| relation.kind == "contains"
                && relation.source_id == outer_artifact.id
                && relation.target_ids.contains(&inner_artifact.id)),
        "outer-to-inner is not direct"
    );
    let links = result
        .g3_observations()
        .expect("negative Git-bound batch")
        .containment();
    assert_eq!(
        links.len(),
        1,
        "nested child is the only direct module-to-function pair"
    );
    assert!(
        links
            .iter()
            .all(|link| link.source().file_id() == entry.artifact_id()
                && link.source().source_hash() == entry.content_hash())
    );
    assert!(
        !links
            .iter()
            .any(|link| link.parent_range() == outer_range && link.child_range() == inner_range)
    );
    assert!(!links.iter().any(|link| link.parent_range()
        == observed_from_start_and_end(out_of_line.mod_token.span, out_of_line.span())
        || link.child_range() == root_range));
    assert!(
        links
            .iter()
            .any(|link| link.parent_range() == nested_range && link.child_range() == inner_range),
        "nested module's *own* direct child remains observable"
    );
    let [inner_link] = links else {
        panic!("only the nested module's direct child")
    };
    assert_eq!(
        (inner_link.parent_range(), inner_link.child_range()),
        (nested_range, inner_range)
    );
    assert_eq!(inner_link.source().snapshot_id(), program.snapshot_id());
    assert_eq!(inner_link.source().file_id(), entry.artifact_id());
    assert_eq!(inner_link.source().canonical_path(), PATH);
    assert_eq!(inner_link.source().source_hash(), entry.content_hash());
    assert!(
        matches!(inner_link.outcome(), RustG3OutcomeV1::ExistingFact(RustFactRefV1::Relation {
        id, source_id, target_ids }) if id == &inner_relation.id
        && source_id == &nested_artifact.id && target_ids == &inner_targets),
        "nested direct child must resolve its own accepted relation, never Obstructed"
    );
    assert!(
        result
            .g3_observations()
            .unwrap()
            .construct_partition()
            .exclusions()
            .iter()
            .any(|row| format!("{row:?}").contains("out_of_line")),
        "out-of-line module explicitly excluded"
    );
}
