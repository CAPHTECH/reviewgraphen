//! Frozen, first-cohort acceptance contract for TypeScript source review.
//!
//! Every expected value in this file is derived from the frozen design,
//! never from the current implementation.  The current binary deliberately has
//! no request-v5 route, so the product-facing tests are expected to be red until
//! the corresponding slice exists.

use reviewgraphen_core::{ContentHash, canonical_json};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use tempfile::TempDir;

const REGISTRY: &[u8] = include_bytes!("fixtures/typescript-v1/registry.r1.json");
const REGISTRY_HASHES: &[u8] = include_bytes!("fixtures/typescript-v1/registry.r1.hashes.json");
const REGISTRY_ID: &str = "reviewgraphen.source_review_registry.r1";
const REGISTRY_HASH: &str =
    "sha256:c5a463f163782deb03fd4d5d259424c38ec9bec11c84e73c1092c0e1eced7f10";
const ARM_ID: &str = "typescript.production.v1.source@1";
const ARM_HASH: &str = "sha256:b127d16edd1d296f9dbf0cdfb745b5ce1c27416af72dd4de38f06855e8ccb93c";
const TUPLE_HASH: &str = "sha256:71f7cdfcf2b49a267caa1c5143cd83aa007a1da56bca48954be6f156ee3e50df";
const EXTRACTOR_SET_HASH: &str =
    "sha256:5feedacab3ab07da6152fc9e5a46ef3aabb188d98898cffd4b34f90aa7f30ae2";
const RULE_SET_HASH: &str =
    "sha256:4bbca92bc2b5bccd9ccfdd569055fe1465bed30304f94cb21ec1eba4b4f6ec2b";
const PROFILE_HASH: &str =
    "sha256:602483ac9ea899426e600b72f3fabba413de496908ece8341dc62617d857fb44";
const PROJECTION_ID: &str = "typescript.obligation_report@1";
const NODE_RULE: &str = "node.public_function_contract@2";
const NODE_PROPERTY: &str = "typescript.public_function_contract_review@1";
const D_RULE: &str = "relation.changed_public_callee@2";
const D_PROPERTY: &str = "typescript.callee_contract_review@1";
const GAP_RULE: &str = "capability_gap.origin_rule@1";

const SCHEMA_PAIRS: [(&str, &str); 6] = [
    (
        "../../schemas/reviewgraphen.review_profile.v2.schema.json",
        "../../schemas/reviewgraphen.review_profile.v2.example.json",
    ),
    (
        "../../schemas/reviewgraphen.extraction_report.v2.schema.json",
        "../../schemas/reviewgraphen.extraction_report.v2.example.json",
    ),
    (
        "../../schemas/reviewgraphen.ingestion_report.v3.schema.json",
        "../../schemas/reviewgraphen.ingestion_report.v3.example.json",
    ),
    (
        "../../schemas/reviewgraphen.generic_review_request.v5.schema.json",
        "../../schemas/reviewgraphen.generic_review_request.v5.example.json",
    ),
    (
        "../../schemas/reviewgraphen.generic_review_run.v5.schema.json",
        "../../schemas/reviewgraphen.generic_review_run.v5.example.json",
    ),
    (
        "../../schemas/reviewgraphen.generic_review_human_report.v4.schema.json",
        "../../schemas/reviewgraphen.generic_review_human_report.v4.example.json",
    ),
];

#[derive(Clone, Copy)]
struct Case {
    id: &'static str,
    source: &'static str,
    nodes: usize,
    reason: Option<&'static str>,
}

type FileTree = BTreeMap<String, String>;
type LocalFixture = (FileTree, FileTree, usize, usize, &'static [&'static str]);
type RelativeFixture = (
    FileTree,
    FileTree,
    usize,
    usize,
    &'static [&'static str],
    &'static [&'static str],
);

fn hash_json(value: &Value) -> String {
    ContentHash::sha256(&canonical_json(value).expect("canonical test literal")).to_string()
}

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "ReviewGraphen TypeScript Acceptance")
        .env("GIT_AUTHOR_EMAIL", "typescript-acceptance@example.invalid")
        .env("GIT_COMMITTER_NAME", "ReviewGraphen TypeScript Acceptance")
        .env(
            "GIT_COMMITTER_EMAIL",
            "typescript-acceptance@example.invalid",
        )
        .env("GIT_AUTHOR_DATE", "2026-09-15T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-09-15T00:00:00Z")
        .output()
        .expect("git is available");
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git emits UTF-8")
        .trim()
        .to_owned()
}

fn git_with_stdin(root: &Path, arguments: &[&str], input: &[u8]) -> String {
    let mut child = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "ReviewGraphen TypeScript Acceptance")
        .env("GIT_AUTHOR_EMAIL", "typescript-acceptance@example.invalid")
        .env("GIT_COMMITTER_NAME", "ReviewGraphen TypeScript Acceptance")
        .env(
            "GIT_COMMITTER_EMAIL",
            "typescript-acceptance@example.invalid",
        )
        .env("GIT_AUTHOR_DATE", "2026-09-15T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-09-15T00:00:00Z")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("git is available");
    child
        .stdin
        .as_mut()
        .expect("git stdin")
        .write_all(input)
        .expect("git fixture input");
    let output = child.wait_with_output().expect("git fixture completion");
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git emits UTF-8")
        .trim()
        .to_owned()
}

fn amended_target(root: &Path) -> String {
    git(root, &["commit", "-q", "--amend", "--no-edit"]);
    git(root, &["rev-parse", "HEAD"])
}

fn incomplete_target_tree(root: &Path, parent: &str) -> String {
    let src_tree = git(root, &["rev-parse", "HEAD:src"]);
    let source = format!(
        "040000 tree {src_tree}\tsrc\n040000 tree 1111111111111111111111111111111111111111\tzz-incomplete\n"
    );
    let tree = git_with_stdin(root, &["mktree", "--missing"], source.as_bytes());
    let commit = git(root, &["commit-tree", &tree, "-p", parent]);
    git(root, &["update-ref", "HEAD", &commit]);
    commit
}

fn write_tree(root: &Path, files: &BTreeMap<String, String>) {
    for (path, contents) in files {
        let destination = root.join(path);
        fs::create_dir_all(destination.parent().expect("fixture parent")).expect("fixture parent");
        fs::write(destination, contents).expect("fixture source");
    }
}

fn repository(
    base_files: BTreeMap<String, String>,
    target_files: BTreeMap<String, String>,
) -> (TempDir, PathBuf, String, String) {
    let temporary = tempfile::tempdir().expect("temporary fixture repository");
    let root = temporary.path().join("repository");
    fs::create_dir(&root).expect("repository directory");
    git(&root, &["init", "-q"]);
    write_tree(&root, &base_files);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "--allow-empty", "-m", "base"]);
    let base = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["rm", "-q", "-r", "--", "."]);
    write_tree(&root, &target_files);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "--allow-empty", "-m", "target"]);
    let target = git(&root, &["rev-parse", "HEAD"]);
    (temporary, root, base, target)
}

