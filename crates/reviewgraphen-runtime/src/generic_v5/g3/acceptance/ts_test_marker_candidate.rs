//! TS-test-marker-candidate@2: A0/A1-bound generic calls remain semantically unknown.
use crate::generic_v5::admission::g3_observe::{TestFrameworkUnknownCodeV1, TypeScriptG3OutcomeV1};
use crate::generic_v5::admission::{
    ReconstructionContextV1, SourceAdmissionBoundsV1, admit_typescript_revision_pair,
};
use reviewgraphen_core::source_review::basis::SourceSyntaxRole;
use reviewgraphen_core::source_review::ids::{
    CanonicalFileKey, SourceFileId, SourceHash, SourceRange, SyntaxKeyV1,
};
use reviewgraphen_ingest::typescript::native_syntax::{NativeSyntaxRecord, collect_native_syntax};
use reviewgraphen_ingest::typescript::payload::SyntaxRole;
use std::{fs, path::Path, process::Command};

const SOURCE: &[u8] = b"export function callee() {}\nexport function caller() {\n  let value = 0;\n  value = 1;\n  callee();\n}\nit(\"marker\", () => {});\n";
const FILE: &str = "src/structural.ts";
fn range(start: u64, end: u64) -> SourceRange {
    SourceRange::new(start, end).expect("half-open byte interval")
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
    let output = command.output().expect("run isolated Git fixture command");
    assert!(
        output.status.success(),
        "fixture git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("Git stdout UTF-8")
        .trim()
        .to_owned()
}

struct Fixture {
    _workspace: tempfile::TempDir,
    context: ReconstructionContextV1,
    file_id: SourceFileId,
    marker_call: SyntaxKeyV1,
    ordinary_call: SyntaxKeyV1,
    wrong_role: SyntaxKeyV1,
}
impl Fixture {
    fn admitted(target_message: &str) -> Self {
        assert_eq!(SOURCE.len(), 123);
        assert_eq!(
            SourceHash::from_source_bytes(SOURCE).wire_literal(),
            "sha256:eca129e7be017d0a741b7a35a3183bccb61ae6753916ec83209a4c651c1cf9d7"
        );
        assert_eq!(&SOURCE[99..121], b"it(\"marker\", () => {})");
        assert_eq!(&SOURCE[87..95], b"callee()");
        let workspace = tempfile::tempdir().expect("temporary A0 fixture");
        let repository = workspace.path().join("repository");
        fs::create_dir(&repository).expect("repository");
        git(&repository, &["init", "--quiet"]);
        git(&repository, &["config", "user.email", "g3@example.invalid"]);
        git(
            &repository,
            &["config", "user.name", "G3 acceptance fixture"],
        );
        fs::create_dir(repository.join("src")).expect("source directory");
        fs::write(repository.join(FILE), b"export const base = 0;\n").expect("base bytes");
        git(&repository, &["add", "."]);
        git(&repository, &["commit", "--quiet", "-m", "fixture base"]);
        let base = git(&repository, &["rev-parse", "HEAD"]);
        fs::write(repository.join(FILE), SOURCE).expect("target bytes");
        git(&repository, &["add", "."]);
        git(&repository, &["commit", "--quiet", "-m", target_message]);
        let target = git(&repository, &["rev-parse", "HEAD"]);
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
        .expect("real literal Git A0 admission and comparison");
        let file_id = file(FILE);
        let native = collect_native_syntax(file_id.clone(), SOURCE);
        let key_for = |role: SourceSyntaxRole, observed: SourceRange| {
            let kind = native
                .iter()
                .find_map(|record| match (role, record) {
                    (SourceSyntaxRole::Call, NativeSyntaxRecord::Call { kind, range, .. })
                        if *range == observed =>
                    {
                        Some(kind)
                    }
                    (
                        SourceSyntaxRole::Callable,
                        NativeSyntaxRecord::Callable { kind, range, .. },
                    ) if *range == observed => Some(kind),
                    _ => None,
                })
                .expect("source-native role and half-open range");
            SyntaxKeyV1::derive_from_source(
                context.registry_binding(),
                context.snapshot_binding(),
                &file_id,
                role,
                kind,
                observed,
            )
        };
        let marker_call = key_for(SourceSyntaxRole::Call, range(99, 121));
        let ordinary_call = key_for(SourceSyntaxRole::Call, range(87, 95));
        let wrong_role = key_for(SourceSyntaxRole::Callable, range(35, 98));
        let rebuilt = context
            .rebuild_payload_catalogue_from_a0()
            .expect("A0 rebuilds A1");
        assert_eq!(rebuilt.parsed_file_count(), 1);
        let sealed = context.seal_rebuilt_payload_catalogue_from_a0(rebuilt);
        for (observed, raw_key) in [
            (range(99, 121), &marker_call),
            (range(87, 95), &ordinary_call),
        ] {
            assert!(
                sealed.iter().any(|(id, row)| id == &file_id
                    && row.as_ref().role == SyntaxRole::Call
                    && row.as_ref().range == observed
                    && &SyntaxKeyV1::derive_from_source(
                        context.registry_binding(),
                        context.snapshot_binding(),
                        id,
                        SourceSyntaxRole::Call,
                        &row.as_ref().kind,
                        row.as_ref().range
                    ) == raw_key),
                "generic Call key must belong to a sealed A1 row"
            );
        }
        Self {
            _workspace: workspace,
            context,
            file_id,
            marker_call,
            ordinary_call,
            wrong_role,
        }
    }
}

#[test]
fn both_it_and_ordinary_accepted_generic_calls_remain_unknown_not_test_markers() {
    let f = Fixture::admitted("fixture target");
    let observed = collect_native_syntax(f.file_id.clone(), SOURCE)
        .iter()
        .filter_map(|row| match row {
            NativeSyntaxRecord::Call { range, .. } => Some((range.start(), range.end())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        observed,
        [(87, 95), (99, 121)],
        "independent existing native Call parser"
    );
    assert_eq!(&SOURCE[87..95], b"callee()");
    assert_eq!(&SOURCE[99..121], b"it(\"marker\", () => {})");
    let candidates = f
        .context
        .g3_observations()
        .expect("A0 stores Git-bound calls")
        .test_marker_candidates();
    assert_eq!(
        candidates.len(),
        2,
        "complete generic Call universe, not just named it"
    );
    for (span, key) in [
        (range(99, 121), &f.marker_call),
        (range(87, 95), &f.ordinary_call),
    ] {
        let matching = candidates
            .iter()
            .filter(|candidate| candidate.occurrence_range() == span)
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1);
        let candidate = matching[0];
        assert_eq!(candidate.binding().file_id(), &f.file_id);
        assert_eq!(candidate.binding().canonical_path(), FILE);
        assert_eq!(
            candidate.binding().snapshot_binding(),
            f.context.snapshot_binding()
        );
        assert_eq!(
            candidate.binding().source_hash(),
            &SourceHash::from_source_bytes(SOURCE)
        );
        assert_eq!(candidate.call_key(), Some(key));
        assert!(
            matches!(
                candidate.outcome(),
                TypeScriptG3OutcomeV1::Unknown(
                    TestFrameworkUnknownCodeV1::TestFrameworkSemanticsUnaccepted
                )
            ),
            "neither generic call becomes accepted TestMarker semantics"
        );
    }
}

#[test]
fn wrong_role_dangling_file_and_snapshot_keys_cannot_become_candidates() {
    let f = Fixture::admitted("fixture target");
    let candidates = f
        .context
        .g3_observations()
        .unwrap()
        .test_marker_candidates();
    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate.call_key() == Some(&f.wrong_role)),
        "A1 Callable is the wrong role for any call candidate"
    );
    let dangling = SyntaxKeyV1::parse_wire(
        f.context.registry_binding(),
        SourceHash::from_source_bytes(b"not a recorded key").wire_literal(),
    )
    .expect("raw syntactically valid key is not an A1 row");
    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate.call_key() == Some(&dangling))
    );
    let observed = collect_native_syntax(f.file_id.clone(), SOURCE);
    let call_kind = observed
        .iter()
        .find_map(|row| match row {
            NativeSyntaxRecord::Call {
                kind,
                range: observed_range,
                ..
            } if *observed_range == range(99, 121) => Some(kind),
            _ => None,
        })
        .expect("observed call kind");
    let wrong_file = SyntaxKeyV1::derive_from_source(
        f.context.registry_binding(),
        f.context.snapshot_binding(),
        &file("src/other.ts"),
        SourceSyntaxRole::Call,
        call_kind,
        range(99, 121),
    );
    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate.call_key() == Some(&wrong_file))
    );
    let other = Fixture::admitted("different target commit message");
    assert_ne!(
        f.context.snapshot_binding(),
        other.context.snapshot_binding(),
        "distinct literal Git commit binding"
    );
    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate.call_key() == Some(&other.marker_call)),
        "different committed snapshot cannot occur in retained old context"
    );
    assert_eq!(
        other
            .context
            .g3_observations()
            .unwrap()
            .test_marker_candidates()
            .len(),
        2
    );
}
