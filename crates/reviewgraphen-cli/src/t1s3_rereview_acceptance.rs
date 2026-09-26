//! T1 SUPPLEMENT-3 acceptance (Acc-T1): the T1 re-review findings
//! R3 / R4 / R5, frozen against
//! `bb7df8b9`. The fixtures and goldens are a verbatim copy of
//! `t1_toctou_acceptance.rs` lines 86–414 at `bb7df8b9`, so the frozen T1,
//! T1-R2 and T1-S2 files stay byte-identical.
//!
//! Rulings under test:
//! - R3: the product NEVER calls `umask()` to change the mask. On Linux it
//!   reads `/proc/self/status` `Umask:`; when that is unavailable it skips
//!   ONLY the mode comparison of the fresh-directory check (uid and emptiness
//!   still apply) and records that the mode check was skipped.
//! - R5: during unwind only `ENOENT` means "not present"; any other identity
//!   `statat` error is `UnwindOutcome::Failed(errno)`, so stderr carries
//!   `unwind incomplete` with that entry and errno (existing
//!   `ArtifactFailure::message` format: `<reason>; unwind incomplete:
//!   "<entry>" (errno <n>)`).
//! - R4: `t1s2_seam::Point::AfterReopen` fires only after a SUCCESSFUL open.
//!
//! Seam interface `[R]` (a NEW module; `#[cfg(test)]` only, absent from
//! non-test builds, thread-local, identity when unset, guards unset on drop):
//!
//! ```ignore
//! #[cfg(test)]
//! pub(crate) mod t1s3_seam {
//!     /// R3: while held, the product's umask query takes its "unavailable"
//!     /// path (as if `/proc/self/status` had no parseable `Umask:` line), on
//!     /// every platform.
//!     pub(crate) fn install_umask_unavailable() -> impl Drop;
//!     /// R3: thread-local running count of fresh-directory verifications (root
//!     /// and `records/`) that skipped the mode comparison because the umask
//!     /// was unavailable. Never reset; tests compare before/after.
//!     pub(crate) fn mode_check_skips() -> usize;
//!     /// R5: while held, during unwind the identity `statat` of the tracked
//!     /// FILE whose root-relative label (`name` or `records/<name>`, the same
//!     /// label `UnwindRecord::entry` carries) equals `entry` returns
//!     /// `Err(errno)` instead of calling `statat`.
//!     pub(crate) fn install_unwind_stat_fault(entry: &'static str, errno: i32) -> impl Drop;
//!     /// R5: thread-local running count of faults actually injected.
//!     pub(crate) fn unwind_stat_faults_injected() -> usize;
//! }
//! ```
//!
//! "No `umask()` call" is checked statically by
//! `t1s3_product_source_never_calls_umask`: the CLI product sources
//! `artifact_root.rs` and `lib.rs` must not contain `process::umask` or
//! `libc::umask` (a plain token check; an aliased import would escape it
//! [R]). Dynamically, the process umask read from `/proc/self/status` must
//! be the same before and after each run.
#![allow(dead_code)]

use crate::g7_r1_sup1_acceptance::cwd_lock;
use crate::run;
use crate::t1r2_seam::{CreatedDirectory, install_created_directory_hook};
use crate::t1s2_seam::{Point, install_hook};
use crate::t1s3_seam::{
    install_umask_unavailable, install_unwind_stat_fault, mode_check_skips,
    unwind_stat_faults_injected,
};
use reviewgraphen_core::ContentHash;
use serde_json::json;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    env, fs,
    io::Write as _,
    os::unix::fs::{PermissionsExt as _, symlink},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
};
use tempfile::TempDir;

const FOREIGN: &[u8] = b"foreign bytes that must survive\n";
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

// ------------------------------------------------------------ helpers

fn file_hashes(root: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if fs::symlink_metadata(root).is_err() {
        return out;
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for item in fs::read_dir(&directory).expect("read dir") {
            let path = item.expect("entry").path();
            let metadata = fs::symlink_metadata(&path).expect("metadata");
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                stack.push(path);
            } else if metadata.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .expect("relative")
                    .display()
                    .to_string();
                out.insert(
                    relative,
                    ContentHash::sha256(&fs::read(&path).expect("bytes")).to_string(),
                );
            }
        }
    }
    out
}

fn assert_golden(root: &Path, route: Route, what: &str) {
    let expected: BTreeMap<String, String> = golden(route)
        .iter()
        .map(|(name, hash)| ((*name).to_owned(), format!("sha256:{hash}")))
        .collect();
    let actual = file_hashes(root);
    if golden_hashes_apply(route) {
        assert_eq!(actual, expected, "{route:?} {what}: exact whole files");
    } else {
        // Off the recording host only the exact published file set is
        // literal (Rust-route bytes embed the host's Git version).
        assert_eq!(
            actual.keys().collect::<Vec<_>>(),
            expected.keys().collect::<Vec<_>>(),
            "{route:?} {what}: exact published file set"
        );
    }
}