fn tree(rows: impl IntoIterator<Item = (&'static str, String)>) -> BTreeMap<String, String> {
    rows.into_iter()
        .map(|(path, contents)| (path.to_owned(), contents))
        .collect()
}

fn v5_request(base_revision: &str, target_revision: &str) -> Vec<u8> {
    canonical_json(&json!({
        "schema": "reviewgraphen.generic_review_request.v5",
        "registry_id": REGISTRY_ID,
        "registry_hash": REGISTRY_HASH,
        "workspace_admission_root": ".",
        "repository_admission_root": ".",
        "repository_identity": "reviewgraphen/typescript-v1-acceptance@1",
        "base_revision": base_revision,
        "target_revision": target_revision,
        "ingest": {
            "profile_id": "typescript.production.v1",
            "profile_version": "1",
            "language": "typescript",
            "producer_id": "reviewgraphen.ingest.typescript_tree_sitter@1",
            "extractor_set_hash": EXTRACTOR_SET_HASH,
            "rule_set_hash": RULE_SET_HASH,
            "max_files": 128,
            "max_file_bytes": 1_048_576,
            "max_total_source_bytes": 4_194_304
        },
        "execution_mode": "enumerate_and_defer",
        "projection_id": PROJECTION_ID
    }))
    .expect("canonical request literal")
}

fn review(root: &Path, request: &[u8], artifact_directory: &str) -> std::process::Output {
    fs::write(root.join("request.v5.json"), request).expect("request fixture");
    Command::new(env!("CARGO_BIN_EXE_reviewgraphen"))
        .args([
            "review",
            "--request",
            "request.v5.json",
            "--artifacts",
            artifact_directory,
        ])
        .current_dir(root)
        .output()
        .expect("review binary")
}

fn successful_v5_run(
    root: &Path,
    base: &str,
    target: &str,
    label: &str,
) -> (Value, Value, PathBuf) {
    successful_v5_run_with_request(root, &v5_request(base, target), label)
}

fn successful_v5_run_with_request(
    root: &Path,
    request: &[u8],
    label: &str,
) -> (Value, Value, PathBuf) {
    let output = review(root, request, "typescript-artifacts");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{label}: request-v5 TypeScript route must complete; stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let artifacts = root.join("typescript-artifacts");
    let extraction: Value = serde_json::from_slice(
        &fs::read(artifacts.join("extraction-report.v2.json")).expect("extraction artifact"),
    )
    .expect("extraction JSON");
    let run: Value = serde_json::from_slice(
        &fs::read(artifacts.join("audit.run.v5.json")).expect("run artifact"),
    )
    .expect("run JSON");
    (extraction, run, artifacts)
}

fn obligations<'a>(run: &'a Value, rule: &str) -> Vec<&'a Value> {
    run["obligations"]
        .as_array()
        .expect("v5 obligations")
        .iter()
        .filter(|obligation| obligation["rule_id"] == rule)
        .collect()
}

fn assert_common_run(run: &Value, nodes: usize, d: usize, label: &str) {
    assert_eq!(
        obligations(run, NODE_RULE).len(),
        nodes,
        "{label}: Node count"
    );
    assert_eq!(obligations(run, D_RULE).len(), d, "{label}: D count");
    assert_eq!(
        obligations(run, GAP_RULE).len(),
        1,
        "{label}: one partial-D gap"
    );
    for obligation in run["obligations"].as_array().expect("obligations") {
        let id = obligation["id"].as_str().expect("non-empty obligation ID");
        assert!(!id.is_empty(), "{label}: obligation ID form");
        assert_eq!(
            obligation["registry_binding"]["registry_hash"],
            REGISTRY_HASH
        );
        assert_eq!(obligation["status"], "deferred");
        match obligation["rule_id"].as_str() {
            Some(NODE_RULE) => assert_eq!(obligation["property_id"], NODE_PROPERTY),
            Some(D_RULE) => assert_eq!(obligation["property_id"], D_PROPERTY),
            Some(GAP_RULE) => assert_eq!(obligation["origin_rule_id"], D_RULE),
            Some(other) => panic!("{label}: unregistered obligation rule {other}"),
            None => panic!("{label}: missing obligation rule"),
        }
    }
    assert_eq!(run["plan_partition"]["planned_ids"], json!([]));
    assert_eq!(run["execution"]["mode"], "enumerate_and_defer");
    assert_eq!(run["execution"]["context_projection"], "unavailable");
    assert_eq!(run["execution"]["reviewer_execution"], "not_run");
    assert_eq!(run["execution"]["verifier_execution"], "not_run");
    assert_eq!(run["authority"]["trusted_pass"], false);
}

fn assert_typescript_file_language(extraction: &Value, path: &str, label: &str) {
    let row = extraction["file_records"]
        .as_array()
        .expect("file records")
        .iter()
        .find(|row| row["path"] == path)
        .unwrap_or_else(|| panic!("{label}: missing file record {path}"));
    assert_eq!(row["language"], "typescript", "{label}: file language");
}

#[test]
fn typescript_v1_registry_and_six_schema_families_are_frozen_independently() {
    let registry: Value = serde_json::from_slice(REGISTRY).expect("registry fixture JSON");
    let expected: Value = serde_json::from_slice(REGISTRY_HASHES).expect("hash fixture JSON");
    let arm = &registry["arms"][0];
    assert_eq!(registry["registry_id"], REGISTRY_ID);
    assert_eq!(arm["arm_id"], ARM_ID);
    assert_eq!(
        hash_json(&registry),
        REGISTRY_HASH,
        "registry canonical hash"
    );
    assert_eq!(hash_json(arm), ARM_HASH);
    assert_eq!(expected["arm_hash"], ARM_HASH);
    assert_eq!(hash_json(&arm["tuple"]), TUPLE_HASH);
    assert_eq!(hash_json(&arm["extractor_definition"]), EXTRACTOR_SET_HASH);
    assert_eq!(hash_json(&arm["profile_definition"]), PROFILE_HASH);
    assert_eq!(
        hash_json(&arm["projection_definition"]),
        expected["projection_hash"]
    );
    assert_eq!(hash_json(&arm["rules"]), RULE_SET_HASH);
    assert_eq!(
        arm["extractor_definition"]["grammar_bundle"]["sha256"],
        "sha256:48934f086feab3af672f42a8dd17f0912058b68ed0b99c9b7d9c498cf28600e9"
    );
    for descriptor in arm["payload_descriptors"].as_array().expect("descriptors") {
        assert_eq!(
            descriptor["descriptor_hash"],
            hash_json(&descriptor["definition"])
        );
    }
    for (schema_path, example_path) in SCHEMA_PAIRS {
        let schema: Value = serde_json::from_slice(
            &fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(schema_path)).expect("schema"),
        )
        .expect("schema JSON");
        let example: Value = serde_json::from_slice(
            &fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(example_path)).expect("example"),
        )
        .expect("example JSON");
        jsonschema::validator_for(&schema)
            .expect("schema compiles")
            .validate(&example)
            .unwrap_or_else(|error| panic!("{example_path}: {error}"));
    }
}

