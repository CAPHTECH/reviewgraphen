//! G7-R1 acceptance O2 (in-tree analogue) and O4 (Acc-R1, frozen before
//! production), scopes 1 and 4.
//!
//! * O2: a Rust v4 repository with one `include!`d function-body fragment
//!   (not a standalone Rust file, like a `*_body.rs` fragment) makes
//!   `ast`/`containment` Partial. The route must exit 0 with the four
//!   artifacts and a gap row `["ast","containment","direct_calls"]`
//!   (today: exit 20 "invalid run v4 schema" and an empty artifact root).
//! * O4: v2, v3 and v4 routes leave nothing behind after a failure that
//!   happens after artifact-root admission today:
//!   - v4 report-stage refusal (the O2 fixture on binary 2acc5961);
//!   - write-stage failure on every route, injected without a product seam by
//!     running the real binary under `RLIMIT_FSIZE` = 4096 bytes with
//!     `SIGXFSZ` ignored, so the first artifact write returns `EFBIG`.
//!     The control run proves every route's audit artifact is > 4096 bytes,
//!     so the limit can only bite inside `write_artifacts`.
//!
//! All runs use the real `reviewgraphen` binary as a subprocess with
//! cwd = repository root (the product contract), so no cwd lock is needed.

use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_reviewgraphen");
const RULE_SET_HASH: &str =
    "sha256:8f6bfbfb2dbf2f0eaf916b152ddba1e422db8b6de95f931c0c78c9ee4d050b47";
const FSIZE_LIMIT_BYTES: u64 = 4096;
const FOUR_V4: [&str; 4] = [
    "artifact-manifest.v1.json",
    "audit.run.v4.json",
    "human-report.manifest.v3.json",
    "human-report.md",
];

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "ReviewGraphen Test")
        .env("GIT_AUTHOR_EMAIL", "reviewgraphen@example.invalid")
        .env("GIT_COMMITTER_NAME", "ReviewGraphen Test")
        .env("GIT_COMMITTER_EMAIL", "reviewgraphen@example.invalid")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git is available");
    assert!(output.status.success(), "git failed: {arguments:?}");
    String::from_utf8(output.stdout)
        .expect("git stdout UTF-8")
        .trim()
        .to_owned()
}

struct Fixture {
    _temporary: TempDir,
    repository: PathBuf,
    base: String,
    target: String,
}

const CARGO_TOML: &str = "[package]\nname = \"g7r1fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"lib.rs\"\n";

fn lib_source(callee_value: u64, with_fragment: bool) -> String {
    let mut source = format!(
        "pub fn callee() -> u64 {{ {callee_value} }}\npub fn caller() -> u64 {{ callee() }}\n"
    );
    if with_fragment {
        source.push_str("pub fn fragment_user() -> u64 { include!(\"frag_body.rs\") }\n");
    }
    source
}

/// Two commits: base -> target, the target changes `callee`'s body.
fn fixture(with_fragment: bool) -> Fixture {
    let temporary = TempDir::new().expect("temporary directory");
    let repository = temporary.path().join("repository");
    fs::create_dir(&repository).expect("repository directory");
    git(&repository, &["init", "-q"]);
    fs::write(repository.join("Cargo.toml"), CARGO_TOML).expect("Cargo.toml");
    fs::write(repository.join("lib.rs"), lib_source(1, with_fragment)).expect("base lib.rs");
    if with_fragment {
        // A statement list plus a tail expression: valid only when included
        // into a block, `syn::parse_file` fails on it as a standalone file.
        fs::write(
            repository.join("frag_body.rs"),
            "let value = 1;\nvalue + 1\n",
        )
        .expect("fragment");
    }
    git(&repository, &["add", "-A"]);
    git(&repository, &["commit", "-q", "-m", "base"]);
    let base = git(&repository, &["rev-parse", "HEAD"]);
    fs::write(repository.join("lib.rs"), lib_source(2, with_fragment)).expect("target lib.rs");
    git(&repository, &["add", "-A"]);
    git(&repository, &["commit", "-q", "-m", "target"]);
    let target = git(&repository, &["rev-parse", "HEAD"]);
    Fixture {
        _temporary: temporary,
        repository,
        base,
        target,
    }
}

