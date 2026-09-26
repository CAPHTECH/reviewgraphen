//! TS-containment@2: admission's own A0 bytes bind native direct-member syntax.

use crate::generic_v5::admission::g3_observe::TypeScriptG3OutcomeV1;
use crate::generic_v5::admission::{
    ReconstructionContextV1, SourceAdmissionBoundsV1, admit_typescript_revision_pair,
};
use reviewgraphen_core::source_review::basis::SourceSyntaxRole;
use reviewgraphen_core::source_review::ids::{
    CanonicalFileKey, SourceFileId, SourceHash, SourceRange, SyntaxKeyV1,
};
use reviewgraphen_core::source_review::reasons::RecordOutcomeV1;
use reviewgraphen_ingest::typescript::native_syntax::{NativeSyntaxRecord, collect_native_syntax};
use reviewgraphen_ingest::typescript::payload::{
    SyntaxRole, TypeScriptOutcomeV1, TypeScriptPayloadData,
};
use std::{collections::BTreeSet, fs, path::Path, process::Command};

// Frozen literal: ASCII, 123 bytes.
const SOURCE: &[u8] = b"export function callee() {}\nexport function caller() {\n  let value = 0;\n  value = 1;\n  callee();\n}\nit(\"marker\", () => {});\n";
const FILE: &str = "src/structural.ts";

fn range(start: u64, end: u64) -> SourceRange {
    SourceRange::new(start, end).expect("independent half-open byte range")
}

fn file(path: &str) -> SourceFileId {
    SourceFileId::from_basis_file_key(CanonicalFileKey::from_basis_path(path).expect("basis path"))
}

fn git(root: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    command
        .env_clear()
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("LC_ALL", "C")
        .args(args);
    if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }
    let output = command.output().expect("run isolated fixture git");
    assert!(
        output.status.success(),
        "fixture git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git stdout UTF-8")
        .trim()
        .to_owned()
}

struct Fixture {
    _workspace: tempfile::TempDir,
    context: ReconstructionContextV1,
    file_id: SourceFileId,
    scope_key: SyntaxKeyV1,
    callable_key: SyntaxKeyV1,
}