#[ignore = "superseded: the common-v5 TypeScript route refuses registry-r1 requests; changed-callee relations are covered by source review v6 (ADR 0053)"]
#[test]
fn typescript_v1_f1_public_callable_matrix_produces_only_specified_nodes() {
    let cases = [
        Case {
            id: "F1-P1",
            source: "export function f(){return 1}\n",
            nodes: 1,
            reason: None,
        },
        Case {
            id: "F1-P2",
            source: "export const f=()=>1\n",
            nodes: 1,
            reason: None,
        },
        Case {
            id: "F1-P3",
            source: "export const f=function(){return 1}\n",
            nodes: 1,
            reason: None,
        },
        Case {
            id: "F1-P4-named",
            source: "export default function f(){return 1}\n",
            nodes: 1,
            reason: None,
        },
        Case {
            id: "F1-P4-anonymous",
            source: "export default function(){return 1}\n",
            nodes: 1,
            reason: None,
        },
        Case {
            id: "F1-P5",
            source: "export default ()=>1\n",
            nodes: 1,
            reason: None,
        },
        Case {
            id: "F1-P6",
            source: "function f(){return 1}; export {f as g,f as default}\n",
            nodes: 1,
            reason: None,
        },
        Case {
            id: "F1-P7",
            source: "function f(){return 1}; export default f\n",
            nodes: 1,
            reason: None,
        },
        Case {
            id: "F1-P8-function",
            source: "function f(){return 1}\n",
            nodes: 0,
            reason: Some("non_public"),
        },
        Case {
            id: "F1-P8-arrow",
            source: "const f=()=>1\n",
            nodes: 0,
            reason: Some("non_public"),
        },
        Case {
            id: "F1-P9-literal",
            source: "export const n=1\n",
            nodes: 0,
            reason: Some("not_runtime_callable"),
        },
        Case {
            id: "F1-P9-type",
            source: "type T={n:number}; export type {T}\n",
            nodes: 0,
            reason: Some("not_runtime_callable"),
        },
        Case {
            id: "F1-P10",
            source: "export class C { m(){return 1} }\n",
            nodes: 0,
            reason: Some("unsupported_callable_kind"),
        },
        Case {
            id: "F1-P11-let",
            source: "export let f=()=>1\n",
            nodes: 0,
            reason: Some("unsupported_syntax"),
        },
        Case {
            id: "F1-P11-factory",
            source: "export const f=factory()\n",
            nodes: 0,
            reason: Some("unsupported_syntax"),
        },
        Case {
            id: "F1-P11-overload",
            source: "export function f(x:string):string; export function f(x:any){return x}\n",
            nodes: 0,
            reason: Some("unsupported_syntax"),
        },
        Case {
            id: "F1-P12-namespace",
            source: "export namespace N { export function f(){return 1} }\n",
            nodes: 0,
            reason: Some("out_of_scope"),
        },
        Case {
            id: "F1-P12-nested",
            source: "function outer(){function inner(){return 1};return inner()}\n",
            nodes: 0,
            reason: Some("out_of_scope"),
        },
        Case {
            id: "F1-P12-callback",
            source: "consume(()=>1)\n",
            nodes: 0,
            reason: Some("out_of_scope"),
        },
        Case {
            id: "F1-P11-wrapper",
            source: "export const f=(function(){return 1})\n",
            nodes: 0,
            reason: Some("unsupported_syntax"),
        },
        Case {
            id: "F1-P11-commonjs",
            source: "function f(){return 1}; module.exports=f\n",
            nodes: 0,
            reason: Some("unsupported_syntax"),
        },
        Case {
            id: "F1-P12-class-field",
            source: "export class C { f=()=>1 }\n",
            nodes: 0,
            reason: Some("unsupported_callable_kind"),
        },
        Case {
            id: "F1-P13-async",
            source: "export async function f(){return 1}\n",
            nodes: 1,
            reason: None,
        },
        Case {
            id: "F1-P13-generator",
            source: "export function* f(){yield 1}\n",
            nodes: 1,
            reason: None,
        },
        Case {
            id: "F1-P13-typed-arrow",
            source: "export const f:()=>number=()=>1\n",
            nodes: 1,
            reason: None,
        },
    ];
    for case in cases {
        let files = tree([("src/public.ts", case.source.to_owned())]);
        let (_temporary, root, base, target) = repository(files.clone(), files);
        let (extraction, run, _) = successful_v5_run(&root, &base, &target, case.id);
        assert_common_run(&run, case.nodes, 0, case.id);
        assert_typescript_file_language(&extraction, "src/public.ts", case.id);
        if let Some(reason) = case.reason {
            assert!(
                extraction["syntax_records"]
                    .as_array()
                    .expect("syntax records")
                    .iter()
                    .any(|row| {
                        row["reasons"]
                            .as_array()
                            .is_some_and(|reasons| reasons.iter().any(|entry| entry == reason))
                    }),
                "{}: expected catalog reason {reason}",
                case.id
            );
        }
    }
}

fn local_case(id: &str) -> LocalFixture {
    let base = "export function f(){return 1}\nfunction g(){return f()}\n";
    let changed = "export function f(){return 2}\nfunction g(){return f()}\n";
    let (base, target, nodes, d, reasons): (String, String, usize, usize, &[&str]) = match id {
        "F2-C1" => (base.into(), changed.into(), 1, 1, &[]),
        "F2-C2" => (
            base.into(),
            "export function f(){return 2}\nfunction g(){return f()+f()}\n".into(),
            1,
            1,
            &[],
        ),
        "F2-C3" => (base.into(), base.into(), 1, 0, &["unchanged_callee"]),
        "F2-C4" => (
            "function f(){return 1}\nfunction g(){return f()}\n".into(),
            "function f(){return 2}\nfunction g(){return f()}\n".into(),
            0,
            0,
            &["non_public_callee"],
        ),
        "F2-C5" => (
            base.into(),
            "export function f(){return 1}\nfunction g(){return f()+1}\n".into(),
            1,
            0,
            &["unchanged_callee"],
        ),
        "F2-C6" => (
            "function f(){return 1}\nfunction g(){return f()}\n".into(),
            "function f(){return 1}\nexport {f}\nfunction g(){return f()}\n".into(),
            1,
            1,
            &[],
        ),
        "F2-C7-parameter" => (
            base.into(),
            "export function f(){return 2}\nfunction g(f:()=>number){return f()}\n".into(),
            1,
            0,
            &["shadowed_binding"],
        ),
        "F2-C7-block" => (
            base.into(),
            "export function f(){return 2}\nfunction g(){let f=()=>1;return f()}\n".into(),
            1,
            0,
            &["shadowed_binding"],
        ),
        "F2-C8-write" => (
            base.into(),
            "export function f(){return 2}\nf=()=>3\nfunction g(){return f()}\n".into(),
            1,
            0,
            &["written_binding"],
        ),
        "F2-C8-eval" => (
            base.into(),
            "export function f(){return 2}\neval('f')\nfunction g(){return f()}\n".into(),
            1,
            0,
            &["unsupported_syntax"],
        ),
        "F2-C9-optional" => (
            base.into(),
            "export function f(){return 2}\nfunction g(){return f?.()}\n".into(),
            1,
            0,
            &["dynamic_dispatch"],
        ),
        "F2-C9-member" => (
            base.into(),
            "export function f(){return 2}\nfunction g(){return obj.f()}\n".into(),
            1,
            0,
            &["dynamic_dispatch"],
        ),
        "F2-C9-new" => (
            base.into(),
            "export function f(){return 2}\nfunction g(){return new F()}\n".into(),
            1,
            0,
            &["dynamic_dispatch"],
        ),
        "F2-C9-alias" => (
            base.into(),
            "export function f(){return 2}\nfunction g(){return (f)()}\n".into(),
            1,
            0,
            &["unsupported_syntax"],
        ),
        "F2-C9-method" => (
            base.into(),
            "export function f(){return 2}\nclass C{g(){return f()}}\n".into(),
            1,
            0,
            &["unsupported_caller"],
        ),
        "F2-C10" => (
            base.into(),
            "export function f(){return 2}\nconst ref=f\nfunction g(){return 1}\n".into(),
            1,
            0,
            &[],
        ),
        "F2-C11" => (
            "export const f=()=>1\nfunction g(){return f()}\n".into(),
            "export const f=()=>2\nfunction g(){return f()}\n".into(),
            1,
            1,
            &[],
        ),
        "F2-C12" => (
            base.into(),
            "function g(){return 1}\n".into(),
            0,
            0,
            &["target_removed"],
        ),
        _ => panic!("unknown local case {id}"),
    };
    (
        tree([("src/local.ts", base)]),
        tree([("src/local.ts", target)]),
        nodes,
        d,
        reasons,
    )
}

#[ignore = "superseded: the common-v5 TypeScript route refuses registry-r1 requests; changed-callee relations are covered by source review v6 (ADR 0053)"]
#[test]
fn typescript_v1_f2_local_calls_change_witnesses_and_negative_reasons_are_accounted() {
    let ids = [
        "F2-C1",
        "F2-C2",
        "F2-C3",
        "F2-C4",
        "F2-C5",
        "F2-C6",
        "F2-C7-parameter",
        "F2-C7-block",
        "F2-C8-write",
        "F2-C8-eval",
        "F2-C9-optional",
        "F2-C9-member",
        "F2-C9-new",
        "F2-C9-alias",
        "F2-C9-method",
        "F2-C10",
        "F2-C11",
        "F2-C12",
    ];
    for id in ids {
        let (base_files, target_files, nodes, d, reasons) = local_case(id);
        let (_temporary, root, base, target) = repository(base_files, target_files);
        let (extraction, run, _) = successful_v5_run(&root, &base, &target, id);
        assert_common_run(&run, nodes, d, id);
        assert_typescript_file_language(&extraction, "src/local.ts", id);
        for reason in reasons {
            assert!(
                extraction.to_string().contains(reason),
                "{id}: missing independently specified reason {reason}"
            );
        }
    }
}