fn request(version: u8, fixture: &Fixture) -> Vec<u8> {
    let value = match version {
        4 => {
            let mut request: Value = serde_json::from_slice(include_bytes!(
                "../../../schemas/reviewgraphen.generic_review_request.v4.example.json"
            ))
            .expect("request v4 example");
            request["workspace_admission_root"] = json!(".");
            request["repository_admission_root"] = json!(".");
            request["repository_identity"] = json!("g7-r1/acceptance@1");
            request["base_revision"] = json!(fixture.base);
            request["target_revision"] = json!(fixture.target);
            request["ingest"]["max_files"] = json!(64);
            request["ingest"]["max_file_bytes"] = json!(1_048_576);
            request["ingest"]["max_total_source_bytes"] = json!(1_048_576);
            request["verifier_descriptor_id"] = Value::Null;
            request
        }
        2 | 3 => {
            let mut request = json!({
                "schema": "reviewgraphen.generic_review_request.v2",
                "workspace_admission_root": ".",
                "repository_admission_root": ".",
                "repository_identity": "g7-r1/acceptance@1",
                "base_revision": fixture.base,
                "target_revision": fixture.target,
                "ingest": {
                    "profile_id": "rust.production.v1",
                    "profile_version": "1",
                    "rule_set_hash": RULE_SET_HASH,
                    "max_files": 32,
                    "max_file_bytes": 1048576,
                    "max_total_source_bytes": 1048576
                },
                "plan": {"max_waves": 8, "max_obligations_per_wave": 32},
                "observer": {"kind": "deterministic_abstain"},
                "verifier_descriptor_id": null
            });
            if version == 3 {
                request["schema"] = json!("reviewgraphen.generic_review_request.v3");
                request["context_policy_id"] = json!("context.subject_windows@3");
            }
            request
        }
        _ => unreachable!("routes under test are v2, v3, v4"),
    };
    reviewgraphen_core::canonical_json(&value).expect("canonical request")
}

/// Writes the request OUTSIDE the repository and runs the real binary from the
/// repository root, optionally under `RLIMIT_FSIZE` with `SIGXFSZ` ignored.
fn review(fixture: &Fixture, version: u8, artifacts: &str, fsize_limit: Option<u64>) -> Output {
    let request_path = fixture
        .repository
        .parent()
        .expect("temporary parent")
        .join(format!("request.v{version}.{artifacts}.json"));
    fs::write(&request_path, request(version, fixture)).expect("request file");
    let arguments = [
        "review",
        "--request",
        request_path.to_str().expect("UTF-8 request path"),
        "--artifacts",
        artifacts,
    ];
    let mut command = match fsize_limit {
        None => {
            let mut command = Command::new(BIN);
            command.args(arguments);
            command
        }
        Some(bytes) => {
            // POSIX sh `ulimit -f` counts 512-byte blocks. An ignored signal
            // stays ignored across exec, so write(2) returns EFBIG instead of
            // the process being killed.
            let mut command = Command::new("/bin/sh");
            command
                .arg("-c")
                .arg(format!(
                    "trap '' XFSZ; ulimit -f {}; exec \"$0\" \"$@\"",
                    bytes / 512
                ))
                .arg(BIN)
                .args(arguments);
            command
        }
    };
    command
        .current_dir(&fixture.repository)
        .output()
        .expect("run reviewgraphen")
}

fn listing(root: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(&directory).expect("read directory") {
            let entry = entry.expect("directory entry");
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .expect("relative")
                .display()
                .to_string();
            if relative == ".git" {
                continue;
            }
            if entry.file_type().expect("file type").is_dir() {
                stack.push(path.clone());
            }
            names.insert(relative);
        }
    }
    names
}

