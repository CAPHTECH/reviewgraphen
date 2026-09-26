//! T1 SUPPLEMENT-3b acceptance (Acc-T1): the U5 mutation survivor (unwind
//! removal errors reported as `Removed`) and U5b (`unwind incomplete` suffix
//! dropped). Frozen against F `af31e3cf`. The fixtures are a verbatim copy
//! of `t1_toctou_acceptance.rs` lines 86–414 at `af31e3cf`; every earlier
//! acceptance file stays byte-identical.
//!
//! Seam interface `[R]`: two more functions in the existing `crate::t1s3_seam`
//! (`#[cfg(test)]` only, thread-local, identity when unset, guard unsets):
//!
//! ```ignore
//! /// While held, during unwind the `unlinkat` of the tracked entry whose
//! /// root-relative label (the `UnwindRecord::entry` label: `name`,
//! /// `records/<name>`, `records`, or `""` for the root) equals `entry`
//! /// returns `Err(errno)` instead of unlinking. It fires only after that
//! /// entry's identity check matched (i.e. where a real `unlinkat` would run).
//! pub(crate) fn install_unwind_unlink_fault(entry: &'static str, errno: i32) -> impl Drop;
//! /// Thread-local running count of unlink faults actually injected.
//! pub(crate) fn unwind_unlink_faults_injected() -> usize;
//! ```
//!
//! Expected: an
//! unlink error other than ENOTEMPTY/EEXIST is `UnwindOutcome::Failed(errno)`,
//! rendered on stderr as `<reason>; unwind incomplete: "<entry>" (errno <n>)`.
//! `message()` lists ONLY `Failed` records, so that suffix naming exactly the
//! faulted entry is the observable form of the typed record (no test hook
//! exposes `UnwindRecord`s directly [R]).
#![allow(dead_code)]