fn relative_case(id: &str) -> RelativeFixture {
    let api_base = "export function f(){return 1}\n";
    let api_target = "export function f(){return 2}\n";
    let client = "import {f} from './api'\nexport function run(){return f()}\n";
    let mut base = tree([
        ("src/api.ts", api_base.into()),
        ("src/client.ts", client.into()),
    ]);
    let mut target = tree([
        ("src/api.ts", api_target.into()),
        ("src/client.ts", client.into()),
    ]);
    let mut nodes = 2;
    let mut d = 1;
    let mut reasons: &[&str] = &[];
    let mut candidates: &[&str] = &[
        "src/api",
        "src/api.ts",
        "src/api.tsx",
        "src/api/index.ts",
        "src/api/index.tsx",
    ];
    match id {
        "F3-R01" => {}
        "F3-R02" => {
            target.insert(
                "src/client.ts".into(),
                "import {f as local} from './api'\nexport function run(){return local()}\n".into(),
            );
        }
        "F3-R03" => {
            base.insert(
                "src/api.ts".into(),
                "export default function f(){return 1}\n".into(),
            );
            target.insert(
                "src/api.ts".into(),
                "export default function f(){return 2}\n".into(),
            );
            target.insert(
                "src/client.ts".into(),
                "import local from './api'\nexport function run(){return local()}\n".into(),
            );
        }
        "F3-R03a" => {
            base.insert(
                "src/api.ts".into(),
                "export default function(){return 1}\n".into(),
            );
            target.insert(
                "src/api.ts".into(),
                "export default function(){return 2}\n".into(),
            );
            target.insert(
                "src/client.ts".into(),
                "import local from './api'\nexport function run(){return local()}\n".into(),
            );
        }
        "F3-R03b" => {
            base.insert("src/api.ts".into(), "export default ()=>1\n".into());
            target.insert("src/api.ts".into(), "export default ()=>2\n".into());
            target.insert(
                "src/client.ts".into(),
                "import local from './api'\nexport function run(){return local()}\n".into(),
            );
        }
        "F3-R04" => {
            target.insert(
                "src/client.ts".into(),
                "import {f} from './api.js'\nexport function run(){return f()}\n".into(),
            );
            candidates = &["src/api.js", "src/api.ts", "src/api.tsx"];
        }
        "F3-R05" => {
            base.remove("src/api.ts");
            target.remove("src/api.ts");
            base.insert("src/api/index.ts".into(), api_base.into());
            target.insert("src/api/index.ts".into(), api_target.into());
            candidates = &[
                "src/api",
                "src/api.ts",
                "src/api.tsx",
                "src/api/index.ts",
                "src/api/index.tsx",
            ];
        }
        "F3-R06" => {
            base.remove("src/client.ts");
            target.remove("src/client.ts");
            base.insert(
                "src/ui/client.ts".into(),
                "import {f} from '../api'\nexport function run(){return f()}\n".into(),
            );
            target.insert(
                "src/ui/client.ts".into(),
                "import {f} from '../api'\nexport function run(){return f()}\n".into(),
            );
            candidates = &[
                "src/api",
                "src/api.ts",
                "src/api.tsx",
                "src/api/index.ts",
                "src/api/index.tsx",
            ];
        }
        "F3-R07" => {
            target.insert(
                "src/client.ts".into(),
                "import {\n f as local,\n} from './api'\nexport function run(){return local()}\n"
                    .into(),
            );
        }
        "F3-R08" => {
            target.insert(
                "src/client.ts".into(),
                "import {f} from './api.ts'\nexport function run(){return f()}\n".into(),
            );
            candidates = &["src/api.ts"];
        }
        "F3-R09" => {
            base.insert(
                "src/api.ts".into(),
                "function f(){return 1}; export {f as renamed}\n".into(),
            );
            target.insert(
                "src/api.ts".into(),
                "function f(){return 2}; export {f as renamed}\n".into(),
            );
            target.insert(
                "src/client.ts".into(),
                "import {renamed as local} from './api'\nexport function run(){return local()}\n"
                    .into(),
            );
        }
        "F3-R10a" => {
            base.insert(
                "src/api.ts".into(),
                "function f(){return 1}; export {f as default}\n".into(),
            );
            target.insert(
                "src/api.ts".into(),
                "function f(){return 2}; export {f as default}\n".into(),
            );
            target.insert(
                "src/client.ts".into(),
                "import local from './api'\nexport function run(){return local()}\n".into(),
            );
        }
        "F3-R10b" => {
            base.insert(
                "src/api.ts".into(),
                "function f(){return 1}; export default f\n".into(),
            );
            target.insert(
                "src/api.ts".into(),
                "function f(){return 2}; export default f\n".into(),
            );
            target.insert(
                "src/client.ts".into(),
                "import local from './api'\nexport function run(){return local()}\n".into(),
            );
        }
        "F3-R11" => {
            target.insert("src/api/index.ts".into(), api_target.into());
            nodes = 2;
            d = 0;
            reasons = &["relative_target_ambiguous"];
        }
        "F3-R12" => {
            target.insert(
                "src/api.tsx".into(),
                "export function f(){return 2}\n".into(),
            );
            nodes = 2;
            d = 0;
            reasons = &["relative_target_ambiguous", "relative_target_unread"];
        }
        "F3-R13" => {
            target.insert(
                "src/api.js".into(),
                "export function f(){return 2}\n".into(),
            );
            target.insert(
                "src/client.ts".into(),
                "import {f} from './api.js'\nexport function run(){return f()}\n".into(),
            );
            d = 0;
            reasons = &["relative_target_ambiguous"];
            candidates = &["src/api.js", "src/api.ts", "src/api.tsx"];
        }
        "F3-R14a" => {
            base.remove("src/api.ts");
            target.remove("src/api.ts");
            base.insert(
                "src/api.js".into(),
                "export function f(){return 1}\n".into(),
            );
            target.insert(
                "src/api.js".into(),
                "export function f(){return 2}\n".into(),
            );
            nodes = 1;
            d = 0;
            reasons = &["relative_target_ambiguous", "relative_target_missing"];
        }
        "F3-R14b" => {
            target.insert(
                "src/api/index.js".into(),
                "export function f(){return 2}\n".into(),
            );
            nodes = 2;
            d = 0;
            reasons = &["relative_target_ambiguous"];
        }
        "F3-R15" => {
            base.remove("src/api.ts");
            target.remove("src/api.ts");
            nodes = 1;
            d = 0;
            reasons = &["relative_target_missing"];
        }
        "F3-R16" => {
            base.remove("src/api.ts");
            target.remove("src/api.ts");
            base.insert("src/api.test.ts".into(), api_base.into());
            target.insert("src/api.test.ts".into(), api_target.into());
            target.insert(
                "src/client.ts".into(),
                "import {f} from './api.test.ts'\nexport function run(){return f()}\n".into(),
            );
            nodes = 1;
            d = 0;
            reasons = &["relative_target_excluded"];
            candidates = &["src/api.test.ts"];
        }
        "F3-R17" => {
            target.insert(
                "src/api.ts".into(),
                format!("export function f(){{return 2}}\n//{}\n", "x".repeat(256)),
            );
            nodes = 1;
            d = 0;
            reasons = &["relative_target_unread"];
        }
        "F3-R18" => {
            target.insert("src/api.ts".into(), "export function f( {\n".into());
            nodes = 1;
            d = 0;
            reasons = &["parse_failure", "relative_target_unread"];
        }
        "F3-R19a-symlink" => {
            nodes = 1;
            d = 0;
            reasons = &["relative_target_unread"];
        }
        "F3-R19b-submodule-ancestor" => {
            base.remove("src/client.ts");
            target.remove("src/client.ts");
            base.insert(
                "src/client.ts".into(),
                "import {f} from './api/inside'\nexport function run(){return f()}\n".into(),
            );
            target.insert(
                "src/client.ts".into(),
                "import {f} from './api/inside'\nexport function run(){return f()}\n".into(),
            );
            nodes = 1;
            d = 0;
            reasons = &["relative_target_unread"];
            candidates = &[
                "src/api/inside",
                "src/api/inside.ts",
                "src/api/inside.tsx",
                "src/api/inside/index.ts",
                "src/api/inside/index.tsx",
            ];
        }
        "F3-R20" => {
            target.insert(
                "src/client.ts".into(),
                "import {f} from '../../outside'\nexport function run(){return f()}\n".into(),
            );
            nodes = 1;
            d = 0;
            reasons = &["relative_specifier_unsupported"];
        }
        "F3-R21a-parameter" => {
            target.insert("src/client.ts".into(), "import {f as local} from './api'\nexport function run(local:()=>number){return local()}\n".into());
            d = 0;
            reasons = &["shadowed_binding"];
        }
        "F3-R21b-block" => {
            target.insert("src/client.ts".into(), "import {f as local} from './api'\nexport function run(){let local=()=>1;return local()}\n".into());
            d = 0;
            reasons = &["shadowed_binding"];
        }
        "F3-R22a-duplicate-import" => {
            target.insert("src/client.ts".into(), "import {f} from './api'\nimport {f as f} from './api'\nexport function run(){return f()}\n".into());
            d = 0;
            reasons = &["import_binding_ambiguous"];
        }
        "F3-R22b-local-declaration" => {
            target.insert(
                "src/client.ts".into(),
                "import {f} from './api'\nconst f=()=>1\nexport function run(){return f()}\n"
                    .into(),
            );
            d = 0;
            reasons = &["import_binding_ambiguous"];
        }
        "F3-R23a-import-write" => {
            target.insert(
                "src/client.ts".into(),
                "import {f} from './api'\nf=()=>1\nexport function run(){return f()}\n".into(),
            );
            d = 0;
            reasons = &["written_binding"];
        }
        "F3-R23b-direct-eval" => {
            target.insert(
                "src/client.ts".into(),
                "import {f} from './api'\neval('f')\nexport function run(){return f()}\n".into(),
            );
            d = 0;
            reasons = &["unsupported_syntax"];
        }
        "F3-R23c-callee-write" => {
            target.insert(
                "src/api.ts".into(),
                "export function f(){return 2}\nf=()=>3\n".into(),
            );
            d = 0;
            reasons = &["written_binding"];
        }
        "F3-R24a-import-type" => {
            target.insert(
                "src/client.ts".into(),
                "import type {f} from './api'\nexport function run(){return f()}\n".into(),
            );
            d = 0;
            reasons = &["type_only_binding"];
        }
        "F3-R24b-named-type" => {
            target.insert(
                "src/client.ts".into(),
                "import {type f} from './api'\nexport function run(){return f()}\n".into(),
            );
            d = 0;
            reasons = &["type_only_binding"];
        }
        "F3-R24c-type-only-export" => {
            target.insert("src/api.ts".into(), "export type f=()=>number\n".into());
            nodes = 1;
            d = 0;
            reasons = &["export_binding_unsupported"];
        }
        "F3-R25a-duplicate-slot" => {
            target.insert(
                "src/api.ts".into(),
                "export function f(){return 2}; export {f as f}\n".into(),
            );
            d = 0;
            reasons = &["export_binding_unsupported"];
        }
        "F3-R25b-default-named" => {
            target.insert(
                "src/api.ts".into(),
                "export default function f(){return 2}\n".into(),
            );
            d = 0;
            reasons = &["export_binding_unsupported"];
        }
        "F3-R25c-noncallable" => {
            target.insert("src/api.ts".into(), "export const f=1\n".into());
            nodes = 1;
            d = 0;
            reasons = &["export_binding_unsupported"];
        }
        "F3-R26a-factory" => {
            target.insert("src/api.ts".into(), "export const f=factory()\n".into());
            nodes = 1;
            d = 0;
            reasons = &["export_binding_unsupported"];
        }
        "F3-R26b-overload" => {
            target.insert(
                "src/api.ts".into(),
                "export function f(x:string):string; export function f(x:any){return x}\n".into(),
            );
            nodes = 1;
            d = 0;
            reasons = &["export_binding_unsupported"];
        }
        "F3-R26c-wrapper" => {
            target.insert(
                "src/api.ts".into(),
                "export const f=(function(){return 2})\n".into(),
            );
            nodes = 2;
        }
        "F3-R27" => {
            base.insert(
                "src/barrel.ts".into(),
                "export {f as g} from './api'\n".into(),
            );
            target.insert(
                "src/barrel.ts".into(),
                "export {f as g} from './api'\n".into(),
            );
            target.insert(
                "src/client.ts".into(),
                "import {g as f} from './barrel'\nexport function run(){return f()}\n".into(),
            );
            d = 0;
            reasons = &["export_binding_unsupported"];
        }
        "F3-R28a-star" => {
            base.insert("src/barrel.ts".into(), "export * from './api'\n".into());
            target.insert("src/barrel.ts".into(), "export * from './api'\n".into());
            target.insert(
                "src/client.ts".into(),
                "import {f} from './barrel'\nexport function run(){return f()}\n".into(),
            );
            d = 0;
            reasons = &["export_binding_unsupported"];
        }
        "F3-R28b-namespace" => {
            base.insert(
                "src/barrel.ts".into(),
                "export * as ns from './api'\n".into(),
            );
            target.insert(
                "src/barrel.ts".into(),
                "export * as ns from './api'\n".into(),
            );
            target.insert(
                "src/client.ts".into(),
                "import {ns} from './barrel'\nexport function run(){return ns()}\n".into(),
            );
            d = 0;
            reasons = &["export_binding_unsupported"];
        }
        "F3-R29a-bare" => {
            target.insert(
                "src/client.ts".into(),
                "import {f} from 'package'\nexport function run(){return f()}\n".into(),
            );
            d = 0;
            reasons = &["import_resolution_unavailable"];
        }
        "F3-R29b-path-alias" => {
            target.insert(
                "src/client.ts".into(),
                "import {f} from '@api'\nexport function run(){return f()}\n".into(),
            );
            d = 0;
            reasons = &["import_resolution_unavailable"];
        }
        "F3-R30" => {
            target.insert(
                "src/client.ts".into(),
                "import * as ns from './api'\nexport function run(){return ns.f()}\n".into(),
            );
            d = 0;
            reasons = &["dynamic_dispatch", "import_resolution_unavailable"];
        }
        "F3-R31" => {
            base.remove("src/api.ts");
            target.remove("src/api.ts");
            base.insert(
                "src/api.tsx".into(),
                "export function f(){return 1}\n".into(),
            );
            target.insert(
                "src/api.tsx".into(),
                "export function f(){return 2}\n".into(),
            );
            nodes = 1;
            d = 0;
            reasons = &["relative_target_unread"];
        }
        "F3-R32a-excluded-index" => {
            base.remove("src/api.ts");
            target.remove("src/api.ts");
            base.insert("src/examples.ts".into(), api_base.into());
            target.insert("src/examples.ts".into(), api_target.into());
            target.insert("src/examples/index.ts".into(), api_target.into());
            target.insert(
                "src/client.ts".into(),
                "import {f} from './examples'\nexport function run(){return f()}\n".into(),
            );
            nodes = 2;
            d = 0;
            reasons = &["relative_target_ambiguous", "relative_target_excluded"];
            candidates = &[
                "src/examples",
                "src/examples.ts",
                "src/examples.tsx",
                "src/examples/index.ts",
                "src/examples/index.tsx",
            ];
        }
        "F3-R32b-unread-index" => {
            target.insert("src/api/index.ts".into(), api_target.into());
            target.insert(
                "src/api/index.ts".into(),
                format!("export function f(){{return 2}}\n//{}\n", "x".repeat(256)),
            );
            nodes = 2;
            d = 0;
            reasons = &["relative_target_ambiguous", "relative_target_unread"];
        }
        "F3-R33" => {
            target.insert("src/api.ts".into(), api_base.into());
            target.insert(
                "src/client.ts".into(),
                "import {f as local} from './api'\nexport function run(){return local()}\n".into(),
            );
            d = 0;
            reasons = &["unchanged_callee"];
        }
        "F3-R34a-mts" => {
            target.insert(
                "src/client.ts".into(),
                "import {f} from './api.mts'\nexport function run(){return f()}\n".into(),
            );
            d = 0;
            reasons = &["relative_specifier_unsupported"];
        }
        "F3-R34b-escaped" => {
            target.insert(
                "src/client.ts".into(),
                "import {f} from './api\\\\name'\nexport function run(){return f()}\n".into(),
            );
            d = 0;
            reasons = &["relative_specifier_unsupported"];
        }
        "F3-R34c-query" => {
            target.insert(
                "src/client.ts".into(),
                "import {f} from './api?raw'\nexport function run(){return f()}\n".into(),
            );
            d = 0;
            reasons = &["relative_specifier_unsupported"];
        }
        "F3-R35" => {}
        "F3-R36" => {
            nodes = 2;
            d = 0;
            reasons = &["relative_target_unread"];
        }
        _ => panic!("unknown relative case {id}"),
    }
    (base, target, nodes, d, reasons, candidates)
}

