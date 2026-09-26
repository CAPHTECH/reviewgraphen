//! R-test-marker@2: exact #[test] observed inside Git-bound admission only.

use crate::g3::rust::{RustFactRefV1, RustG3OutcomeV1, RustInclusiveLineColumnRange};
use crate::{IngestRequest, IngestWithSourcesResult, ingest_with_sources};
use reviewgraphen_core::{ContentHash, SnapshotSourceEntry};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use syn::spanned::Spanned;

const PATH: &str = "src/structural.rs";
const SOURCE: &str = "#[cfg(test)]\nmod structural {\n    fn callee() {}\n    #[test]\n    fn caller() {\n        let mut value = 0;\n        value = 1;\n        callee();\n    }\n}\n";
const SOURCE_SHA256: &str =
    "sha256:e22111d84a1611a511f1c7bccaeca7abb03c058ab51fc753511c75e3f087eadd";

fn observed(span: proc_macro2::Span) -> RustInclusiveLineColumnRange {
    observed_from_start_and_end(span, span)
}

fn observed_from_start_and_end(
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
    .expect("nonzero inclusive Rust span")
}

fn accepted_test_identity(
    artifact: &reviewgraphen_core::Artifact,
    source: &SnapshotSourceEntry,
    label: &str,
) -> bool {
    artifact.kind == "test"
        && artifact.label == label
        && artifact.language.as_deref() == Some("rust")
        && artifact.content_hash.as_ref() == Some(source.content_hash())
        && artifact
            .location
            .as_ref()
            .is_some_and(|loc| loc.path == source.path())
}

fn assert_accepted_location(
    artifact: &reviewgraphen_core::Artifact,
    source: &SnapshotSourceEntry,
    accepted_full: RustInclusiveLineColumnRange,
) {
    assert!(
        artifact.location.as_ref().is_some_and(|loc| {
            loc.path == source.path()
                && loc.start_line == Some(accepted_full.start_line())
                && loc.start_column == Some(accepted_full.start_column())
                && loc.end_line == Some(accepted_full.end_line())
                && loc.end_column == Some(accepted_full.end_column())
        }),
        "accepted attribute-inclusive test Artifact location"
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
    fn new(source: &str) -> Self {
        let temporary = tempfile::tempdir().expect("isolated Git fixture");
        let workspace = temporary.path().to_owned();
        let root = workspace.join("repo");
        fs::create_dir(&root).unwrap();
        git(&root, &["init", "--quiet", "--object-format=sha1"]);
        git(&root, &["config", "user.name", "R marker acceptance"]);
        git(&root, &["config", "user.email", "test@example.invalid"]);
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join(PATH), "fn baseline() {}\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "--quiet", "-m", "base"]);
        let base = git(&root, &["rev-parse", "HEAD"]);
        fs::write(root.join(PATH), source).unwrap();
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
            "reviewgraphen.acceptance/r-test-marker@1",
            &self.base,
            &self.target,
        );
        ingest_with_sources(&request, 1 << 20).expect("actual source-retaining Git ingest")
    }
}

fn entry<'a>(result: &'a IngestWithSourcesResult, bytes: &[u8]) -> &'a SnapshotSourceEntry {
    let source = result
        .source_bundle
        .entries()
        .iter()
        .find(|source| source.path() == PATH)
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

#[test]
fn unqualified_attribute_has_its_own_range_and_existing_full_function_test_artifact() {
    let fixture = Fixture::new(SOURCE);
    let result = fixture.ingest();
    let source = entry(&result, SOURCE.as_bytes());
    assert_eq!(SOURCE.len(), 151);
    assert_eq!(source.content_hash().as_str(), SOURCE_SHA256);
    let tree = syn::parse_file(std::str::from_utf8(source.bytes()).unwrap()).unwrap();
    let module = match &tree.items[0] {
        syn::Item::Mod(module) => module,
        _ => panic!("inline module"),
    };
    let caller = module
        .content
        .as_ref()
        .expect("inline module")
        .1
        .iter()
        .find_map(|item| match item {
            syn::Item::Fn(function) if function.sig.ident == "caller" => Some(function),
            _ => None,
        })
        .expect("direct free caller");
    let marker = caller
        .attrs
        .iter()
        .find(|attribute| attribute.path().is_ident("test"))
        .expect("exact unqualified test attribute");
    let occurrence = observed(marker.span());
    let function_range = observed_from_start_and_end(caller.sig.fn_token.span, caller.span());
    let accepted_function_location = observed(caller.span());
    assert_eq!(
        occurrence,
        RustInclusiveLineColumnRange::new(4, 5, 4, 11).unwrap()
    );
    assert_eq!(
        function_range,
        RustInclusiveLineColumnRange::new(5, 5, 9, 5).unwrap()
    );
    assert_eq!(
        accepted_function_location,
        RustInclusiveLineColumnRange::new(4, 5, 9, 5).unwrap()
    );
    assert_ne!(
        occurrence, accepted_function_location,
        "marker occurrence is not accepted Artifact location"
    );
    assert_ne!(
        function_range, accepted_function_location,
        "syntax start excludes the test attribute"
    );
    let program = &result.program_space;
    let candidates = program
        .artifacts()
        .iter()
        .filter(|artifact| accepted_test_identity(artifact, source, "caller"))
        .collect::<Vec<_>>();
    assert_eq!(candidates.len(), 1, "one accepted test Artifact identity");
    let accepted = candidates[0];
    assert_accepted_location(accepted, source, accepted_function_location);
    assert_eq!(program.artifact(&accepted.id).unwrap(), accepted);
    let links = result
        .g3_observations()
        .expect("internal Git-bound markers")
        .test_markers();
    assert_eq!(
        links.len(),
        1,
        "only the exact test attribute is in the marker denominator"
    );
    let matching = links
        .iter()
        .filter(|link| {
            link.occurrence() == occurrence && link.function_syntax_range() == function_range
        })
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1);
    let link = matching[0];
    assert_eq!(link.source().snapshot_id(), program.snapshot_id());
    assert_eq!(link.source().file_id(), source.artifact_id());
    assert_eq!(link.source().canonical_path(), source.path());
    assert_eq!(link.source().source_hash(), source.content_hash());
    assert!(
        matches!(link.outcome(), RustG3OutcomeV1::ExistingFact(RustFactRefV1::Artifact { id })
        if id == &accepted.id),
        "existing test Artifact, not inferred from marker text"
    );
}

