//! Common-v5 A02 — S00b (X3), S00c (U6/H5), S00d (C09 manifest + registration)
//! acceptance, frozen before production by acceptance author Acc-S00.
//!
//! Everything is exercised through the existing public CLI
//! surface only (`schema list|print|validate` and the real `review` binary for
//! legacy TypeScript controls); no new test API is assumed.
//!
//! Two layers are asserted separately:
//! * structural: the *printed* schema compiled with local resources only must
//!   accept the exact fixtures and reject every single-mutation negative;
//! * CLI `schema validate` (structural + local semantic dispatch): C09 manifests
//!   are fully determinable and are asserted positive and negative; for X3/U6/H5
//!   the exact producer contract hash is `[U]` (definition bytes not yet frozen),
//!   so only the "no fallthrough success" negative is asserted there.
//!
//! Fixture values are independent literals from the primaries (or bytes emitted
//! by the unchanged legacy TypeScript route), never outputs of the code under test.

use reviewgraphen_cli::run;
use reviewgraphen_core::{ContentHash, canonical_json};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path, process::Command, sync::OnceLock};
use tempfile::TempDir;

const AM: &str = "reviewgraphen.generic_review_artifact_manifest.v1";

const TS_REGISTRY_ID: &str = "reviewgraphen.source_review_registry.r2";
const TS_REGISTRY_HASH: &str =
    "sha256:4e4aa59c25c1c43257eb946de2aa0a99ad0ac193ad21b72a64f2163a1ec99a51";

// ---------------------------------------------------------------- helpers

fn cli(arguments: &[&str]) -> reviewgraphen_cli::CommandOutcome {
    run(arguments
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect())
}

fn listed() -> Vec<String> {
    let outcome = cli(&["schema", "list"]);
    assert_eq!(outcome.exit_code, 0, "schema list");
    serde_json::from_slice(&outcome.stdout).expect("schema list is a JSON string array")
}

fn printed(id: &str) -> Vec<u8> {
    let outcome = cli(&["schema", "print", id]);
    assert_eq!(
        outcome.exit_code, 0,
        "schema print {id} must be registered: {}",
        outcome.stderr
    );
    outcome.stdout
}

/// Compiles a printed schema with every printed listed schema that has an `$id`
/// available as a local resource: no network or filesystem resolution.
fn structural(id: &str) -> jsonschema::Validator {
    let schema: Value = serde_json::from_slice(&printed(id)).expect("printed schema is JSON");
    let mut options = jsonschema::options();
    for other in listed() {
        let value: Value = serde_json::from_slice(&printed(&other)).expect("listed schema JSON");
        if let Some(uri) = value.get("$id").and_then(Value::as_str).map(str::to_owned) {
            options = options.with_resource(
                uri,
                jsonschema::Resource::from_contents(value).expect("local schema resource"),
            );
        }
    }
    options
        .build(&schema)
        .expect("printed schema compiles offline")
}

fn assert_structure(validator: &jsonschema::Validator, doc: &Value, valid: bool, name: &str) {
    assert_eq!(
        validator.is_valid(doc),
        valid,
        "structural verdict for {name}"
    );
}

fn validate_bytes(bytes: &[u8]) -> (u8, Value) {
    let directory = TempDir::new().expect("temporary validation directory");
    let path = directory.path().join("document.json");
    fs::write(&path, bytes).expect("document bytes");
    let outcome = cli(&["schema", "validate", path.to_str().expect("UTF-8 path")]);
    let verdict = serde_json::from_slice(&outcome.stdout).unwrap_or(Value::Null);
    (outcome.exit_code, verdict)
}

fn assert_cli_valid(doc_bytes: &[u8], name: &str) {
    let (exit, verdict) = validate_bytes(doc_bytes);
    assert_eq!(
        exit, 0,
        "{name}: schema validate must accept; verdict={verdict}"
    );
    assert_eq!(verdict["valid"], true, "{name}");
}

/// A rejection must come from the registered family (structure or local
/// semantics), not from "unsupported_schema"; only existing reason literals.
fn assert_cli_rejected(doc_bytes: &[u8], name: &str) {
    let (exit, verdict) = validate_bytes(doc_bytes);
    assert_eq!(
        exit, 3,
        "{name}: schema validate must reject; verdict={verdict}"
    );
    assert_eq!(verdict["valid"], false, "{name}");
    let reason = verdict["reason"].as_str().unwrap_or_default();
    assert!(
        reason == "schema_invalid" || reason == "invalid_json",
        "{name}: rejected by the registered family, got reason {reason:?}"
    );
}