#[ignore = "superseded: the common-v5 TypeScript route refuses registry-r1 requests; changed-callee relations are covered by source review v6 (ADR 0053)"]
#[test]
fn typescript_v1_f3_relative_import_matrix_keeps_candidate_and_reason_closure() {
    let ids = [
        "F3-R01",
        "F3-R02",
        "F3-R03",
        "F3-R03a",
        "F3-R03b",
        "F3-R04",
        "F3-R05",
        "F3-R06",
        "F3-R07",
        "F3-R08",
        "F3-R09",
        "F3-R10a",
        "F3-R10b",
        "F3-R11",
        "F3-R12",
        "F3-R13",
        "F3-R14a",
        "F3-R14b",
        "F3-R15",
        "F3-R16",
        "F3-R17",
        "F3-R18",
        "F3-R19a-symlink",
        "F3-R19b-submodule-ancestor",
        "F3-R20",
        "F3-R21a-parameter",
        "F3-R21b-block",
        "F3-R22a-duplicate-import",
        "F3-R22b-local-declaration",
        "F3-R23a-import-write",
        "F3-R23b-direct-eval",
        "F3-R23c-callee-write",
        "F3-R24a-import-type",
        "F3-R24b-named-type",
        "F3-R24c-type-only-export",
        "F3-R25a-duplicate-slot",
        "F3-R25b-default-named",
        "F3-R25c-noncallable",
        "F3-R26a-factory",
        "F3-R26b-overload",
        "F3-R26c-wrapper",
        "F3-R27",
        "F3-R28a-star",
        "F3-R28b-namespace",
        "F3-R29a-bare",
        "F3-R29b-path-alias",
        "F3-R30",
        "F3-R31",
        "F3-R32a-excluded-index",
        "F3-R32b-unread-index",
        "F3-R33",
        "F3-R34a-mts",
        "F3-R34b-escaped",
        "F3-R34c-query",
        "F3-R35",
        "F3-R36",
    ];
    for id in ids {
        let (base_files, target_files, nodes, d, reasons, candidates) = relative_case(id);
        let (_temporary, root, base, mut target) = repository(base_files, target_files);
        match id {
            "F3-R19a-symlink" => {
                fs::remove_file(root.join("src/api.ts"))
                    .expect("replace target candidate with symlink");
                std::os::unix::fs::symlink("../outside", root.join("src/api.ts"))
                    .expect("target candidate symlink");
                git(&root, &["add", "-A"]);
                target = amended_target(&root);
            }
            "F3-R19b-submodule-ancestor" => {
                fs::remove_file(root.join("src/api.ts")).expect("remove normal candidate");
                git(&root, &["rm", "-q", "--cached", "src/api.ts"]);
                git(
                    &root,
                    &[
                        "update-index",
                        "--add",
                        "--cacheinfo",
                        "160000,1111111111111111111111111111111111111111,src/api",
                    ],
                );
                target = amended_target(&root);
            }
            "F3-R36" => target = incomplete_target_tree(&root, &target),
            _ => {}
        }
        if id == "F3-R35" {
            fs::write(root.join("src/api.tsx"), "export function f(){return 99}\n")
                .expect("untracked host file");
        }
        let request = if matches!(id, "F3-R17" | "F3-R32b-unread-index") {
            let mut request: Value =
                serde_json::from_slice(&v5_request(&base, &target)).expect("request JSON");
            request["ingest"]["max_file_bytes"] = json!(96);
            canonical_json(&request).expect("bounded request")
        } else {
            v5_request(&base, &target)
        };
        let (extraction, run, _) = successful_v5_run_with_request(&root, &request, id);
        assert_common_run(&run, nodes, d, id);
        assert_typescript_file_language(
            &extraction,
            if id == "F3-R06" {
                "src/ui/client.ts"
            } else {
                "src/client.ts"
            },
            id,
        );
        let call = extraction["syntax_records"]
            .as_array()
            .expect("syntax records")
            .iter()
            .find(|row| {
                row["record_role"] == "call" && row["payload"]["data"]["binding_key"].is_string()
            })
            .unwrap_or_else(|| panic!("{id}: missing import call record"));
        let actual_reasons = call["reasons"]
            .as_array()
            .expect("call reasons")
            .iter()
            .map(|value| value.as_str().expect("reason string"))
            .collect::<BTreeSet<_>>();
        assert_eq!(
            actual_reasons,
            reasons.iter().copied().collect(),
            "{id}: reason set"
        );
        let actual_candidates = call["payload"]["data"]["candidate_paths"]
            .as_array()
            .expect("candidate paths")
            .iter()
            .map(|value| value.as_str().expect("candidate path"))
            .collect::<BTreeSet<_>>();
        assert_eq!(
            actual_candidates,
            candidates.iter().copied().collect(),
            "{id}: candidate set"
        );
        if id == "F3-R14a" {
            assert_eq!(
                call["primary_reason"], "relative_target_ambiguous",
                "F3-R14(a): all-candidate/rejection-witness scan plus precedence"
            );
            assert_eq!(
                actual_reasons,
                BTreeSet::from(["relative_target_ambiguous", "relative_target_missing"]),
                "F3-R14(a): full reason set, not D=0 alone"
            );
        }
        if id == "F3-R12" {
            assert_eq!(
                call["primary_reason"], "relative_target_unread",
                "F3-R12: the candidate scan preserves the .tsx unread candidate; precedence selects it"
            );
        }
    }
}