/// Git version of the host on which the Rust-route goldens were recorded.
/// Rust-route artifacts embed the executing Git version (the snapshot's tool
/// fingerprint), so their whole-file hashes are host-bound; TypeScript goldens
/// are not.
const GOLDEN_RECORDING_GIT_VERSION: &str = "git version 2.47.3";

fn host_git_version() -> String {
    std::process::Command::new("git")
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

/// Whether `route`'s literal golden hashes apply on this host.
fn golden_hashes_apply(route: Route) -> bool {
    !matches!(route, Route::V2 | Route::V3 | Route::V4)
        || host_git_version() == GOLDEN_RECORDING_GIT_VERSION
}

/// The process umask as the kernel reports it (Linux); never changes it.
fn process_umask() -> u32 {
    let status = fs::read_to_string("/proc/self/status").expect("proc status");
    status
        .lines()
        .find_map(|line| line.strip_prefix("Umask:"))
        .map(|value| u32::from_str_radix(value.trim(), 8).expect("octal umask"))
        .expect("Umask line (kernel >= 4.7)")
}

fn nested_fixture(route: Route) -> Fixture {
    let fixture = fixture(route);
    fs::create_dir(fixture.root.join("nested")).expect("nested");
    fixture
}

// ------------------------------------------------------------ R3

/// R3 regression guard: an ordinary publish leaves the process umask as it
/// was and compares the mode (no skip recorded).
fn publish_leaves_umask_unchanged(route: Route) {
    let _cwd = cwd_lock();
    let fixture = nested_fixture(route);
    let (umask, skips) = (process_umask(), mode_check_skips());
    let outcome = review(&fixture, "nested/out");
    assert_eq!(outcome.exit_code, 0, "{route:?}: {}", outcome.stderr);
    assert_golden(&fixture.root.join("nested/out"), route, "publish");
    assert_eq!(process_umask(), umask, "{route:?}: process umask unchanged");
    assert_eq!(
        mode_check_skips(),
        skips,
        "{route:?}: with a readable umask the mode comparison is never skipped"
    );
}

/// R3: with the umask unavailable the run still publishes the same bytes,
/// the umask is unchanged, and exactly the root's mode comparison was
/// recorded as skipped (these fixtures create no `records/`).
fn umask_unavailable_publishes_and_skips_only_mode(route: Route) {
    let _cwd = cwd_lock();
    let fixture = nested_fixture(route);
    let (umask, skips) = (process_umask(), mode_check_skips());
    let outcome = {
        let _guard = install_umask_unavailable();
        review(&fixture, "nested/out")
    };
    assert_eq!(
        outcome.exit_code, 0,
        "{route:?}: a fresh root publishes when the umask is unavailable: {}",
        outcome.stderr
    );
    assert_golden(&fixture.root.join("nested/out"), route, "umask unavailable");
    assert_eq!(process_umask(), umask, "{route:?}: process umask unchanged");
    assert_eq!(
        mode_check_skips() - skips,
        1,
        "{route:?}: the skipped mode comparison is recorded, once (root)"
    );
}

/// R3: the unavailable path skips ONLY the mode comparison: a non-empty
/// foreign root substituted after mkdirat (T1-R2) is still refused.
fn umask_unavailable_still_refuses_nonempty_foreign_root(route: Route) {
    let _cwd = cwd_lock();
    let fixture = nested_fixture(route);
    let umask = process_umask();
    let fired = Rc::new(RefCell::new(0_usize));
    let outcome = {
        let counter = Rc::clone(&fired);
        let _unavailable = install_umask_unavailable();
        let _swap = install_created_directory_hook(Box::new(move |kind, path: &Path| {
            if kind != CreatedDirectory::Root {
                return;
            }
            *counter.borrow_mut() += 1;
            fs::rename(path, path.with_file_name("out.mine")).expect("move created root");
            fs::create_dir(path).expect("foreign root");
            fs::write(path.join("foreign.txt"), FOREIGN).expect("foreign file");
            fs::set_permissions(path, fs::Permissions::from_mode(0o777 & !umask))
                .expect("product mode");
        }));
        review(&fixture, "nested/out")
    };
    assert_eq!(*fired.borrow(), 1, "{route:?}: substitution fired once");
    assert_eq!(
        outcome.exit_code, 20,
        "{route:?}: emptiness still applies without a umask: {}",
        outcome.stderr
    );
    assert!(outcome.stdout.is_empty(), "{route:?}: no audit on stdout");
    let root = fixture.root.join("nested/out");
    assert_eq!(
        file_hashes(&root),
        BTreeMap::from([(
            "foreign.txt".to_owned(),
            ContentHash::sha256(FOREIGN).to_string()
        )]),
        "{route:?}: nothing written into the foreign root"
    );
    assert_eq!(process_umask(), umask, "{route:?}: process umask unchanged");
}

#[test]
fn t1s3_product_source_never_calls_umask() {
    for (name, source) in [
        ("artifact_root.rs", include_str!("artifact_root.rs")),
        ("lib.rs", include_str!("lib.rs")),
    ] {
        for token in ["process::umask", "libc::umask"] {
            assert!(
                !source.contains(token),
                "{name}: the product must never call umask(); found `{token}`"
            );
        }
    }
}

// ------------------------------------------------------------ R5

/// R5: an unexpected identity-stat error during unwind is reported as Failed
/// with its errno ("unwind incomplete"); the entry is left in place.
fn unwind_stat_error_is_failed(route: Route) {
    let _cwd = cwd_lock();
    let fixture = nested_fixture(route);
    let eacces = rustix::io::Errno::ACCESS.raw_os_error();
    let injected_before = unwind_stat_faults_injected();
    let outcome = {
        let _fault = install_unwind_stat_fault("human-report.md", eacces);
        let _plant = install_hook(Box::new(move |point, path: &Path| {
            if point == (Point::BetweenFiles { written: 0 }) {
                fs::write(path.join("artifact-manifest.v1.json"), FOREIGN_MANIFEST)
                    .expect("foreign manifest");
            }
        }));
        review(&fixture, "nested/out")
    };
    assert_eq!(
        unwind_stat_faults_injected() - injected_before,
        1,
        "{route:?}: the stat fault was injected once (escape check); stderr {:?}",
        outcome.stderr
    );
    assert_eq!(outcome.exit_code, 20, "{route:?}: {}", outcome.stderr);
    assert!(outcome.stdout.is_empty(), "{route:?}: no audit on stdout");
    let expected = format!("unwind incomplete: \"human-report.md\" (errno {eacces})");
    assert!(
        outcome
            .stderr
            .starts_with("unable to write generic review artifact")
            && outcome.stderr.contains(&expected),
        "{route:?}: stderr must carry the typed Failed record `{expected}`, got {:?}",
        outcome.stderr
    );
    let left = file_hashes(&fixture.root.join("nested/out"));
    assert!(
        left.contains_key("human-report.md"),
        "{route:?}: the entry whose stat failed is left in place: {left:?}"
    );
    assert_eq!(
        left.get("artifact-manifest.v1.json"),
        Some(&ContentHash::sha256(FOREIGN_MANIFEST).to_string()),
        "{route:?}: the foreign manifest is untouched"
    );
    assert_eq!(
        left.len(),
        2,
        "{route:?}: every other tracked file was still removed: {left:?}"
    );
}

// ------------------------------------------------------------ R4

/// R4: when the reopen of the root fails, AfterReopen does not fire.
fn after_reopen_not_fired_on_failed_open(route: Route) {
    let _cwd = cwd_lock();
    let fixture = nested_fixture(route);
    let outside = TempDir::new().expect("outside");
    let events: Rc<RefCell<Vec<Point>>> = Rc::default();
    let outcome = {
        let seen = Rc::clone(&events);
        let target = outside.path().to_path_buf();
        let _guard = install_hook(Box::new(move |point, path: &Path| {
            seen.borrow_mut().push(point);
            if point == Point::BeforeReopen {
                fs::rename(path, path.with_file_name("out.moved")).expect("move root");
                symlink(&target, path).expect("root -> outside");
            }
        }));
        review(&fixture, "nested/out")
    };
    assert_eq!(outcome.exit_code, 20, "{route:?}: {}", outcome.stderr);
    let events = events.borrow();
    assert_eq!(
        events.iter().filter(|p| **p == Point::BeforeReopen).count(),
        1,
        "{route:?}: escape check: {events:?}"
    );
    assert!(
        !events.contains(&Point::AfterReopen),
        "{route:?}: AfterReopen must fire only after a successful open: {events:?}"
    );
}

macro_rules! s3_tests {
    ($($(#[$attr:meta])* $name:ident => $body:ident($route:ident);)*) => {
        $(
            $(#[$attr])*
            #[test]
            fn $name() {
                $body(Route::$route);
            }
        )*
    };
}

s3_tests! {
    t1s3_v2_publish_leaves_process_umask_unchanged => publish_leaves_umask_unchanged(V2);
    t1s3_v2_umask_unavailable_publishes_and_skips_only_mode_check => umask_unavailable_publishes_and_skips_only_mode(V2);
    t1s3_v2_umask_unavailable_still_refuses_nonempty_foreign_root => umask_unavailable_still_refuses_nonempty_foreign_root(V2);
    t1s3_v2_unwind_stat_error_is_reported_failed => unwind_stat_error_is_failed(V2);
    t1s3_v2_after_reopen_not_fired_when_reopen_fails => after_reopen_not_fired_on_failed_open(V2);
}