fn mutate(doc: &Value, edit: impl FnOnce(&mut Value)) -> Value {
    let mut copy = doc.clone();
    edit(&mut copy);
    copy
}

fn remove(doc: &Value, member: &str) -> Value {
    mutate(doc, |value| {
        value
            .as_object_mut()
            .expect("object")
            .remove(member)
            .expect("member present");
    })
}

fn hash(fill: char) -> String {
    format!("sha256:{}", fill.to_string().repeat(64))
}

fn kotlin_arm() -> Value {
    json!({"kind": "kotlin_r3", "version": 1})
}

// ------------------------------------------------ legacy TypeScript v5 capture

struct LegacyTypeScriptBundle {
    files: Vec<(String, Vec<u8>)>,
}

impl LegacyTypeScriptBundle {
    fn json(&self, name: &str) -> Value {
        let bytes = &self
            .files
            .iter()
            .find(|(path, _)| path == name)
            .expect(name)
            .1;
        serde_json::from_slice(bytes).expect("legacy artifact JSON")
    }
    fn bytes(&self, name: &str) -> &[u8] {
        &self
            .files
            .iter()
            .find(|(path, _)| path == name)
            .expect(name)
            .1
    }
}

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "Acc S00")
        .env("GIT_AUTHOR_EMAIL", "acc-s00@example.invalid")
        .env("GIT_COMMITTER_NAME", "Acc S00")
        .env("GIT_COMMITTER_EMAIL", "acc-s00@example.invalid")
        .output()
        .expect("git available");
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("UTF-8")
        .trim()
        .to_owned()
}

/// Runs the unchanged legacy TS v5 route once on a tiny literal repository.
fn legacy_typescript_bundle() -> &'static LegacyTypeScriptBundle {
    static BUNDLE: OnceLock<LegacyTypeScriptBundle> = OnceLock::new();
    BUNDLE.get_or_init(|| {
        let temporary = TempDir::new().expect("temporary repository");
        let repository = temporary.path().join("repository");
        fs::create_dir_all(repository.join("src")).expect("src directory");
        git(&repository, &["init", "-q"]);
        let source = |value: u32| {
            format!(
                "export function callee(): number {{ return {value}; }}\nexport function caller(): number {{ return callee(); }}\n"
            )
        };
        fs::write(repository.join("src/index.ts"), source(1)).expect("base source");
        git(&repository, &["add", "src/index.ts"]);
        git(&repository, &["commit", "-q", "-m", "base"]);
        let base = git(&repository, &["rev-parse", "HEAD"]);
        fs::write(repository.join("src/index.ts"), source(2)).expect("target source");
        git(&repository, &["add", "src/index.ts"]);
        git(&repository, &["commit", "-q", "-m", "target"]);
        let target = git(&repository, &["rev-parse", "HEAD"]);
        let request = json!({
            "schema": "reviewgraphen.generic_review_request.v5",
            "registry_id": TS_REGISTRY_ID, "registry_hash": TS_REGISTRY_HASH,
            "workspace_admission_root": ".", "repository_admission_root": ".",
            "repository_identity": "acc-s00/typescript@1",
            "base_revision": base, "target_revision": target,
            "ingest": {"profile_id": "typescript.production.v1", "profile_version": "1", "language": "typescript",
                "producer_id": "reviewgraphen.ingest.typescript_tree_sitter@1",
                "extractor_set_hash": "sha256:f5722ef19c0a2a4a5b6ff583f95aa8fdc9d0cf4f3cbc0c5631c041770842b39f",
                "rule_set_hash": "sha256:bcce4c84557970e6bc52a519b873ddd3c3eed33d518f49418da101f8c496c1d8",
                "max_files": 32, "max_file_bytes": 1_048_576, "max_total_source_bytes": 1_048_576},
            "execution_mode": "enumerate_and_defer",
            "projection_id": "typescript.obligation_report@1"
        });
        let request_path = temporary.path().join("request.v5.json");
        fs::write(&request_path, canonical_json(&request).expect("request")).expect("request bytes");
        let output = Command::new(env!("CARGO_BIN_EXE_reviewgraphen"))
            .args(["review", "--request"])
            .arg(&request_path)
            .args(["--artifacts", ".acc-s00-legacy-ts"])
            .current_dir(&repository)
            .output()
            .expect("legacy TS v5 review runs");
        assert_eq!(
            output.status.code(),
            Some(0),
            "legacy TS v5 route must succeed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let root = repository.join(".acc-s00-legacy-ts");
        let mut files = fs::read_dir(&root)
            .expect("artifact root")
            .map(|entry| {
                let entry = entry.expect("entry");
                let name = entry.file_name().into_string().expect("UTF-8 name");
                let bytes = fs::read(entry.path()).expect("artifact bytes");
                (name, bytes)
            })
            .collect::<Vec<_>>();
        files.sort();
        LegacyTypeScriptBundle { files }
    })
}