#[ignore = "superseded: the common-v5 TypeScript route refuses registry-r1 requests; changed-callee relations are covered by source review v6 (ADR 0053)"]
#[test]
fn typescript_v1_f4_profile_partition_never_converts_unread_files_to_zero() {
    let cases = [
        ("F4-T1-ts", "src/f.ts", "parsed", true, 1usize),
        (
            "F4-T1-tsx",
            "src/f.tsx",
            "non_target_extension",
            false,
            0usize,
        ),
        (
            "F4-T2-vendor",
            "node_modules/pkg/f.ts",
            "profile_excluded",
            false,
            0usize,
        ),
        (
            "F4-T2-generated",
            "dist/f.ts",
            "profile_excluded",
            false,
            0usize,
        ),
        (
            "F4-T2-declaration",
            "src/f.d.ts",
            "profile_excluded",
            false,
            0usize,
        ),
        (
            "F4-T2-test",
            "src/f.test.ts",
            "profile_excluded",
            false,
            0usize,
        ),
        (
            "F4-T2-spec",
            "src/f.spec.ts",
            "profile_excluded",
            false,
            0usize,
        ),
        (
            "F4-T2-test-tsx",
            "src/f.test.tsx",
            "non_target_extension",
            false,
            0usize,
        ),
        (
            "F4-T2-spec-tsx",
            "src/f.spec.tsx",
            "non_target_extension",
            false,
            0usize,
        ),
        (
            "F4-T2-tests-component",
            "__tests__/f.ts",
            "profile_excluded",
            false,
            0usize,
        ),
        (
            "F4-T2-mocks-component",
            "__mocks__/f.ts",
            "profile_excluded",
            false,
            0usize,
        ),
        (
            "F4-T2-benchmark-component",
            "benchmarks/f.ts",
            "profile_excluded",
            false,
            0usize,
        ),
        (
            "F4-T3-precedence",
            "node_modules/__tests__/f.test.ts",
            "profile_excluded",
            false,
            0usize,
        ),
        ("F4-T4-contest", "src/contest.ts", "parsed", true, 1usize),
        (
            "F4-T4-helper",
            "src/test_helpers.ts",
            "parsed",
            true,
            1usize,
        ),
        ("F4-T4-case", "src/Tests/f.ts", "parsed", true, 1usize),
        (
            "F4-T5-upper",
            "src/f.TS",
            "non_target_extension",
            false,
            0usize,
        ),
        (
            "F4-T5-js",
            "src/f.js",
            "non_target_extension",
            false,
            0usize,
        ),
        (
            "F4-T5-mts",
            "src/f.mts",
            "non_target_extension",
            false,
            0usize,
        ),
        (
            "F4-T5-cts",
            "src/f.cts",
            "non_target_extension",
            false,
            0usize,
        ),
        (
            "F4-T5-jsx",
            "src/f.jsx",
            "non_target_extension",
            false,
            0usize,
        ),
        ("F4-T6", "src/test.ts", "parsed", true, 1usize),
        ("F4-T7", "src/framework.ts", "parsed", true, 0usize),
    ];
    for (id, path, outcome, bytes_read, nodes) in cases {
        let source = if id == "F4-T7" {
            "describe('x',()=>1)\n"
        } else {
            "export function f(){return 1}\n"
        };
        let files = tree([(path, source.into())]);
        let (_temporary, root, base, target) = repository(files.clone(), files);
        let (extraction, run, _) = successful_v5_run(&root, &base, &target, id);
        assert_common_run(&run, nodes, 0, id);
        let record = extraction["file_records"]
            .as_array()
            .expect("file records")
            .iter()
            .find(|row| row["path"] == path)
            .expect("fixture file record");
        assert_eq!(record["outcome"], outcome, "{id}: file outcome");
        assert_eq!(record["bytes_read"], bytes_read, "{id}: read accounting");
        if !bytes_read {
            assert!(
                record
                    .get("latent_callable_count")
                    .is_some_and(Value::is_null),
                "{id}: latent count remains unknown"
            );
        }
    }
}