fn artifact_names(root: &Path) -> Vec<String> {
    let mut names = fs::read_dir(root)
        .expect("artifact root")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn validate(path: &Path) -> Output {
    Command::new(BIN)
        .args(["schema", "validate", path.to_str().expect("UTF-8 path")])
        .output()
        .expect("schema validate")
}

/// O4 invariant used by every failing run below: non-zero exit and the
/// artifact root does not exist, and nothing else appeared in the repository.
fn assert_failed_without_leftovers(
    output: &Output,
    fixture: &Fixture,
    before: &BTreeSet<String>,
    artifacts: &str,
    name: &str,
) {
    assert_ne!(output.status.code(), Some(0), "{name}: must fail");
    let root = fixture.repository.join(artifacts);
    assert!(
        fs::symlink_metadata(&root).is_err(),
        "{name}: artifact root must not exist after failure; found {:?}; stderr={}",
        fs::read_dir(&root)
            .map(|entries| entries
                .map(|e| e.map(|e| e.file_name()))
                .collect::<Vec<_>>())
            .ok(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        &listing(&fixture.repository),
        before,
        "{name}: repository tree unchanged"
    );
}

// ------------------------------------------------------------------- O2

#[test]
fn o2_v4_include_fragment_repository_exits_0_with_four_valid_artifacts() {
    let fixture = fixture(true);
    let before = listing(&fixture.repository);
    let output = review(&fixture, 4, ".g7-r1-out", None);
    let root = fixture.repository.join(".g7-r1-out");
    if output.status.code() != Some(0) {
        // O4 applies to this failure too (today: exit 20 + empty root).
        assert_failed_without_leftovers(
            &output,
            &fixture,
            &before,
            ".g7-r1-out",
            "v4 include fragment",
        );
        panic!(
            "v4 include-fragment run must exit 0; exit={:?} stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(
        artifact_names(&root),
        FOUR_V4,
        "exactly the four v4 artifacts"
    );
    let audit_bytes = fs::read(root.join("audit.run.v4.json")).expect("audit");
    assert_eq!(output.stdout, audit_bytes, "stdout == audit bytes");
    for name in [
        "audit.run.v4.json",
        "artifact-manifest.v1.json",
        "human-report.manifest.v3.json",
    ] {
        let verdict = validate(&root.join(name));
        assert_eq!(
            verdict.status.code(),
            Some(0),
            "schema validate {name}: {}",
            String::from_utf8_lossy(&verdict.stdout)
        );
    }
    let audit: Value = serde_json::from_slice(&audit_bytes).expect("audit JSON");
    let contract = audit["obligation_contract"].as_array().expect("contract");
    let gaps = contract
        .iter()
        .filter(|row| row["rule_id"] == "capability_gap.origin_rule@1")
        .collect::<Vec<_>>();
    assert_eq!(gaps.len(), 1, "one D candidate-space gap row");
    assert_eq!(
        gaps[0]["target_support_capabilities"],
        json!(["ast", "containment", "direct_calls"])
    );
    assert_eq!(gaps[0]["enumeration_capabilities"], json!([]));
    let nodes = contract
        .iter()
        .filter(|row| row["rule_id"] == "node.public_function_contract@1")
        .collect::<Vec<_>>();
    assert!(!nodes.is_empty(), "Node rows are present");
    assert!(
        nodes
            .iter()
            .all(|row| row["applicability_status"] == "unknown"),
        "partial ast/containment keeps every Node row unknown (ADR 0040:65)"
    );
    assert_eq!(audit["authority"]["trusted_pass"], json!(false));
}

/// Control for O2: the same repository without the fragment passes on
/// 2acc5961 as well, with the historical `["direct_calls"]` gap row.
#[test]
fn o2_control_v4_fully_parsable_repository_keeps_direct_calls_only_gap() {
    let fixture = fixture(false);
    let output = review(&fixture, 4, ".g7-r1-out", None);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let audit: Value = serde_json::from_slice(&output.stdout).expect("audit JSON");
    let gap = audit["obligation_contract"]
        .as_array()
        .expect("contract")
        .iter()
        .find(|row| row["rule_id"] == "capability_gap.origin_rule@1")
        .expect("gap row")
        .clone();
    assert_eq!(gap["target_support_capabilities"], json!(["direct_calls"]));
    assert_eq!(
        artifact_names(&fixture.repository.join(".g7-r1-out")),
        FOUR_V4
    );
}

// ------------------------------------------------------------------- O4

fn write_failure_leaves_nothing(version: u8) {
    let fixture = fixture(false);
    // Control: same request, no limit -> success, and the audit artifact is
    // larger than the limit, so the injected fault must fire inside the write.
    let control = review(&fixture, version, ".g7-r1-control", None);
    assert_eq!(
        control.status.code(),
        Some(0),
        "v{version} control: {}",
        String::from_utf8_lossy(&control.stderr)
    );
    let audit_name = format!("audit.run.v{version}.json");
    let audit_len = fs::metadata(fixture.repository.join(".g7-r1-control").join(&audit_name))
        .expect("control audit")
        .len();
    assert!(
        audit_len > FSIZE_LIMIT_BYTES,
        "v{version} control audit {audit_len} B must exceed the {FSIZE_LIMIT_BYTES} B limit"
    );
    let before = listing(&fixture.repository);

    let output = review(&fixture, version, ".g7-r1-faulted", Some(FSIZE_LIMIT_BYTES));
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Escape check [R]: the refusal is the artifact-write refusal, not an
    // earlier stage (message family unchanged since 2acc5961).
    assert!(
        stderr.contains("unable to write generic review"),
        "v{version}: write-stage failure expected, got: {stderr}"
    );
    assert!(
        output.stdout.is_empty(),
        "v{version}: no audit on stdout after a failed write"
    );
    assert_failed_without_leftovers(
        &output,
        &fixture,
        &before,
        ".g7-r1-faulted",
        &format!("v{version} write failure"),
    );
}

#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "RLIMIT_FSIZE write-fault injection is Linux-specific (macOS may raise SIGXFSZ)"
)]
fn o4_v2_write_failure_leaves_no_artifact_root() {
    write_failure_leaves_nothing(2);
}

#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "RLIMIT_FSIZE write-fault injection is Linux-specific (macOS may raise SIGXFSZ)"
)]
fn o4_v3_write_failure_leaves_no_artifact_root() {
    write_failure_leaves_nothing(3);
}

#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "RLIMIT_FSIZE write-fault injection is Linux-specific (macOS may raise SIGXFSZ)"
)]
fn o4_v4_write_failure_leaves_no_artifact_root() {
    write_failure_leaves_nothing(4);
}

/// Pre-admission refusals already leave nothing (control for the invariant
/// helper; green on 2acc5961 and after the fix).
#[test]
fn o4_control_existing_artifact_root_is_refused_without_mutation() {
    let fixture = fixture(false);
    fs::create_dir(fixture.repository.join(".g7-r1-existing")).expect("pre-existing root");
    let before = listing(&fixture.repository);
    for version in [2, 3, 4] {
        let output = review(&fixture, version, ".g7-r1-existing", None);
        assert_eq!(output.status.code(), Some(20), "v{version}");
        assert_eq!(
            listing(&fixture.repository),
            before,
            "v{version}: nothing written"
        );
    }
}