// ------------------------------------------------ S00d registration and C09

const OLD_REGISTERED: [(&str, &str, &str); 21] = [
    (
        "reviewgraphen.review.report.v1",
        "reviewgraphen.report.schema.json",
        "bee9fdd30497955ae21ad01a4eb3efbbe932cbfa63b0cc46e5a146118bd8cebf",
    ),
    (
        "reviewgraphen.review.report.v2",
        "reviewgraphen.report.v2.schema.json",
        "57e90ac9624b6f1ac1536fd8e65f67d88bb08b95c579886d0800c7d8f5c6ca58",
    ),
    (
        "reviewgraphen.review.report.v3",
        "reviewgraphen.report.v3.schema.json",
        "ee66f0996799ba38fb1d2ed48761b995b3b07a120fbd68a028af305a7780e1ae",
    ),
    (
        "reviewgraphen.review.report.v4",
        "reviewgraphen.report.v4.schema.json",
        "647e24f5befb0d5304825e4e9ffd536621b6ff9981c87cebb6dc4343d3ecb588",
    ),
    (
        "reviewgraphen.review.report.v5",
        "reviewgraphen.report.v5.schema.json",
        "0f6ef08a7a0b2819f73e661dce4878097d471ff1c5126e6ea55d8b4140198985",
    ),
    (
        "reviewgraphen.generic_review_request.v1",
        "reviewgraphen.generic_review_request.v1.schema.json",
        "85bcd466c62da5052949bcfbe0145fe620e5df22a20092acb426aff48ae1cb5f",
    ),
    (
        "reviewgraphen.reviewer_output.v1",
        "reviewgraphen.reviewer_output.v1.schema.json",
        "3c259cbc67396799957efd68f1543bb17450af8858148e2a7ef0af01e6908c82",
    ),
    (
        "reviewgraphen.reviewer_output.v2",
        "reviewgraphen.reviewer_output.v2.schema.json",
        "40e4daca2aad69db01479d771e7f232507061461c9b28673b9dfa8386ed99599",
    ),
    (
        "reviewgraphen.process_reviewer_record.v1",
        "reviewgraphen.process_reviewer_record.v1.schema.json",
        "303abd08a59c745675fc68a4c921580b8c90f29a94ec7333fac251b99192af65",
    ),
    (
        "reviewgraphen.generic_review_run.v1",
        "reviewgraphen.generic_review_run.v1.schema.json",
        "63eea719d580913c9d6d3e4ad982841c9d331887df487a2858373a1a6d79c048",
    ),
    (
        "reviewgraphen.generic_review_request.v2",
        "reviewgraphen.generic_review_request.v2.schema.json",
        "e8a36c5086781d8b603c1d98899acd5ff9476698d449eb2b1342c722d2a0884b",
    ),
    (
        "reviewgraphen.generic_review_run.v2",
        "reviewgraphen.generic_review_run.v2.schema.json",
        "46940bff45e8d6ac7c748f0682138fe5e17d1556d11fa450f0c71bc7ccf64877",
    ),
    (
        "reviewgraphen.generic_review_human_report.v1",
        "reviewgraphen.generic_review_human_report.v1.schema.json",
        "a0617be5415eda1349f9a7d3e7e101a94011371f435c13c91f23d3edd366409d",
    ),
    (
        "reviewgraphen.generic_review_request.v3",
        "reviewgraphen.generic_review_request.v3.schema.json",
        "a96ed31ad2dffbaa2466088792681a39c56658ab434c3550fd0d3effbb37f827",
    ),
    (
        "reviewgraphen.generic_review_run.v3",
        "reviewgraphen.generic_review_run.v3.schema.json",
        "e9a3bdaaf9dd37012f9adcefe048beae704fc01881e66e8a31bba78249f21f02",
    ),
    (
        "reviewgraphen.generic_review_human_report.v2",
        "reviewgraphen.generic_review_human_report.v2.schema.json",
        "175642c943f5f2bfa45bf3d675b06bc1120e1f609de32bb9dc63fb28cf8ca617",
    ),
    (
        "reviewgraphen.generic_review_request.v4",
        "reviewgraphen.generic_review_request.v4.schema.json",
        "adf0458abcfd88395c5a457b80a0be06354b824f3a459da08df779510384052d",
    ),
    (
        "reviewgraphen.generic_review_run.v4",
        "reviewgraphen.generic_review_run.v4.schema.json",
        "e703c191e21b55bcdda92f77bafc98daf892bde9402787a7f0c7fdb15b85391f",
    ),
    (
        "reviewgraphen.generic_review_human_report.v3",
        "reviewgraphen.generic_review_human_report.v3.schema.json",
        "734cdbd040f49a6dfb83952174870511bfb8fa168558aca1662c8168a473e439",
    ),
    (
        "reviewgraphen.generic_review_diagnostics.v1",
        "reviewgraphen.generic_review_diagnostics.v1.schema.json",
        "4bb931586908e50af881c5bb510a74cc808a70ec1f3769877a2c64614d5b1c93",
    ),
    (
        "reviewgraphen.responsibility_family_state.v1",
        "reviewgraphen.responsibility_family_state.v1.schema.json",
        "1284a794cea22706590f6a3977e60f44bb9eb87fc5f9eecd500e7cb8d6c8b5a6",
    ),
];