use crate::g7_r1_sup1_acceptance::cwd_lock;
use crate::run;
use crate::t1s2_seam::{Point, install_hook};
use crate::t1s3_seam::{install_unwind_unlink_fault, unwind_unlink_faults_injected};
use reviewgraphen_core::ContentHash;
use serde_json::json;
use std::{
    collections::BTreeMap,
    env, fs,
    io::Write as _,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use tempfile::TempDir;

const FOREIGN_MANIFEST: &[u8] = b"foreign manifest that must survive\n";

// ------------------------------------------------------------ fixtures

#[derive(Clone, Copy, Debug)]
enum Route {
    V2,
    V3,
    V4,
    TypeScriptV5,
}

struct Fixture {
    _repository: TempDir,
    _request_dir: TempDir,
    root: PathBuf,
    request: PathBuf,
}

fn git_in(root: &Path, args: &[&str], input: &[u8]) -> String {
    let mut child = Command::new("git")
        .current_dir(root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Acc-S06")
        .env("GIT_COMMITTER_NAME", "Acc-S06")
        .env("GIT_AUTHOR_EMAIL", "acc-s06@example.invalid")
        .env("GIT_COMMITTER_EMAIL", "acc-s06@example.invalid")
        .env("GIT_AUTHOR_DATE", "1700000000 +0000")
        .env("GIT_COMMITTER_DATE", "1700000000 +0000")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("git spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .expect("git input");
    let out = child.wait_with_output().expect("git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .expect("git utf-8")
        .trim()
        .to_owned()
}

/// v2–v4: the SUP1 Rust fixture (common_v5_publication_root_acceptance.rs
/// `rust_repo`/`rust_request`, fixed dates so revisions are stable).
fn rust_fixture(version: u8) -> (TempDir, Vec<u8>) {
    let dir = TempDir::new().expect("rust repo");
    let root = dir.path();
    git_in(root, &["init", "-q"], b"");
    fs::write(root.join("lib.rs"), "pub fn value() -> u64 { 1 }\n").expect("base");
    git_in(root, &["add", "lib.rs"], b"");
    git_in(root, &["commit", "-q", "-m", "base"], b"");
    let base = git_in(root, &["rev-parse", "HEAD"], b"");
    fs::write(root.join("lib.rs"), "pub fn value() -> u64 { 2 }\n").expect("target");
    git_in(root, &["add", "lib.rs"], b"");
    git_in(root, &["commit", "-q", "-m", "target"], b"");
    let target = git_in(root, &["rev-parse", "HEAD"], b"");
    let mut value = json!({
        "schema": format!("reviewgraphen.generic_review_request.v{version}"),
        "workspace_admission_root": ".",
        "repository_admission_root": ".",
        "repository_identity": "acc-t1-root-fixture@1",
        "base_revision": base,
        "target_revision": target,
        "ingest": {
            "profile_id": "rust.production.v1",
            "profile_version": "1",
            "rule_set_hash": "sha256:8f6bfbfb2dbf2f0eaf916b152ddba1e422db8b6de95f931c0c78c9ee4d050b47",
            "max_files": 32,
            "max_file_bytes": 1048576,
            "max_total_source_bytes": 1048576
        },
        "plan": {"max_waves": 8, "max_obligations_per_wave": 32},
        "observer": {"kind": "deterministic_abstain"},
        "verifier_descriptor_id": null
    });
    if version == 3 {
        value["context_policy_id"] = json!("context.subject_windows@3");
    } else if version == 4 {
        value["context_policies"] = json!({
            "relation.changed_public_callee@1": {
                "property_id": "rust.callee_contract_review@1",
                "target_kind": "relation",
                "policy_id": "context.subject_windows@3",
                "policy_hash": "sha256:932bfa18c5d286c63196366d6d2dc1aaf402f50baa1f1ab5f075b1007be55dd8"
            },
            "node.public_function_contract@1": {
                "property_id": "rust.public_function_contract_review@1",
                "target_kind": "node",
                "policy_id": "context.subject_windows@4",
                "policy_hash": "sha256:9f15006986a73853ff3be7b9a158e8c79f69b9a8099c9d0a1991d8c7da170aed"
            }
        });
    }
    (
        dir,
        reviewgraphen_core::canonical_json(&value).expect("canonical request"),
    )
}

use crate::t1_fixtures::typescript_fixture;

fn fixture(route: Route) -> Fixture {
    let (repository, request_bytes) = match route {
        Route::V2 => rust_fixture(2),
        Route::V3 => rust_fixture(3),
        Route::V4 => rust_fixture(4),
        Route::TypeScriptV5 => typescript_fixture(),
    };
    let request_dir = TempDir::new().expect("request dir");
    let request = request_dir.path().join("request.json");
    fs::write(&request, request_bytes).expect("request bytes");
    let root = repository.path().to_path_buf();
    Fixture {
        _repository: repository,
        _request_dir: request_dir,
        root,
        request,
    }
}

/// In-process `run` (so the thread-local seam applies) from the invocation
/// root; the request lives outside it.
fn review(fixture: &Fixture, artifacts: &str) -> crate::CommandOutcome {
    let previous = env::current_dir().expect("cwd");
    env::set_current_dir(&fixture.root).expect("enter invocation root");
    let outcome = run(vec![
        "review".to_owned(),
        "--request".to_owned(),
        fixture.request.display().to_string(),
        "--artifacts".to_owned(),
        artifacts.to_owned(),
    ]);
    env::set_current_dir(previous).expect("restore cwd");
    outcome
}

// ------------------------------------------------------------ goldens

/// TypeScript v5 whole files for the synthetic T1 fixture.
#[rustfmt::skip]
const TS_GOLDEN: &[(&str, &str)] = &[
    ("artifact-manifest.v1.json", "09959e3d720d5c1d025ea5051429260edb5203343e7c77c12fb6eb4aa610450f"),
    ("audit.run.v5.json", "eebbdf6dfffe47ce82c0017b73e3d6065a8201c0ca154f373bfcecda2dd406ad"),
    ("extraction-report.v2.json", "590409e16e27f805700cec5b2964d584b37d35cd8405f5606f88f422c8b8f11c"),
    ("human-report.manifest.v4.json", "c9bba96546efc2bacfc6262f3d36717ca7e8a869f2898894a33f51be8848cbf6"),
    ("human-report.md", "22559b5da422b9a4867ae36a7bfde576433fa8c8fb5902d23dbe2737e415069d"),
    ("ingestion-report.v3.json", "dd06577bbed4b7734847e13b92c7cb6f0dd334d2248c2e1062a7a78e12045426"),
];
/// v2–v4 on this fixture, frozen by Acc-T1 from F `31eba48a` (`nested/out`).
#[rustfmt::skip]
const V2_GOLDEN: &[(&str, &str)] = &[
    ("artifact-manifest.v1.json", "19153475abe93d64c107ee76b1a9c10cddffa63afa4b52d6c4998c7fbe7fd910"),
    ("audit.run.v2.json", "db2104d49f0f474422713cdf1c6c01672c145f6a28ec99cd46b59412a2f7446b"),
    ("human-report.manifest.v1.json", "169d23c5200d0ad2882b555c25f180f78877b3cab220684a38c9c7f0d3e4d884"),
    ("human-report.md", "b690e34c01909103b791b4e3e60772394eb5411ca0f911231e07e2e206746178"),
];
#[rustfmt::skip]
const V3_GOLDEN: &[(&str, &str)] = &[
    ("artifact-manifest.v1.json", "70a42c22f38282f785921c965138637766dc72eae660b6dcfb3e5101a5d49101"),
    ("audit.run.v3.json", "1cc2f18808c95cda59934ea0eea6b84f257076fe3f1748376f1a985f7fca8727"),
    ("human-report.manifest.v2.json", "6e39909ef167be1239bc29f5e1376deb8ab71c798dbd045896e4db886829c702"),
    ("human-report.md", "e0df761562571ba03d4da366824b9434945ca338ed02dc84d9ba42c8cfa508c9"),
];
#[rustfmt::skip]
const V4_GOLDEN: &[(&str, &str)] = &[
    ("artifact-manifest.v1.json", "ca5987bd5cad7c10614eed4c01711788efec11e53974019dd63f6dfbd674c771"),
    ("audit.run.v4.json", "94787e147de1dd01460d00d85165ccdbea14e22017cecf2b936652e019e15733"),
    ("human-report.manifest.v3.json", "0bdbe20549563b3190bde5000d40722af5c68a9d178eaa63b130724b1ada1f62"),
    ("human-report.md", "8f6633eb41ca5d026d0e4d44b741ff2263aa4e229b5edb2cdaf43a2abbfed382"),
];

fn golden(route: Route) -> &'static [(&'static str, &'static str)] {
    match route {
        Route::V2 => V2_GOLDEN,
        Route::V3 => V3_GOLDEN,
        Route::V4 => V4_GOLDEN,
        Route::TypeScriptV5 => TS_GOLDEN,
    }
}

fn file_hashes(root: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for item in fs::read_dir(root).expect("read root") {
        let path = item.expect("entry").path();
        let metadata = fs::symlink_metadata(&path).expect("metadata");
        assert!(
            metadata.is_file(),
            "only files expected in the root: {path:?}"
        );
        out.insert(
            path.file_name()
                .expect("name")
                .to_string_lossy()
                .into_owned(),
            ContentHash::sha256(&fs::read(&path).expect("bytes")).to_string(),
        );
    }
    out
}

/// A planted foreign manifest makes the last write fail (as SUP1 Q10), so
/// unwind runs; the unlink of `human-report.md` then fails with EACCES.
fn unwind_unlink_error_is_failed(route: Route) {
    let _cwd = cwd_lock();
    let fixture = fixture(route);
    fs::create_dir(fixture.root.join("nested")).expect("nested");
    let eacces = rustix::io::Errno::ACCESS.raw_os_error();
    let injected_before = unwind_unlink_faults_injected();
    let outcome = {
        let _fault = install_unwind_unlink_fault("human-report.md", eacces);
        let _plant = install_hook(Box::new(move |point, path: &Path| {
            if point == (Point::BetweenFiles { written: 0 }) {
                fs::write(path.join("artifact-manifest.v1.json"), FOREIGN_MANIFEST)
                    .expect("foreign manifest");
            }
        }));
        review(&fixture, "nested/out")
    };
    assert_eq!(
        unwind_unlink_faults_injected() - injected_before,
        1,
        "{route:?}: the unlink fault was injected once (escape check); stderr {:?}",
        outcome.stderr
    );
    assert_eq!(outcome.exit_code, 20, "{route:?}: {}", outcome.stderr);
    assert!(outcome.stdout.is_empty(), "{route:?}: no audit on stdout");
    let suffix = format!("; unwind incomplete: \"human-report.md\" (errno {eacces})");
    assert!(
        outcome
            .stderr
            .starts_with("unable to write generic review artifact")
            && outcome.stderr.ends_with(&suffix),
        "{route:?}: stderr must end with the typed Failed record `{suffix}` and name no \
         other entry, got {:?}",
        outcome.stderr
    );
    let left = file_hashes(&fixture.root.join("nested/out"));
    assert!(
        left.contains_key("human-report.md"),
        "{route:?}: the entry whose unlink failed remains: {left:?}"
    );
    assert_eq!(
        left.get("artifact-manifest.v1.json"),
        Some(&ContentHash::sha256(FOREIGN_MANIFEST).to_string()),
        "{route:?}: the foreign manifest is untouched"
    );
    assert_eq!(
        left.len(),
        2,
        "{route:?}: every other tracked file was removed: {left:?}"
    );
}

#[test]
fn t1s3b_v2_unwind_unlink_error_is_reported_failed() {
    unwind_unlink_error_is_failed(Route::V2);
}