#[test]
fn qualified_cfg_and_unrelated_attributes_are_excluded_even_if_legacy_marks_test() {
    const NEGATIVE: &str = "#[tokio::test]\nfn qualified_tokio() {}\n#[foo::test]\nfn qualified_foo() {}\n#[cfg(test)]\nfn cfg_only() {}\n#[inline]\nfn inline_only() {}\n";
    let fixture = Fixture::new(NEGATIVE);
    let result = fixture.ingest();
    let source = entry(&result, NEGATIVE.as_bytes());
    let tree = syn::parse_file(std::str::from_utf8(source.bytes()).unwrap()).unwrap();
    let functions = tree
        .items
        .iter()
        .map(|item| match item {
            syn::Item::Fn(function) => function,
            _ => panic!("expected parsed free function"),
        })
        .collect::<Vec<_>>();
    assert_eq!(functions.len(), 4);
    let spellings = ["tokio::test", "foo::test", "cfg", "inline"];
    for (function, spelling) in functions.iter().zip(spellings) {
        assert_eq!(function.attrs.len(), 1, "source-observed candidate");
        let attribute = &function.attrs[0];
        assert_eq!(
            attribute
                .path()
                .segments
                .iter()
                .map(|part| part.ident.to_string())
                .collect::<Vec<_>>()
                .join("::"),
            spelling
        );
        assert!(
            !attribute.path().is_ident("test"),
            "only exact unqualified #[test] is accepted"
        );
        let occurrence = observed(attribute.span());
        let syntax_full = observed_from_start_and_end(function.sig.fn_token.span, function.span());
        let accepted_full = observed(function.span());
        assert_ne!(occurrence, syntax_full);
        assert_ne!(
            syntax_full, accepted_full,
            "leading negative attribute is excluded from syntax start"
        );
        assert!(
            result
                .program_space
                .artifacts()
                .iter()
                .any(|artifact| artifact.kind == "function"
                    && artifact.label.ends_with(&function.sig.ident.to_string())
                    && artifact.language.as_deref() == Some("rust")
                    && artifact.content_hash.as_ref() == Some(source.content_hash())
                    && artifact
                        .location
                        .as_ref()
                        .is_some_and(|loc| loc.path == source.path()
                            && loc.start_line == Some(accepted_full.start_line())
                            && loc.start_column == Some(accepted_full.start_column())
                            && loc.end_line == Some(accepted_full.end_line())
                            && loc.end_column == Some(accepted_full.end_column()))),
            "negative is parsed and admitted; not an absent parser artifact"
        );
    }
    assert!(
        result
            .program_space
            .artifacts()
            .iter()
            .any(|artifact| artifact.kind == "test" && artifact.label == "qualified_tokio"),
        "legacy broad producer marks qualified attribute test"
    );
    assert!(
        result
            .program_space
            .artifacts()
            .iter()
            .any(|artifact| artifact.kind == "test" && artifact.label == "qualified_foo"),
        "second qualified spelling also receives legacy test Artifact"
    );
    let batch = result.g3_observations().expect("negative Git-bound batch");
    let links = batch.test_markers();
    assert!(
        links.is_empty(),
        "qualified test/cfg(test)/inline yield no marker links"
    );
    assert!(
        !batch
            .obstructions()
            .iter()
            .any(|row| format!("{row:?}").contains("R-test-marker")),
        "excluded attributes do not become marker obstructions either"
    );
}