/// Existing unchanged definitions newly registered, pinned to F0 bytes.
const NEWLY_REGISTERED_EXISTING: [(&str, &str, &str); 5] = [
    (
        "reviewgraphen.generic_review_request.v5",
        "reviewgraphen.generic_review_request.v5.schema.json",
        "bb9989425acc3fa3ad94a8d76b76be0043b03e5c8f399ff67bfb94cf4558d4e3",
    ),
    (
        "reviewgraphen.extraction_report.v2",
        "reviewgraphen.extraction_report.v2.schema.json",
        "cd096fcd926878f54bfbafc0023cd7cea20bc8937f985b02334ca5a15081d1af",
    ),
    (
        "reviewgraphen.ingestion_report.v3",
        "reviewgraphen.ingestion_report.v3.schema.json",
        "5f282f9392b1fa2f84baa466004c20ab6acdedfc4834f763442b80e0bd7c1760",
    ),
    (
        "reviewgraphen.generic_review_run.v5",
        "reviewgraphen.generic_review_run.v5.schema.json",
        "1df93ccfc2757ec75a77f819d762d1ab1fc284053e332a862df0d562a85d378e",
    ),
    (
        "reviewgraphen.generic_review_human_report.v4",
        "reviewgraphen.generic_review_human_report.v4.schema.json",
        "4515614fbea9d760193fabe925a43f32410f924430ba9d7283b9f307e0494f61",
    ),
];

const NEW_FAMILIES: [(&str, &str); 1] = [(
    AM,
    "reviewgraphen.generic_review_artifact_manifest.v1.schema.json",
)];

fn schema_file(name: &str) -> Vec<u8> {
    fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../schemas")
            .join(name),
    )
    .expect("schema file")
}

#[test]
fn s00d_schema_list_is_exactly_the_old_set_plus_the_six_registrations() {
    let list = listed();
    let unique = list.iter().cloned().collect::<BTreeSet<_>>();
    assert_eq!(unique.len(), list.len(), "no duplicate schema IDs");
    let expected = OLD_REGISTERED
        .iter()
        .map(|(id, _, _)| *id)
        .chain(NEWLY_REGISTERED_EXISTING.iter().map(|(id, _, _)| *id))
        .chain(NEW_FAMILIES.iter().map(|(id, _)| *id))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    assert_eq!(expected.len(), 27);
    assert_eq!(unique, expected, "exact registered schema set");
}

