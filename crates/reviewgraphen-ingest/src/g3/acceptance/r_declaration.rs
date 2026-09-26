//! R-declaration@2: admission observes free ItemFn from its own retained Git bytes.

use crate::g3::rust::{RustFactRefV1, RustG3OutcomeV1, RustInclusiveLineColumnRange};
use crate::{IngestRequest, ingest_with_sources_v2};
use reviewgraphen_core::ContentHash;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

const PATH: &str = "src/structural.rs";
const SOURCE: &str = "#[cfg(test)]\nmod structural {\n    fn callee() {}\n    #[test]\n    fn caller() {\n        let mut value = 0;\n        value = 1;\n        callee();\n    }\n}\n";
const SOURCE_SHA256: &str =
    "sha256:e22111d84a1611a511f1c7bccaeca7abb03c058ab51fc753511c75e3f087eadd";
// There is no free ItemFn here. The method body contains an actual closure.
const NEGATIVE: &str = "struct Holder;\nimpl Holder {\n    fn associated(&self) {\n        let callback = || {};\n        callback();\n    }\n}\n";
const GIT_DATE: &str = "2000-01-01T00:00:00Z";

struct Fixture {
    _workspace: tempfile::TempDir,
    workspace: PathBuf,
    root: PathBuf,
    base: String,
    target: String,
}

