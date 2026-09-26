//! TS-write@2: raw public syntax cannot bind facts; A0 stores its own Git-byte observations.
use crate::generic_v5::admission::g3_observe::TypeScriptG3OutcomeV1;
use crate::generic_v5::admission::{
    ReconstructionContextV1, SourceAdmissionBoundsV1, admit_typescript_revision_pair,
};
use reviewgraphen_core::{
    StableId,
    source_review::ids::{
        CanonicalFileKey, SourceFileId, SourceHash, SourceRange, registry_tuple_hash,
    },
};
use reviewgraphen_ingest::typescript::g3_syntax::{TypeScriptLhsKindV1, parse_g3_syntax};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path, process::Command};

const SOURCE: &[u8] = b"export function callee() {}\nexport function caller() {\n  let value = 0;\n  value = 1;\n  callee();\n}\nit(\"marker\", () => {});\n";
const FIELD_SOURCE: &[u8] = b"let holder = { value: 0 }; holder.value = 1;\n";
const PATHS: [&str; 4] = [
    "src/structural.ts",
    "src/copy.ts",
    "src/field.ts",
    "src/field-copy.ts",
];
fn file(path: &str) -> SourceFileId {
    SourceFileId::from_basis_file_key(CanonicalFileKey::from_basis_path(path).unwrap())
}
fn range(start: u64, end: u64) -> SourceRange {
    SourceRange::new(start, end).unwrap()
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
    let output = command.output().expect("fixture Git command");
    assert!(
        output.status.success(),
        "fixture git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

struct Fixture {
    _workspace: tempfile::TempDir,
    context: ReconstructionContextV1,
}
impl Fixture {
    fn admitted(files: &[(&str, &[u8])]) -> Self {
        let workspace = tempfile::tempdir().unwrap();
        let repository = workspace.path().join("repository");
        fs::create_dir(&repository).unwrap();
        git(&repository, &["init", "--quiet"]);
        git(&repository, &["config", "user.email", "g3@example.invalid"]);
        git(
            &repository,
            &["config", "user.name", "G3 acceptance fixture"],
        );
        fs::create_dir(repository.join("src")).unwrap();
        fs::write(repository.join(PATHS[0]), b"export const base = 0;\n").unwrap();
        git(&repository, &["add", "."]);
        git(&repository, &["commit", "--quiet", "-m", "fixture base"]);
        let base = git(&repository, &["rev-parse", "HEAD"]);
        for (path, bytes) in files {
            fs::write(repository.join(path), bytes).unwrap();
        }
        git(&repository, &["add", "."]);
        git(&repository, &["commit", "--quiet", "-m", "fixture target"]);
        let target = git(&repository, &["rev-parse", "HEAD"]);
        // Git bytes, not a dirty on-disk substitute, must feed both A0 and stored G3.
        fs::write(
            repository.join(PATHS[0]),
            b"export function dirty() { let x = 0; x = 2; }\n",
        )
        .unwrap();
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
        .expect("A0 independently re-reads literal Git blobs");
        Self {
            _workspace: workspace,
            context,
        }
    }
}

fn assert_ast(bytes: &[u8], occurrence: SourceRange, lhs: SourceRange, lhs_kind: &str) {
    // Coordinates/kinds were separately parsed by pinned Tree-sitter in the external
    // OBSERVATION-TABLE before this test was written; runtime has no TS grammar dependency.
    let observed = &bytes[occurrence.start() as usize..occurrence.end() as usize];
    let observed_lhs = &bytes[lhs.start() as usize..lhs.end() as usize];
    match lhs_kind {
        "identifier" => {
            assert_eq!(observed, b"value = 1");
            assert_eq!(observed_lhs, b"value");
        }
        "member_expression" => {
            assert_eq!(observed, b"holder.value = 1");
            assert_eq!(observed_lhs, b"holder.value");
        }
        _ => panic!("unobserved fixture kind"),
    }
}

fn full_f4(context: &ReconstructionContextV1, path: &str) -> (StableId, StableId) {
    // Independent full v2 preimage. In particular canonical_source_path is NOT inferred
    // from the path-dependent file ID: deleting just this field must break this assertion.
    let base = BTreeMap::<String, Value>::from([
        ("version".into(), json!(2)),
        ("contract".into(), json!("TS-write@2")),
        ("profile".into(), json!("typescript.g3-admission@2")),
        ("extractor".into(), json!("tree-sitter-typescript.g3@2")),
        (
            "snapshot".into(),
            json!({"kind":"ts_a0","value":context.snapshot_binding().as_str()}),
        ),
        (
            "registry".into(),
            json!({"registry_hash":context.registry_binding().registry_hash,
            "tuple_hash":registry_tuple_hash(context.registry_binding())}),
        ),
        ("file_id".into(), json!(file(path).canonical_key())),
        ("canonical_source_path".into(), json!(path)),
        (
            "source_hash".into(),
            json!(SourceHash::from_source_bytes(FIELD_SOURCE).wire_literal()),
        ),
        ("coordinate".into(), json!("ts_utf8_byte_half_open")),
        (
            "syntax".into(),
            json!({"kind":"assignment_expression",
            "occurrence":{"start":27,"end":43}, "lhs":{"start":27,"end":39}, "lhs_kind":"field"}),
        ),
    ]);
    let id = StableId::derived("reviewgraphen.g3.typescript.observation.v2", &base).unwrap();
    let mut obstruction = base;
    obstruction.insert("observation_id".into(), json!(id.as_str()));
    obstruction.insert("reason".into(), json!("unsupported_assignment_lhs@2"));
    obstruction.insert("reason_detail".into(), json!({"lhs_kind":"field"}));
    (
        id,
        StableId::derived("reviewgraphen.g3.typescript.obstruction.v2", &obstruction).unwrap(),
    )
}

#[test]
fn admission_binds_each_identifier_and_field_row_to_its_own_literal_git_file() {
    assert_eq!(&SOURCE[74..83], b"value = 1");
    assert_ast(SOURCE, range(74, 83), range(74, 79), "identifier");
    assert_eq!(&FIELD_SOURCE[27..43], b"holder.value = 1");
    assert_ast(
        FIELD_SOURCE,
        range(27, 43),
        range(27, 39),
        "member_expression",
    );
    let fixture = Fixture::admitted(&[
        (PATHS[0], SOURCE),
        (PATHS[1], SOURCE),
        (PATHS[2], FIELD_SOURCE),
        (PATHS[3], FIELD_SOURCE),
    ]);
    let context = &fixture.context;
    let batch = context
        .g3_observations()
        .expect("admission stores bound rows");
    assert_eq!(batch.snapshot_binding(), context.snapshot_binding());
    assert_eq!(
        batch.writes().len(),
        4,
        "three-file identifier/field cases and equal-byte F4 control"
    );
    let mut obstructions = Vec::new();
    for (path, bytes, occurrence, lhs) in [
        (PATHS[0], SOURCE, range(74, 83), range(74, 79)),
        (PATHS[1], SOURCE, range(74, 83), range(74, 79)),
        (PATHS[2], FIELD_SOURCE, range(27, 43), range(27, 39)),
        (PATHS[3], FIELD_SOURCE, range(27, 43), range(27, 39)),
    ] {
        let selected = batch
            .writes()
            .iter()
            .filter(|row| row.binding().canonical_path() == path)
            .collect::<Vec<_>>();
        assert_eq!(
            selected.len(),
            1,
            "no copied row may stand in for a different admitted path"
        );
        let row = selected[0];
        assert_eq!(row.binding().file_id(), &file(path));
        assert_eq!(row.binding().canonical_path(), path);
        assert_eq!(
            row.binding().source_hash(),
            &SourceHash::from_source_bytes(bytes)
        );
        assert_eq!(row.binding().snapshot_binding(), context.snapshot_binding());
        assert_eq!((row.occurrence_range(), row.lhs_range()), (occurrence, lhs));
        if bytes == SOURCE {
            assert_eq!(row.lhs_kind(), &TypeScriptLhsKindV1::Identifier);
            assert!(
                matches!(row.outcome(), TypeScriptG3OutcomeV1::SourceObservation(_)),
                "identifier is bound source observation, never accepted writes relation"
            );
        } else {
            assert_eq!(row.lhs_kind(), &TypeScriptLhsKindV1::Field);
            let (observation, expected) = full_f4(context, path);
            assert_eq!(
                row.id(),
                &observation,
                "independent full F4 occurrence preimage"
            );
            assert!(
                matches!(row.outcome(), TypeScriptG3OutcomeV1::Obstructed {
                obstruction_id, reason } if obstruction_id == &expected
                && format!("{reason:?}").contains("UnsupportedAssignmentLhs")),
                "unsupported field retains typed obstruction XOR source-observation success"
            );
            assert!(batch.obstruction(&expected).is_some());
            obstructions.push(expected);
        }
    }
    assert_ne!(
        obstructions[0], obstructions[1],
        "F4 full preimages differ independently of file IDs"
    );
    let mut raw = parse_g3_syntax(SOURCE).expect("public parser returns raw syntax only");
    raw.assignments[0].occurrence_range = range(0, 123);
    assert_eq!(
        context.g3_observations().unwrap().writes().len(),
        4,
        "caller-forged raw values cannot alter the previously stored A0-owned batch"
    );
}

#[test]
fn malformed_source_and_overlap_remain_typed_file_or_construct_obstructions() {
    const OVERLAP: &[u8] = b"let a = 0, b = 0; a = b = 1; a += 1; a++;\n";
    assert_eq!(&OVERLAP[18..27], b"a = b = 1");
    assert_eq!(&OVERLAP[22..27], b"b = 1");
    let fixture = Fixture::admitted(&[(PATHS[0], OVERLAP), ("src/broken.ts", b"let = ;\n")]);
    let batch = fixture
        .context
        .g3_observations()
        .expect("parse failed file does not hide other rows");
    let rows = batch
        .writes()
        .iter()
        .filter(|row| row.binding().canonical_path() == PATHS[0])
        .collect::<Vec<_>>();
    assert_eq!(
        rows.len(),
        2,
        "nested assignment occurrences each accounted once"
    );
    assert_eq!(
        rows.iter()
            .map(|row| row.occurrence_range())
            .collect::<Vec<_>>(),
        [range(18, 27), range(22, 27)]
    );
    assert!(rows.iter().all(
        |row| matches!(row.outcome(), TypeScriptG3OutcomeV1::Obstructed {
        reason, .. } if format!("{reason:?}").contains("OverlappingOccurrence"))
    ));
    assert!(
        batch
            .file_partition()
            .iter()
            .any(|row| format!("{row:?}").contains("broken.ts")
                && format!("{row:?}").contains("ParseFailed"))
    );
    assert!(
        !batch
            .writes()
            .iter()
            .any(|row| row.binding().canonical_path() == "src/broken.ts")
    );
    assert!(
        batch
            .construct_partition()
            .exclusions()
            .iter()
            .any(|row| format!("{row:?}").contains("CompoundAssignment")),
        "compound excluded, not an accepted identifier write"
    );
}