#[test]
fn s00d_print_returns_exact_unchanged_or_installed_bytes() {
    for (id, file, sha) in OLD_REGISTERED
        .iter()
        .chain(NEWLY_REGISTERED_EXISTING.iter())
    {
        let bytes = printed(id);
        assert_eq!(
            ContentHash::sha256(&bytes).to_string(),
            format!("sha256:{sha}"),
            "{id} unchanged bytes"
        );
        assert_eq!(bytes, schema_file(file), "{id} printed from {file}");
    }
    for (id, file) in NEW_FAMILIES {
        let bytes = printed(id);
        assert_eq!(
            bytes,
            schema_file(file),
            "{id} printed from installed {file}"
        );
        serde_json::from_slice::<Value>(&bytes).expect("new schema JSON");
    }
    assert_eq!(
        cli(&["schema", "print", "reviewgraphen.extraction_report.v4"]).exit_code,
        2
    );
    assert_eq!(
        cli(&["schema", "print", "reviewgraphen.generic_review_run.v7"]).exit_code,
        2
    );
}

// C09 manifests -------------------------------------------------------------

fn row(path: &str, role: &str, fill: char, length: u64) -> Value {
    json!({"path": path, "role": role, "byte_length": length, "sha256": hash(fill)})
}

fn manifest(rows: Vec<Value>) -> Value {
    json!({"schema": AM, "request_sha256": hash('1'), "request_id": format!("request:{}", hash('1')),
        "run_id": "run:acc-s00", "snapshot_id": hash('2'), "universe_id": hash('3'), "artifacts": rows})
}

fn ts_v5_rows() -> Vec<Value> {
    vec![
        row("audit.run.v5.json", "audit", 'a', 10),
        row("extraction-report.v2.json", "extraction", 'b', 11),
        row(
            "human-report.manifest.v4.json",
            "human_report_manifest",
            'c',
            12,
        ),
        row("human-report.md", "human_report_markdown", 'd', 13),
        row("ingestion-report.v3.json", "ingestion", 'e', 14),
    ]
}

/// The retired Kotlin R3 publication rows; no longer an accepted family.
fn kotlin_v5_rows() -> Vec<Value> {
    vec![
        row("audit.run.v6.json", "audit", 'a', 10),
        row("extraction-report.v3.json", "extraction", 'b', 11),
        row(
            "human-report.manifest.v5.json",
            "human_report_manifest",
            'c',
            12,
        ),
        row("human-report.md", "human_report_markdown", 'd', 13),
        row("ingestion-report.v3.json", "ingestion", 'e', 14),
    ]
}

const EXEC_ONE: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const EXEC_TWO: &str = "2222222222222222222222222222222222222222222222222222222222222222";

fn record_pair(execution: &str) -> Vec<Value> {
    vec![
        row(
            &format!("records/{execution}.deterministic-observer-output.v1.json"),
            "deterministic_observer_output",
            'e',
            20,
        ),
        row(
            &format!("records/{execution}.provider-free-reviewer-packet.v1.json"),
            "reviewer_packet",
            'f',
            21,
        ),
    ]
}

fn legacy_rows(audit: &str, human: &str, executions: &[&str]) -> Vec<Value> {
    let mut rows = vec![
        row(audit, "audit", 'a', 10),
        row(human, "human_report_manifest", 'b', 11),
        row("human-report.md", "human_report_markdown", 'c', 12),
    ];
    for execution in executions {
        rows.extend(record_pair(execution));
    }
    rows
}

fn bytes(doc: &Value) -> Vec<u8> {
    canonical_json(doc).expect("canonical manifest")
}

#[test]
fn s00d_c09_accepts_exact_v5_and_legacy_manifest_families() {
    let am = structural(AM);
    let positives = [
        ("TS v5 five rows", manifest(ts_v5_rows())),
        (
            "v4 three rows",
            manifest(legacy_rows(
                "audit.run.v4.json",
                "human-report.manifest.v3.json",
                &[],
            )),
        ),
        (
            "v3 three rows + two record pairs",
            manifest(legacy_rows(
                "audit.run.v3.json",
                "human-report.manifest.v2.json",
                &[EXEC_ONE, EXEC_TWO],
            )),
        ),
        (
            "v2 three rows + one record pair",
            manifest(legacy_rows(
                "audit.run.v2.json",
                "human-report.manifest.v1.json",
                &[EXEC_ONE],
            )),
        ),
    ];
    for (name, doc) in positives {
        assert_structure(&am, &doc, true, name);
        assert_cli_valid(&bytes(&doc), name);
    }
}