impl Fixture {
    fn request(&self) -> IngestRequest {
        IngestRequest::new(
            &self.workspace,
            &self.root,
            "reviewgraphen.test/g3-r-declaration",
            &self.base,
            &self.target,
        )
    }
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join(".xdg-config"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_AUTHOR_DATE", GIT_DATE)
        .env("GIT_COMMITTER_DATE", GIT_DATE)
        .args(args)
        .output()
        .expect("fixture Git launches");
    assert!(
        output.status.success(),
        "fixture Git operation {:?} succeeds: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("fixture Git output is UTF-8")
        .trim()
        .to_owned()
}

fn fixture(body: &str) -> Fixture {
    let workspace = tempfile::tempdir().expect("fixture temporary workspace");
    let workspace_root = workspace.path().to_owned();
    let root = workspace_root.join("fixture");
    fs::create_dir(&root).expect("fixture repository directory");
    git(&root, &["init", "--quiet", "--object-format=sha1"]);
    git(
        &root,
        &["config", "user.email", "reviewgraphen@example.test"],
    );
    git(&root, &["config", "user.name", "ReviewGraphen test"]);
    let source_path = root.join(PATH);
    fs::create_dir_all(source_path.parent().expect("literal path has parent"))
        .expect("fixture src directory");
    fs::write(&source_path, "fn baseline() {}\n").expect("baseline source");
    git(&root, &["add", "."]);
    git(&root, &["commit", "--quiet", "-m", "base"]);
    let base = git(&root, &["rev-parse", "HEAD"]);
    fs::write(&source_path, body).expect("target source");
    git(&root, &["add", "."]);
    git(&root, &["commit", "--quiet", "-m", "declaration source"]);
    let target = git(&root, &["rev-parse", "HEAD"]);
    Fixture {
        _workspace: workspace,
        workspace: workspace_root,
        root,
        base,
        target,
    }
}

fn range() -> RustInclusiveLineColumnRange {
    RustInclusiveLineColumnRange::new(3, 5, 3, 18).expect("literal inclusive ItemFn span")
}

fn syntax_range(first: proc_macro2::Span, full: proc_macro2::Span) -> RustInclusiveLineColumnRange {
    let start = first.start();
    let end = full.end();
    RustInclusiveLineColumnRange::new(
        start.line as u64,
        start.column as u64 + 1,
        end.line as u64,
        end.column as u64,
    )
    .unwrap()
}

#[test]
fn free_item_fn_links_exact_accepted_function_artifact_and_full_source_binding() {
    assert_eq!(SOURCE.len(), 151, "frozen source byte count");
    assert_eq!(
        ContentHash::sha256(SOURCE.as_bytes()).as_str(),
        SOURCE_SHA256
    );
    let fixture = fixture(SOURCE);
    let parsed = syn::parse_file(SOURCE).expect("independent syn fixture parse");
    let syn::Item::Mod(module) = &parsed.items[0] else {
        panic!("inline module")
    };
    let functions = module
        .content
        .as_ref()
        .expect("module body")
        .1
        .iter()
        .filter_map(|item| {
            if let syn::Item::Fn(function) = item {
                Some(function)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(functions.len(), 2);
    assert_eq!(functions[0].sig.ident, "callee");
    assert_eq!(functions[1].sig.ident, "caller");
    let callee_range = syntax_range(functions[0].sig.fn_token.span, functions[0].span());
    let caller_range = syntax_range(functions[1].sig.fn_token.span, functions[1].span());
    assert_eq!(callee_range, range());
    assert_eq!(
        caller_range,
        RustInclusiveLineColumnRange::new(5, 5, 9, 5).unwrap()
    );
    assert_eq!(
        syntax_range(functions[1].span(), functions[1].span()),
        RustInclusiveLineColumnRange::new(4, 5, 9, 5).unwrap(),
        "accepted location includes attribute"
    );
    // Mutate the mutable checkout before admission: reading repository_root/PATH
    // instead of the selected commit's blob would now lose BOTH expected functions.
    fs::write(fixture.root.join(PATH), "fn dirty_before_admission() {}\n").unwrap();
    assert_ne!(
        fs::read(fixture.root.join(PATH)).unwrap(),
        SOURCE.as_bytes()
    );
    let result =
        ingest_with_sources_v2(&fixture.request(), u64::MAX).expect("source-retaining Rust ingest");
    let program = &result.legacy.program_space;
    let bundle = &result.legacy.source_bundle;
    assert_eq!(bundle.snapshot_id(), program.snapshot_id());
    let [entry] = bundle.entries() else {
        panic!("one admitted Rust source entry");
    };
    assert_eq!(entry.path(), PATH);
    assert_eq!(entry.bytes(), SOURCE.as_bytes());
    assert_eq!(entry.content_hash().as_str(), SOURCE_SHA256);
    assert_eq!(entry.cas_hash().as_str(), SOURCE_SHA256);
    let file = program
        .artifact(entry.artifact_id())
        .expect("entry resolves to accepted file Artifact");
    assert_eq!(file.kind, "file");
    assert_eq!(file.language.as_deref(), Some("rust"));
    assert_eq!(file.content_hash.as_ref(), Some(entry.content_hash()));
    assert_eq!(file.location.as_ref().expect("file location").path, PATH);

    let expected_functions = program
        .artifacts()
        .iter()
        .filter(|artifact| {
            artifact.kind == "function"
                && artifact.language.as_deref() == Some("rust")
                && artifact.content_hash.as_ref() == Some(entry.content_hash())
                && artifact.location.as_ref().is_some_and(|location| {
                    location.path == PATH
                        && location.start_line == Some(3)
                        && location.start_column == Some(5)
                        && location.end_line == Some(3)
                        && location.end_column == Some(18)
                })
        })
        .collect::<Vec<_>>();
    let [function] = expected_functions.as_slice() else {
        panic!("exactly one accepted full-span Rust function Artifact at the independent literal");
    };
    assert!(function.label.ends_with("::callee"));
    assert_eq!(program.artifact(&function.id), Some(*function));
    let caller = program
        .artifacts()
        .iter()
        .find(|artifact| {
            artifact.kind == "function"
                && artifact.label == "crate::structural::structural::caller"
                && artifact.content_hash.as_ref() == Some(entry.content_hash())
                && artifact.location.as_ref().is_some_and(|loc| {
                    loc.path == PATH
                        && loc.start_line == Some(4)
                        && loc.start_column == Some(5)
                        && loc.end_line == Some(9)
                        && loc.end_column == Some(5)
                })
        })
        .expect("independent accepted attr-inclusive caller identity");
    fs::write(fixture.root.join(PATH), "fn dirty_after_admission() {}\n").unwrap();
    let rows = result
        .legacy
        .g3_observations()
        .expect("internal Git-bound G3 batch")
        .declarations();
    assert_eq!(rows.len(), 2, "complete free-function declaration set");
    for (observed, expected_id) in [(callee_range, &function.id), (caller_range, &caller.id)] {
        let matching = rows
            .iter()
            .filter(|row| row.occurrence() == observed)
            .collect::<Vec<_>>();
        let [row] = matching.as_slice() else {
            panic!("one independent source span per free declaration")
        };
        assert_eq!(row.source().snapshot_id(), bundle.snapshot_id());
        assert_eq!(row.source().file_id(), entry.artifact_id());
        assert_eq!(row.source().canonical_path(), PATH);
        assert_eq!(row.source().source_hash(), entry.content_hash());
        assert!(
            matches!(row.outcome(), RustG3OutcomeV1::ExistingFact(RustFactRefV1::Artifact { id })
            if id == expected_id),
            "existing function ID, never a test-minted source fact"
        );
    }
}

#[derive(Default)]
struct NegativeSyntax {
    free_functions: usize,
    methods: usize,
    closures: usize,
}

impl<'ast> Visit<'ast> for NegativeSyntax {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.free_functions += 1;
        visit::visit_item_fn(self, item);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.methods += 1;
        visit::visit_impl_item_fn(self, item);
    }

    fn visit_expr_closure(&mut self, expr: &'ast syn::ExprClosure) {
        self.closures += 1;
        visit::visit_expr_closure(self, expr);
    }
}

#[test]
fn method_with_closure_is_exercised_but_does_not_become_a_free_declaration() {
    let syntax = syn::parse_file(NEGATIVE).expect("negative source parses");
    let mut observed = NegativeSyntax::default();
    observed.visit_file(&syntax);
    assert_eq!(observed.free_functions, 0);
    assert_eq!(observed.methods, 1);
    assert_eq!(observed.closures, 1);
    let fixture = fixture(NEGATIVE);
    let result = ingest_with_sources_v2(&fixture.request(), u64::MAX)
        .expect("negative Rust source-retaining ingest");
    let program = &result.legacy.program_space;
    let [entry] = result.legacy.source_bundle.entries() else {
        panic!("one admitted negative Rust source");
    };
    assert_eq!(entry.bytes(), NEGATIVE.as_bytes());
    assert_eq!(
        entry.content_hash(),
        &ContentHash::sha256(NEGATIVE.as_bytes())
    );
    assert!(
        program.artifacts().iter().any(|artifact| {
            artifact.kind == "method"
                && artifact.language.as_deref() == Some("rust")
                && artifact
                    .location
                    .as_ref()
                    .is_some_and(|location| location.path == PATH)
        }),
        "the negative method was actually ingested, not removed from the fixture"
    );
    let batch = result
        .legacy
        .g3_observations()
        .expect("negative Git-bound batch");
    assert!(
        batch.declarations().is_empty(),
        "method and closure cannot be free declarations"
    );
    assert!(
        batch
            .construct_partition()
            .exclusions()
            .iter()
            .any(|row| format!("{row:?}").contains("method")),
        "method has an explicit exclusion witness"
    );
    assert!(
        batch
            .construct_partition()
            .exclusions()
            .iter()
            .any(|row| format!("{row:?}").contains("closure")),
        "closure has an explicit exclusion witness"
    );
}

#[test]
fn git_exclusions_are_typed_legacy_refs_in_the_exact_rust_partition_id() {
    use crate::IngestionObstructionKind;
    use reviewgraphen_core::StableId;
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    use std::os::unix::fs::symlink;

    let workspace = tempfile::tempdir().expect("Git exclusion fixture workspace");
    let root = workspace.path().join("fixture");
    fs::create_dir(&root).expect("fixture repository directory");
    git(&root, &["init", "--quiet", "--object-format=sha1"]);
    git(
        &root,
        &["config", "user.email", "reviewgraphen@example.test"],
    );
    git(&root, &["config", "user.name", "ReviewGraphen test"]);
    fs::write(root.join("README.txt"), "regular retained source\n").expect("regular fixture file");
    git(&root, &["add", "README.txt"]);
    git(&root, &["commit", "--quiet", "-m", "base"]);
    let base = git(&root, &["rev-parse", "HEAD"]);
    symlink("README.txt", root.join("linked.rs")).expect("tracked Rust-named symlink");
    git(&root, &["add", "linked.rs"]);
    git(
        &root,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{base},deps/lib"),
        ],
    );
    git(&root, &["commit", "--quiet", "-m", "excluded entries"]);
    let target = git(&root, &["rev-parse", "HEAD"]);
    let tree = git(&root, &["ls-tree", "-r", &target]);
    let entries = tree
        .lines()
        .map(|line| {
            let (metadata, path) = line.split_once('\t').expect("Git tree entry has a path");
            let parts = metadata.split_whitespace().collect::<Vec<_>>();
            assert_eq!(parts.len(), 3, "mode, type, and object OID");
            (path.to_owned(), parts[0].to_owned(), parts[1].to_owned())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        entries,
        [
            ("README.txt".into(), "100644".into(), "blob".into()),
            ("deps/lib".into(), "160000".into(), "commit".into()),
            ("linked.rs".into(), "120000".into(), "blob".into()),
        ],
        "the immutable Git tree has exactly one regular file and two excluded entries"
    );

    let request = IngestRequest::new(
        workspace.path(),
        &root,
        "reviewgraphen.test/g3-git-exclusion-partition",
        &base,
        &target,
    );
    let result = ingest_with_sources_v2(&request, u64::MAX).expect("original-base admission");
    let self_request = IngestRequest::new(
        workspace.path(),
        &root,
        "reviewgraphen.test/g3-git-exclusion-partition",
        &target,
        &target,
    );
    let self_result = ingest_with_sources_v2(&self_request, u64::MAX)
        .expect("same target admitted with itself as diff base");
    let legacy = &result.legacy;
    let self_legacy = &self_result.legacy;
    assert_eq!(
        legacy.program_space.snapshot_id(),
        self_legacy.program_space.snapshot_id(),
        "the same target tree has one snapshot identity regardless of diff base"
    );

    // Both runs must first prove the real Git exclusion report and retained
    // regular-file boundary. Only then can a missing G3 reference be the red.
    let inspect_legacy = |run: &crate::IngestWithSourcesResult| {
        let [entry] = run.source_bundle.entries() else {
            panic!("only the ordinary regular file is accepted into the source bundle")
        };
        assert_eq!(entry.path(), "README.txt");
        assert_eq!(entry.bytes(), b"regular retained source\n");
        let git_adapter = run
            .extraction_report
            .adapters
            .iter()
            .find(|adapter| adapter.id == "reviewgraphen.ingest.git")
            .expect("Git adapter report");
        assert_eq!(git_adapter.total, Some(3));
        assert_eq!(git_adapter.excluded, Some(2));
        assert_eq!(git_adapter.failed, Some(0));
        let selected = run
            .extraction_report
            .obstructions
            .iter()
            .filter(|row| row.paths.contains("linked.rs") || row.paths.contains("deps/lib"))
            .collect::<Vec<_>>();
        assert_eq!(
            selected.len(),
            2,
            "one legacy obstruction per excluded entry"
        );
        let expected_legacy = [
            ("deps/lib", IngestionObstructionKind::UnsupportedInput),
            ("linked.rs", IngestionObstructionKind::RegionExcluded),
        ]
        .into_iter()
        .map(|(path, kind)| {
            let matching = selected
                .iter()
                .filter(|row| row.paths.len() == 1 && row.paths.contains(path))
                .collect::<Vec<_>>();
            let [row] = matching.as_slice() else {
                panic!("one exact-path accepted legacy Git obstruction for {path}")
            };
            assert_eq!(row.kind, kind, "Git exclusion kind for {path}");
            (path, kind, row.id.clone())
        })
        .collect::<Vec<_>>();
        assert_ne!(expected_legacy[0].2, expected_legacy[1].2);
        assert!(
            run.program_space.artifacts().iter().all(|artifact| {
                !artifact.location.as_ref().is_some_and(|location| {
                    location.path == "deps/lib" || location.path == "linked.rs"
                })
            }),
            "excluded entries never acquire accepted file artifacts"
        );
        expected_legacy
    };
    let expected_legacy = inspect_legacy(legacy);
    let expected_self_legacy = inspect_legacy(self_legacy);
    for (original, self_diff) in expected_legacy.iter().zip(&expected_self_legacy) {
        assert_eq!((original.0, original.1), (self_diff.0, self_diff.1));
        assert_ne!(
            original.2, self_diff.2,
            "legacy Git obstruction ID for {} is base-relative",
            original.0
        );
    }
    let [entry] = legacy.source_bundle.entries() else {
        unreachable!("singleton bundle checked above")
    };
    let [self_entry] = self_legacy.source_bundle.entries() else {
        unreachable!("singleton bundle checked above")
    };
    assert_eq!(entry.artifact_id(), self_entry.artifact_id());
    assert_eq!(entry.content_hash(), self_entry.content_hash());

    let batch = legacy
        .g3_observations()
        .expect("original-base G3 admission");
    let self_batch = self_legacy
        .g3_observations()
        .expect("self-base G3 admission");
    for (run_batch, run_entry, report_refs) in [
        (batch, entry, &expected_legacy),
        (self_batch, self_entry, &expected_self_legacy),
    ] {
        let [retained] = run_batch.file_partition().files() else {
            panic!("G3's retained regular-file partition contains exactly one file")
        };
        assert_eq!(retained.source().canonical_path(), "README.txt");
        assert_eq!(retained.source().file_id(), run_entry.artifact_id());
        assert_eq!(retained.source().source_hash(), run_entry.content_hash());
        let refs = run_batch.git_exclusion_refs();
        assert_eq!(
            refs.len(),
            2,
            "one G3 reference for each excluded Git tree entry in each run"
        );
        let actual_refs = refs
            .iter()
            .map(|row| {
                (
                    row.path().to_owned(),
                    serde_json::to_value(row.entry_kind())
                        .expect("closed private G3 Git entry kind"),
                    row.kind(),
                    row.legacy_obstruction_id().clone(),
                    serde_json::to_value(row.latent_occurrence_count())
                        .expect("closed private G3 latent count"),
                )
            })
            .collect::<Vec<_>>();
        let expected_refs = report_refs
            .iter()
            .zip(["git_gitlink@2", "git_symlink@2"])
            .map(|((path, kind, id), entry_kind)| {
                (
                    (*path).to_owned(),
                    json!(entry_kind),
                    *kind,
                    id.clone(),
                    json!("unknown"),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            actual_refs, expected_refs,
            "sorted per-run typed legacy refs"
        );
    }

    // Only target Git tree paths and entry kinds, not the diff-base-relative
    // legacy obstruction IDs, may enter partition identity. No excluded file
    // ID or full source hash is fabricated. This literal is checked against
    // the three exact ls-tree entries above, independently of G3 outputs.
    let latent = json!([
        {"path":"deps/lib","entry_kind":"git_gitlink@2","count":"unknown"},
        {"path":"linked.rs","entry_kind":"git_symlink@2","count":"unknown"}
    ]);
    let contracts = [
        "R-declaration@2",
        "R-containment@2",
        "R-write@2",
        "R-test-marker@2",
    ]
    .iter()
    .map(|contract| {
        json!({"contract":contract,"occurrence_ids":[],"success_ids":[],
            "obstruction_ids":[],"exclusion_keys":[]})
    })
    .collect::<Vec<_>>();
    let preimage = BTreeMap::<String, Value>::from([
        ("version".into(), json!(2)),
        ("profile".into(), json!("rust.g3-admission@2")),
        ("extractor".into(), json!("syn.g3@2")),
        (
            "snapshot".into(),
            json!({"kind":"rust_program","value":legacy.program_space.snapshot_id().as_str()}),
        ),
        ("registry".into(), Value::Null),
        (
            "files".into(),
            json!([{"file_id":entry.artifact_id().as_str(),
                "canonical_source_path":"README.txt",
                "source_hash":entry.content_hash().as_str(),
                "outcome":"non_rust_excluded"}]),
        ),
        ("contracts".into(), json!(contracts)),
        ("latent".into(), json!(latent)),
    ]);
    let expected_id = StableId::derived("reviewgraphen.g3.rust.partition.v2", &preimage)
        .expect("independent exact v2 preimage derives a stable ID");
    assert_eq!(batch.file_partition().id(), &expected_id);
    assert_eq!(batch.construct_partition().id(), &expected_id);
    assert_eq!(self_batch.file_partition().id(), &expected_id);
    assert_eq!(self_batch.construct_partition().id(), &expected_id);
}