impl Fixture {
    fn admitted() -> Self {
        assert_eq!(SOURCE.len(), 123);
        assert_eq!(
            SourceHash::from_source_bytes(SOURCE).wire_literal(),
            "sha256:eca129e7be017d0a741b7a35a3183bccb61ae6753916ec83209a4c651c1cf9d7"
        );
        assert_eq!(
            &SOURCE[35..98],
            b"function caller() {\n  let value = 0;\n  value = 1;\n  callee();\n}"
        );
        let workspace = tempfile::tempdir().expect("fixture workspace");
        let repository = workspace.path().join("repository");
        fs::create_dir(&repository).expect("fixture repository");
        git(&repository, &["init", "--quiet"]);
        git(&repository, &["config", "user.email", "g3@example.invalid"]);
        git(
            &repository,
            &["config", "user.name", "G3 acceptance fixture"],
        );
        fs::create_dir(repository.join("src")).expect("fixture src");
        fs::write(repository.join(FILE), b"export const base = 0;\n").expect("base bytes");
        git(&repository, &["add", "."]);
        git(&repository, &["commit", "--quiet", "-m", "fixture base"]);
        let base = git(&repository, &["rev-parse", "HEAD"]);
        fs::write(repository.join(FILE), SOURCE).expect("target bytes");
        git(&repository, &["add", "."]);
        git(&repository, &["commit", "--quiet", "-m", "fixture target"]);
        let target = git(&repository, &["rev-parse", "HEAD"]);
        fs::write(repository.join(FILE), b"export function dirty() {}\n").unwrap();
        let (_, context) = admit_typescript_revision_pair(
            workspace.path().to_owned(),
            repository,
            base,
            target,
            SourceAdmissionBoundsV1 {
                max_files: 8,
                max_file_bytes: 1024,
                max_total_source_bytes: 4096,
            },
        )
        .expect("literal Git objects admitted and compared by real A0");
        let file_id = file(FILE);
        // Independently observed raw ranges and role/kind, not an A1 key copied
        // from a payload. Derivation is only a candidate value, never a seal.
        let native = collect_native_syntax(file_id.clone(), SOURCE);
        let scope_kind = native
            .iter()
            .find_map(|record| match record {
                NativeSyntaxRecord::Scope {
                    kind,
                    range: observed,
                    ..
                } if *observed == range(0, 123) => Some(kind),
                _ => None,
            })
            .expect("source-native file scope");
        let callable_kind = native
            .iter()
            .find_map(|record| match record {
                NativeSyntaxRecord::Callable {
                    kind,
                    range: observed,
                    ..
                } if *observed == range(35, 98) => Some(kind),
                _ => None,
            })
            .expect("source-native directly held caller");
        let scope_key = SyntaxKeyV1::derive_from_source(
            context.registry_binding(),
            context.snapshot_binding(),
            &file_id,
            SourceSyntaxRole::Scope,
            scope_kind,
            range(0, 123),
        );
        let callable_key = SyntaxKeyV1::derive_from_source(
            context.registry_binding(),
            context.snapshot_binding(),
            &file_id,
            SourceSyntaxRole::Callable,
            callable_kind,
            range(35, 98),
        );
        let rebuilt = context
            .rebuild_payload_catalogue_from_a0()
            .expect("A0 rebuilds A1");
        assert_eq!(rebuilt.parsed_file_count(), 1);
        let sealed = context.seal_rebuilt_payload_catalogue_from_a0(rebuilt);
        let scope = sealed
            .iter()
            .find(|(id, row)| {
                id == &file_id
                    && row.as_ref().role == SyntaxRole::Scope
                    && row.as_ref().range == range(0, 123)
            })
            .expect("sealed A1 Scope");
        assert_eq!(
            scope.1.as_ref().outcome,
            TypeScriptOutcomeV1::Scope(RecordOutcomeV1::Recorded)
        );
        let TypeScriptPayloadData::Scope(data) = &scope.1.as_ref().data else {
            panic!("A1 Scope data required")
        };
        assert!(
            data.member_keys.contains(&callable_key),
            "A1 Scope must own this exact Callable key"
        );
        assert!(sealed.iter().any(|(id, row)| id == &file_id
            && row.as_ref().role == SyntaxRole::Callable
            && row.as_ref().range == range(35, 98)
            && SyntaxKeyV1::derive_from_source(
                context.registry_binding(),
                context.snapshot_binding(),
                id,
                SourceSyntaxRole::Callable,
                &row.as_ref().kind,
                row.as_ref().range
            ) == callable_key));
        Self {
            _workspace: workspace,
            context,
            file_id,
            scope_key,
            callable_key,
        }
    }
}