#[test]
fn s00d_c09_rejects_self_rows_unpaired_records_mixed_majors_and_noncanonical_rows() {
    let v3 = legacy_rows(
        "audit.run.v3.json",
        "human-report.manifest.v2.json",
        &[EXEC_ONE],
    );
    let with = |mut rows: Vec<Value>, extra: Value| {
        rows.push(extra);
        rows.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
        manifest(rows)
    };
    let replace = |rows: &[Value], index: usize, value: Value| {
        let mut rows = rows.to_vec();
        rows[index] = value;
        manifest(rows)
    };
    let ts = ts_v5_rows();
    let kotlin = kotlin_v5_rows();
    let cases: Vec<(&str, Value)> = vec![
        (
            "self row",
            with(
                ts.clone(),
                row("artifact-manifest.v1.json", "audit", '9', 1),
            ),
        ),
        ("missing row", manifest(ts[..4].to_vec())),
        (
            "extra unknown file",
            with(ts.clone(), row("notes.json", "audit", '9', 1)),
        ),
        ("duplicate path", with(ts.clone(), ts[0].clone())),
        (
            "unsorted rows",
            manifest({
                let mut rows = ts.clone();
                rows.swap(0, 1);
                rows
            }),
        ),
        (
            "role/path mismatch",
            replace(
                &ts,
                1,
                row("extraction-report.v2.json", "ingestion", 'b', 11),
            ),
        ),
        (
            "unknown role",
            replace(&ts, 0, row("audit.run.v5.json", "run", 'a', 10)),
        ),
        (
            "mixed majors: X3 with run v5",
            replace(
                &ts,
                1,
                row("extraction-report.v3.json", "extraction", 'b', 11),
            ),
        ),
        (
            "retired Kotlin R3 publication family",
            manifest(kotlin.clone()),
        ),
        (
            "records on v5",
            with(ts.clone(), record_pair(EXEC_ONE)[0].clone()),
        ),
        (
            "records on v4",
            manifest(legacy_rows(
                "audit.run.v4.json",
                "human-report.manifest.v3.json",
                &[EXEC_ONE],
            )),
        ),
        (
            "v3 audit with v1 human manifest",
            manifest(legacy_rows(
                "audit.run.v3.json",
                "human-report.manifest.v1.json",
                &[EXEC_ONE],
            )),
        ),
        (
            "unpaired packet",
            manifest(
                v3.iter()
                    .filter(|r| !r["path"].as_str().unwrap_or_default().contains("observer"))
                    .cloned()
                    .collect(),
            ),
        ),
        (
            "mismatched execution basenames",
            manifest({
                let mut rows =
                    legacy_rows("audit.run.v3.json", "human-report.manifest.v2.json", &[]);
                rows.push(record_pair(EXEC_ONE)[0].clone());
                rows.push(record_pair(EXEC_TWO)[1].clone());
                rows
            }),
        ),
        (
            "swapped record roles",
            replace(
                &v3,
                3,
                row(
                    &format!("records/{EXEC_ONE}.deterministic-observer-output.v1.json"),
                    "reviewer_packet",
                    'e',
                    20,
                ),
            ),
        ),
        (
            "empty execution basename",
            manifest({
                let mut rows =
                    legacy_rows("audit.run.v3.json", "human-report.manifest.v2.json", &[]);
                rows.extend(record_pair(""));
                rows
            }),
        ),
        (
            "path traversal",
            replace(&ts, 0, row("../audit.run.v5.json", "audit", 'a', 10)),
        ),
        (
            "absolute path",
            replace(&ts, 0, row("/audit.run.v5.json", "audit", 'a', 10)),
        ),
        (
            "uppercase hash",
            replace(
                &ts,
                0,
                json!({"path": "audit.run.v5.json", "role": "audit", "byte_length": 10, "sha256": format!("sha256:{}", "A".repeat(64))}),
            ),
        ),
        (
            "negative length",
            replace(
                &ts,
                0,
                json!({"path": "audit.run.v5.json", "role": "audit", "byte_length": -1, "sha256": hash('a')}),
            ),
        ),
        (
            "fractional length",
            replace(
                &ts,
                0,
                json!({"path": "audit.run.v5.json", "role": "audit", "byte_length": 1.5, "sha256": hash('a')}),
            ),
        ),
        (
            "string length",
            replace(
                &ts,
                0,
                json!({"path": "audit.run.v5.json", "role": "audit", "byte_length": "10", "sha256": hash('a')}),
            ),
        ),
        (
            "row extra member",
            replace(
                &ts,
                0,
                json!({"path": "audit.run.v5.json", "role": "audit", "byte_length": 10, "sha256": hash('a'), "schema": "x"}),
            ),
        ),
        (
            "root extra member",
            mutate(&manifest(ts.clone()), |v| {
                v["publication_arm"] = kotlin_arm();
            }),
        ),
        (
            "root missing universe_id",
            remove(&manifest(ts.clone()), "universe_id"),
        ),
        (
            "empty run_id",
            mutate(&manifest(ts.clone()), |v| {
                v["run_id"] = json!("");
            }),
        ),
        (
            "request_sha256 not a hash",
            mutate(&manifest(ts.clone()), |v| {
                v["request_sha256"] = json!("1".repeat(64));
            }),
        ),
        (
            "wrong schema const",
            mutate(&manifest(ts.clone()), |v| {
                v["schema"] = json!("reviewgraphen.generic_review_artifact_manifest.v2");
            }),
        ),
    ];
    for (name, doc) in cases {
        assert_cli_rejected(&bytes(&doc), name);
    }
}