#[ignore = "superseded: the common-v5 TypeScript route refuses registry-r1 requests; changed-callee relations are covered by source review v6 (ADR 0053)"]
#[test]
fn typescript_v1_f6_file_bounds_determinism_and_utf8_bytes_are_accounted() {
    let normal = "export function f(){return 1}\r\n";
    let broken = "export function {\n";
    let non_ascii = "export function caf\u{e9}(){return '\u{1f34a}'}\n";
    let no_final_newline = "export function end(){return 1}";
    let files = tree([
        ("src/normal.ts", normal.into()),
        ("src/broken.ts", "export function f(){return 1}\n".into()),
        ("src/non-ascii.ts", non_ascii.into()),
        ("src/empty.ts", String::new()),
        ("src/no-final-newline.ts", no_final_newline.into()),
    ]);
    let target_files = tree([
        ("src/normal.ts", normal.into()),
        ("src/broken.ts", broken.into()),
        ("src/non-ascii.ts", non_ascii.into()),
        ("src/empty.ts", String::new()),
        ("src/no-final-newline.ts", no_final_newline.into()),
    ]);
    let (_temporary, root, base, target) = repository(files, target_files);
    let (extraction, run, _) = successful_v5_run(&root, &base, &target, "F6-1/F6-13");
    assert_common_run(&run, 3, 0, "F6-1/F6-13");
    for (path, source) in [
        ("src/normal.ts", normal),
        ("src/non-ascii.ts", non_ascii),
        ("src/empty.ts", ""),
        ("src/no-final-newline.ts", no_final_newline),
    ] {
        let file = extraction["file_records"]
            .as_array()
            .expect("F6 file records")
            .iter()
            .find(|file| file["path"] == path)
            .unwrap_or_else(|| panic!("F6-13: missing file record {path}"));
        assert_eq!(file["language"], "typescript", "F6-6: {path} language");
        assert_eq!(file["outcome"], "parsed", "F6-13: {path} outcome");
        assert_eq!(file["bytes_read"], true, "F6-13: {path} bytes read");
        assert_eq!(
            file["source_hash"],
            ContentHash::sha256(source.as_bytes()).to_string(),
            "F6-13: {path} source hash covers exact UTF-8 bytes"
        );
    }
    let broken_file = extraction["file_records"]
        .as_array()
        .expect("F6 file records")
        .iter()
        .find(|file| file["path"] == "src/broken.ts")
        .expect("F6-1 broken file record");
    assert_eq!(broken_file["outcome"], "parse_failed");
    assert_eq!(broken_file["bytes_read"], true);
    assert!(
        broken_file
            .get("latent_callable_count")
            .is_some_and(Value::is_null),
        "F6-1: parse failure leaves latent callable count unknown"
    );

    let bounds_files = tree([
        ("src/a.ts", "export function a(){return 1}\n".into()),
        ("src/b.ts", "export function b(){return 1}\n".into()),
    ]);
    let (_bounds_temp, bounds_root, bounds_base, bounds_target) =
        repository(bounds_files.clone(), bounds_files);
    let mut bounded: Value =
        serde_json::from_slice(&v5_request(&bounds_base, &bounds_target)).expect("F6-2 request");
    bounded["ingest"]["max_files"] = json!(1);
    let bounded = canonical_json(&bounded).expect("F6-2 bounded request");
    let (bounded_extraction, bounded_run, _) =
        successful_v5_run_with_request(&bounds_root, &bounded, "F6-2 max files");
    assert_common_run(&bounded_run, 1, 0, "F6-2 max files");
    let unread = bounded_extraction["file_records"]
        .as_array()
        .expect("F6-2 file records")
        .iter()
        .filter(|file| file["outcome"] == "unread_bound")
        .collect::<Vec<_>>();
    assert_eq!(unread.len(), 1, "F6-2: exactly one source is bound-unread");
    assert_eq!(unread[0]["bytes_read"], false);
    assert!(
        unread[0]
            .get("latent_callable_count")
            .is_some_and(Value::is_null),
        "F6-2: unread denominator remains unknown"
    );

    let file_bound_files = tree([(
        "src/giant.ts",
        format!(
            "export function giant(){{return 1}}\n//{}\n",
            "x".repeat(256)
        ),
    )]);
    let (_file_temp, file_root, file_base, file_target) =
        repository(file_bound_files.clone(), file_bound_files);
    let mut file_bounded: Value = serde_json::from_slice(&v5_request(&file_base, &file_target))
        .expect("F6-2 file-byte request");
    file_bounded["ingest"]["max_file_bytes"] = json!(64);
    let (file_extraction, file_run, _) = successful_v5_run_with_request(
        &file_root,
        &canonical_json(&file_bounded).expect("F6-2 file-byte bounded request"),
        "F6-2 max file bytes",
    );
    assert_common_run(&file_run, 0, 0, "F6-2 max file bytes");
    let giant = file_extraction["file_records"]
        .as_array()
        .expect("F6-2 file records")
        .iter()
        .find(|file| file["path"] == "src/giant.ts")
        .expect("F6-2 giant file record");
    assert_eq!(giant["outcome"], "unread_bound");
    assert_eq!(giant["bytes_read"], false);

    let total_bound_files = tree([
        ("src/a.ts", "export function a(){return 1}\n".into()),
        ("src/b.ts", "export function b(){return 1}\n".into()),
    ]);
    let (_total_temp, total_root, total_base, total_target) =
        repository(total_bound_files.clone(), total_bound_files);
    let mut total_bounded: Value = serde_json::from_slice(&v5_request(&total_base, &total_target))
        .expect("F6-2 total-byte request");
    total_bounded["ingest"]["max_total_source_bytes"] = json!(40);
    let (total_extraction, total_run, _) = successful_v5_run_with_request(
        &total_root,
        &canonical_json(&total_bounded).expect("F6-2 total-byte bounded request"),
        "F6-2 max total bytes",
    );
    assert_common_run(&total_run, 1, 0, "F6-2 max total bytes");
    assert_eq!(
        total_extraction["file_records"]
            .as_array()
            .expect("F6-2 file records")
            .iter()
            .filter(|file| file["outcome"] == "unread_bound")
            .count(),
        1,
        "F6-2: total byte bound records the unread source"
    );

    let deterministic_files = tree([("src/a.ts", "export function a(){return 1}\n".into())]);
    let (_first_temp, first_root, first_base, first_target) =
        repository(deterministic_files.clone(), deterministic_files.clone());
    let (_second_temp, second_root, second_base, second_target) =
        repository(deterministic_files.clone(), deterministic_files);
    let (_first_extraction, first_run, _) =
        successful_v5_run(&first_root, &first_base, &first_target, "F6-3 first");
    let (_second_extraction, second_run, _) =
        successful_v5_run(&second_root, &second_base, &second_target, "F6-3 second");
    assert_eq!(
        canonical_json(&first_run).expect("first deterministic run"),
        canonical_json(&second_run).expect("second deterministic run"),
        "F6-3: equal Git input and tuple yield identical run bytes"
    );
}