#[test]
fn admitted_scope_and_direct_member_seal_only_after_exact_raw_join() {
    let f = Fixture::admitted();
    assert_eq!(&SOURCE[7..27], b"function callee() {}");
    assert_eq!(
        &SOURCE[35..98],
        b"function caller() {\n  let value = 0;\n  value = 1;\n  callee();\n}"
    );
    let kinds = collect_native_syntax(f.file_id.clone(), SOURCE);
    let direct = kinds
        .iter()
        .filter_map(|row| match row {
            NativeSyntaxRecord::Callable { range, .. } => Some((range.start(), range.end())),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        direct,
        BTreeSet::from([(7, 27), (35, 98)]),
        "native independent A1 source parser"
    );
    let rows = f
        .context
        .g3_observations()
        .expect("A0 stores Git-bound G3 result")
        .containment();
    assert_eq!(
        rows.len(),
        2,
        "both direct top-level callables, no nested arrow"
    );
    let callee_range = range(7, 27);
    let callee_kind = kinds
        .iter()
        .find_map(|r| match r {
            NativeSyntaxRecord::Callable { kind, range, .. } if *range == callee_range => {
                Some(kind)
            }
            _ => None,
        })
        .unwrap();
    let callee_key = SyntaxKeyV1::derive_from_source(
        f.context.registry_binding(),
        f.context.snapshot_binding(),
        &f.file_id,
        SourceSyntaxRole::Callable,
        callee_kind,
        callee_range,
    );
    for (child, expected_key) in [
        (range(35, 98), &f.callable_key),
        (callee_range, &callee_key),
    ] {
        let matches = rows
            .iter()
            .filter(|row| row.parent_range() == range(0, 123) && row.child_range() == child)
            .collect::<Vec<_>>();
        assert_eq!(matches.len(), 1);
        let link = matches[0];
        assert_eq!(link.binding().file_id(), &f.file_id);
        assert_eq!(link.binding().canonical_path(), FILE);
        assert_eq!(
            link.binding().snapshot_binding(),
            f.context.snapshot_binding()
        );
        assert_eq!(
            link.binding().source_hash(),
            &SourceHash::from_source_bytes(SOURCE)
        );
        assert!(
            matches!(link.outcome(), TypeScriptG3OutcomeV1::ExistingSyntaxMembership {
            scope_key, callable_key } if scope_key == &f.scope_key && callable_key == expected_key)
        );
        assert_eq!(link.scope_key(), Some(&f.scope_key));
        assert_eq!(link.callable_key(), Some(expected_key));
    }
}

#[test]
fn swapped_out_of_parent_wrong_role_and_dangling_keys_never_seal() {
    let f = Fixture::admitted();
    let rows = f.context.g3_observations().unwrap().containment();
    assert!(rows.iter().all(|row| row.parent_range() == range(0, 123)
        && row.binding().file_id() == &f.file_id
        && row.binding().source_hash() == &SourceHash::from_source_bytes(SOURCE)));
    assert!(
        !rows
            .iter()
            .any(|row| row.child_range() == range(112, 120) || row.parent_range() == range(35, 98)),
        "nested arrow and swapped ownership are nonmembers"
    );
    let raw_foreign_key = SyntaxKeyV1::derive_from_source(
        f.context.registry_binding(),
        f.context.snapshot_binding(),
        &file("src/foreign.ts"),
        SourceSyntaxRole::Callable,
        &reviewgraphen_core::source_review::reasons::TypeScriptSyntaxKind::parse_wire(
            "function_declaration",
        )
        .unwrap(),
        range(35, 98),
    );
    assert!(
        !rows
            .iter()
            .any(|row| row.callable_key() == Some(&raw_foreign_key)),
        "caller-made wrong-file syntax key never becomes an admitted member"
    );
    // A0 preserves malformed committed source in its target inventory, not as a parsed member.
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().join("repository");
    fs::create_dir(&root).unwrap();
    git(&root, &["init", "--quiet"]);
    git(&root, &["config", "user.email", "g3@example.invalid"]);
    git(&root, &["config", "user.name", "G3 acceptance fixture"]);
    fs::write(root.join("structural.ts"), SOURCE).unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "--quiet", "-m", "base"]);
    let base = git(&root, &["rev-parse", "HEAD"]);
    fs::write(root.join("broken.ts"), b"export function = ;\n").unwrap();
    fs::write(root.join("copy.ts"), SOURCE).unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "--quiet", "-m", "target"]);
    let target = git(&root, &["rev-parse", "HEAD"]);
    let (_, context) = admit_typescript_revision_pair(
        scratch.path().to_owned(),
        root,
        base,
        target,
        SourceAdmissionBoundsV1 {
            max_files: 8,
            max_file_bytes: 1024,
            max_total_source_bytes: 4096,
        },
    )
    .unwrap();
    let batch = context
        .g3_observations()
        .expect("parse-failed file remains accounted");
    assert!(
        batch
            .file_partition()
            .iter()
            .any(|row| format!("{row:?}").contains("ParseFailed")
                && format!("{row:?}").contains("broken.ts"))
    );
    assert!(
        !batch
            .containment()
            .iter()
            .any(|row| row.binding().canonical_path() == "broken.ts")
    );
    let rebuilt = context
        .rebuild_payload_catalogue_from_a0()
        .expect("actual A0 rebuilt A1 catalogue");
    assert_eq!(
        rebuilt.parsed_file_count(),
        2,
        "broken file is not a parsed A1 member"
    );
    let sealed = context.seal_rebuilt_payload_catalogue_from_a0(rebuilt);
    let mut observed_ids = Vec::new();
    for path in ["structural.ts", "copy.ts"] {
        let actual_file_id = file(path); // independently selected canonical basis path
        let syntax = collect_native_syntax(actual_file_id.clone(), SOURCE);
        let scope_kind = syntax
            .iter()
            .find_map(|record| match record {
                NativeSyntaxRecord::Scope {
                    kind,
                    range: observed,
                    ..
                } if *observed == range(0, 123) => Some(kind),
                _ => None,
            })
            .expect("native file-lexical Scope kind");
        let scope_key = SyntaxKeyV1::derive_from_source(
            context.registry_binding(),
            context.snapshot_binding(),
            &actual_file_id,
            SourceSyntaxRole::Scope,
            scope_kind,
            range(0, 123),
        );
        let actual_scopes = sealed
            .iter()
            .filter(|(id, row)| {
                id == &actual_file_id
                    && row.as_ref().role == SyntaxRole::Scope
                    && row.as_ref().range == range(0, 123)
            })
            .collect::<Vec<_>>();
        let [(_, scope)] = actual_scopes.as_slice() else {
            panic!("one rebuilt A1 Scope per expected file")
        };
        assert_eq!(
            scope.as_ref().outcome,
            TypeScriptOutcomeV1::Scope(RecordOutcomeV1::Recorded)
        );
        assert_eq!(
            SyntaxKeyV1::derive_from_source(
                context.registry_binding(),
                context.snapshot_binding(),
                &actual_file_id,
                SourceSyntaxRole::Scope,
                &scope.as_ref().kind,
                scope.as_ref().range
            ),
            scope_key,
            "independently derived scope key matches the real A1 row"
        );
        let TypeScriptPayloadData::Scope(scope_data) = &scope.as_ref().data else {
            panic!("real rebuilt A1 Scope payload")
        };
        let per_file = batch
            .containment()
            .iter()
            .filter(|row| row.binding().canonical_path() == path)
            .collect::<Vec<_>>();
        assert_eq!(
            per_file.len(),
            2,
            "each equal-byte file has both direct pairs"
        );
        for child in [range(7, 27), range(35, 98)] {
            let callable_kind = syntax
                .iter()
                .find_map(|record| match record {
                    NativeSyntaxRecord::Callable {
                        kind,
                        range: observed,
                        ..
                    } if *observed == child => Some(kind),
                    _ => None,
                })
                .expect("independently observed top-level Callable kind");
            let callable_key = SyntaxKeyV1::derive_from_source(
                context.registry_binding(),
                context.snapshot_binding(),
                &actual_file_id,
                SourceSyntaxRole::Callable,
                callable_kind,
                child,
            );
            let actual_callables = sealed
                .iter()
                .filter(|(id, row)| {
                    id == &actual_file_id
                        && row.as_ref().role == SyntaxRole::Callable
                        && row.as_ref().range == child
                })
                .collect::<Vec<_>>();
            let [(_, callable)] = actual_callables.as_slice() else {
                panic!("unique rebuilt A1 Callable on this actual file")
            };
            assert_eq!(
                SyntaxKeyV1::derive_from_source(
                    context.registry_binding(),
                    context.snapshot_binding(),
                    &actual_file_id,
                    SourceSyntaxRole::Callable,
                    &callable.as_ref().kind,
                    callable.as_ref().range
                ),
                callable_key
            );
            assert!(
                scope_data.member_keys.contains(&callable_key),
                "rebuilt A1 Scope must own this same-file Callable"
            );
            let selected = per_file
                .iter()
                .filter(|row| row.parent_range() == range(0, 123) && row.child_range() == child)
                .collect::<Vec<_>>();
            let [row] = selected.as_slice() else {
                panic!("unique G3 pair by expected path and child")
            };
            assert_eq!(row.binding().canonical_path(), path);
            assert_eq!(
                row.binding().file_id(),
                &actual_file_id,
                "an admitted equal-byte file cannot borrow the other file's ID"
            );
            assert_eq!(row.binding().snapshot_binding(), context.snapshot_binding());
            assert_eq!(
                row.binding().source_hash(),
                &SourceHash::from_source_bytes(SOURCE)
            );
            assert_eq!(row.scope_key(), Some(&scope_key));
            assert_eq!(row.callable_key(), Some(&callable_key));
            assert!(
                matches!(row.outcome(), TypeScriptG3OutcomeV1::ExistingSyntaxMembership {
                scope_key: actual_scope, callable_key: actual_callable }
                if actual_scope == &scope_key && actual_callable == &callable_key),
                "both equal-byte files must resolve their OWN rebuilt A1 membership, not Obstructed or cross-file keys"
            );
            observed_ids.push(row.id().clone());
        }
    }
    assert_eq!(observed_ids.len(), 4);
    assert_eq!(
        observed_ids.iter().collect::<BTreeSet<_>>().len(),
        4,
        "all four path/child occurrence identities distinct"
    );
}