#[test]
fn s00d_c09_rejects_duplicate_keys_and_noncanonical_numbers_before_lossy_decoding() {
    let good = String::from_utf8(bytes(&manifest(ts_v5_rows()))).expect("UTF-8");
    assert_cli_valid(good.as_bytes(), "canonical control");
    let duplicate_root = good.replacen(
        "\"run_id\":\"run:acc-s00\"",
        "\"run_id\":\"\",\"run_id\":\"run:acc-s00\"",
        1,
    );
    assert_ne!(duplicate_root, good);
    assert_cli_rejected(
        duplicate_root.as_bytes(),
        "duplicate root key (last value valid)",
    );
    let duplicate_row = good.replacen(
        "\"role\":\"audit\"",
        "\"role\":\"extraction\",\"role\":\"audit\"",
        1,
    );
    assert_ne!(duplicate_row, good);
    assert_cli_rejected(
        duplicate_row.as_bytes(),
        "duplicate row key (last value valid)",
    );
    let exponent = good.replacen("\"byte_length\":10", "\"byte_length\":1e1", 1);
    assert_ne!(exponent, good);
    assert_cli_rejected(exponent.as_bytes(), "byte_length exponent lexeme");
    let fraction = good.replacen("\"byte_length\":10", "\"byte_length\":10.0", 1);
    assert_ne!(fraction, good);
    assert_cli_rejected(fraction.as_bytes(), "byte_length fraction lexeme");
}

#[test]
fn s00d_real_legacy_typescript_six_file_bundle_validates_through_registered_families() {
    let bundle = legacy_typescript_bundle();
    let names = bundle
        .files
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "artifact-manifest.v1.json",
            "audit.run.v5.json",
            "extraction-report.v2.json",
            "human-report.manifest.v4.json",
            "human-report.md",
            "ingestion-report.v3.json"
        ],
        "legacy TS route still emits its exact six files"
    );
    for name in [
        "artifact-manifest.v1.json",
        "audit.run.v5.json",
        "extraction-report.v2.json",
        "human-report.manifest.v4.json",
        "ingestion-report.v3.json",
    ] {
        assert_cli_valid(bundle.bytes(name), &format!("real legacy {name}"));
    }
    let manifest = bundle.json("artifact-manifest.v1.json");
    assert_structure(&structural(AM), &manifest, true, "real legacy TS manifest");
    let self_row = mutate(&manifest, |v| {
        v["artifacts"].as_array_mut().expect("rows").insert(
            0,
            json!({"path": "artifact-manifest.v1.json", "role": "audit", "byte_length": 1, "sha256": hash('9')}),
        );
    });
    assert_cli_rejected(&bytes(&self_row), "real legacy manifest plus self row");
}