#[ignore = "superseded: the common-v5 TypeScript route refuses registry-r1 requests; changed-callee relations are covered by source review v6 (ADR 0053)"]
#[test]
fn typescript_v1_f6_registry_schema_cli_and_accounting_rejections_are_closed() {
    let files = tree([("src/api.ts", "export function f(){return 1}\n".into())]);
    let (_temporary, root, base, target) = repository(files.clone(), files);
    let request = v5_request(&base, &target);
    fs::write(root.join("request.v5.json"), &request).expect("request");
    let validate = Command::new(env!("CARGO_BIN_EXE_reviewgraphen"))
        .args(["schema", "validate", "request.v5.json"])
        .current_dir(&root)
        .output()
        .expect("schema validate");
    assert_eq!(
        validate.status.code(),
        Some(0),
        "F6-9: valid TS tuple must be accepted: {}",
        String::from_utf8_lossy(&validate.stdout)
    );

    let listed = Command::new(env!("CARGO_BIN_EXE_reviewgraphen"))
        .args(["schema", "list"])
        .current_dir(&root)
        .output()
        .expect("schema list");
    assert_eq!(listed.status.code(), Some(0));
    let listed: Value = serde_json::from_slice(&listed.stdout).expect("schema list JSON");
    for family in [
        "reviewgraphen.review_profile.v2",
        "reviewgraphen.extraction_report.v2",
        "reviewgraphen.ingestion_report.v3",
        "reviewgraphen.generic_review_request.v5",
        "reviewgraphen.generic_review_run.v5",
        "reviewgraphen.generic_review_human_report.v4",
    ] {
        assert!(
            listed
                .as_array()
                .expect("schema IDs")
                .iter()
                .any(|id| id == family),
            "F6-11: missing {family}"
        );
    }
    for (schema_path, example_path) in SCHEMA_PAIRS {
        let name = serde_json::from_slice::<Value>(
            &fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(example_path))
                .expect("schema example"),
        )
        .expect("schema example JSON")["schema"]
            .as_str()
            .expect("schema name")
            .to_owned();
        let printed = Command::new(env!("CARGO_BIN_EXE_reviewgraphen"))
            .args(["schema", "print", &name])
            .current_dir(&root)
            .output()
            .expect("schema print");
        assert_eq!(printed.status.code(), Some(0), "F6-11: print {name}");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join(example_path),
            root.join(format!("{name}.example.json")),
        )
        .expect("copy schema example");
        let validate_example = Command::new(env!("CARGO_BIN_EXE_reviewgraphen"))
            .args(["schema", "validate", &format!("{name}.example.json")])
            .current_dir(&root)
            .output()
            .expect("schema example validation");
        assert_eq!(
            validate_example.status.code(),
            Some(0),
            "F6-11: valid {name} example"
        );
        assert!(schema_path.ends_with(".schema.json"));
    }

    let original: Value = serde_json::from_slice(&request).expect("request JSON");
    let mut mutations = Vec::<(&str, Value)>::new();
    for field in [
        "profile_id",
        "profile_version",
        "language",
        "producer_id",
        "extractor_set_hash",
        "rule_set_hash",
    ] {
        let mut mutation = original.clone();
        mutation["ingest"][field] = json!(format!("forged-{field}"));
        mutations.push((field, mutation));
    }
    for (name, path, value) in [
        ("projection_id", "projection_id", json!("forged-projection")),
        (
            "foreign_registry",
            "registry_id",
            json!("foreign.registry.r1"),
        ),
        (
            "registry_hash_tamper",
            "registry_hash",
            json!("sha256:04bcf9dbf1b0795f7849f44a28ff01b6233698579940b64e57d8aea54c0f12ab"),
        ),
        ("short_oid", "base_revision", json!(&base[..12])),
        ("symbolic_ref", "target_revision", json!("main")),
    ] {
        let mut mutation = original.clone();
        mutation[path] = value;
        mutations.push((name, mutation));
    }
    for (name, mutation) in mutations {
        fs::write(
            root.join(format!("mutation-{name}.json")),
            canonical_json(&mutation).expect("mutation bytes"),
        )
        .expect("mutation");
        let file = format!("mutation-{name}.json");
        let schema = Command::new(env!("CARGO_BIN_EXE_reviewgraphen"))
            .args(["schema", "validate", &file])
            .current_dir(&root)
            .output()
            .expect("schema mutation");
        assert_ne!(
            schema.status.code(),
            Some(0),
            "F6-9/X3: schema accepted {name}"
        );
        let review = Command::new(env!("CARGO_BIN_EXE_reviewgraphen"))
            .args([
                "review",
                "--request",
                &file,
                "--artifacts",
                &format!("artifact-{name}"),
            ])
            .current_dir(&root)
            .output()
            .expect("review mutation");
        assert_ne!(
            review.status.code(),
            Some(0),
            "F6-9/X3: review accepted {name}"
        );
    }

    let (_extraction, run, artifacts) = successful_v5_run(&root, &base, &target, "F6-11/F6-14");
    assert_common_run(&run, 1, 0, "F6-14");
    let files = fs::read_dir(&artifacts)
        .expect("artifact directory")
        .map(|entry| {
            entry
                .expect("artifact entry")
                .file_name()
                .into_string()
                .expect("UTF-8 artifact name")
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        files,
        BTreeSet::from([
            "artifact-manifest.v1.json".to_owned(),
            "audit.run.v5.json".to_owned(),
            "extraction-report.v2.json".to_owned(),
            "human-report.manifest.v4.json".to_owned(),
            "human-report.md".to_owned(),
            "ingestion-report.v3.json".to_owned(),
        ]),
        "F6-11: exactly six public artifacts"
    );
    let human: Value = serde_json::from_slice(
        &fs::read(artifacts.join("human-report.manifest.v4.json")).expect("human manifest"),
    )
    .expect("human manifest JSON");
    assert_eq!(human["registry_binding"]["registry_hash"], REGISTRY_HASH);
    assert_eq!(human["run_id"], run["run_id"]);

    let markdown = fs::read_to_string(artifacts.join("human-report.md")).expect("human report");
    for literal in [
        "Read files: 1",
        "Node obligations: 1",
        "D obligations: 0",
        "Gap obligations: 1",
        "unresolved or ineligible reasons",
        "trusted_pass: false",
    ] {
        assert!(
            markdown.contains(literal),
            "F6-14: human accounting missing literal {literal}"
        );
    }

    let mut tampered_extraction = serde_json::from_slice::<Value>(
        &fs::read(artifacts.join("extraction-report.v2.json")).expect("extraction artifact"),
    )
    .expect("extraction JSON");
    tampered_extraction["file_records"][0]["language"] = json!("typescriptx");
    fs::write(
        root.join("language-tamper.json"),
        canonical_json(&tampered_extraction).expect("language tamper bytes"),
    )
    .expect("language tamper");
    let rejected_language = Command::new(env!("CARGO_BIN_EXE_reviewgraphen"))
        .args(["schema", "validate", "language-tamper.json"])
        .current_dir(&root)
        .output()
        .expect("language tamper validation");
    assert_ne!(
        rejected_language.status.code(),
        Some(0),
        "F6-6/F6-7: schema validation accepted forged TS file language"
    );

    let mut missing_file = tampered_extraction.clone();
    missing_file["file_records"]
        .as_array_mut()
        .expect("file rows")
        .clear();
    fs::write(
        root.join("missing-file-row.json"),
        canonical_json(&missing_file).expect("missing file bytes"),
    )
    .expect("missing file row");
    let rejected_closure = Command::new(env!("CARGO_BIN_EXE_reviewgraphen"))
        .args(["schema", "validate", "missing-file-row.json"])
        .current_dir(&root)
        .output()
        .expect("closure validation");
    assert_ne!(
        rejected_closure.status.code(),
        Some(0),
        "F6-8: schema validation accepted a ledger difference"
    );
    assert!(
        format!(
            "{}{}",
            String::from_utf8_lossy(&rejected_closure.stdout),
            String::from_utf8_lossy(&rejected_closure.stderr)
        )
        .contains("accounting_mismatch"),
        "F6-8/F6-14: nonzero ledger difference names accounting_mismatch"
    );
}
