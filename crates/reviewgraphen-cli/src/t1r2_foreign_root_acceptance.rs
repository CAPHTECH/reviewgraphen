//! T1-R2 acceptance (Acc-T1): a foreign REAL directory substituted for the
//! directory this call just created is never adopted. Frozen against
//! `a162f59a` before
//! the seam exists. The fixtures and goldens below are a verbatim copy of
//! `t1_toctou_acceptance.rs` (lines 86–414), so that frozen file stays
//! byte-identical.
//!
//! Seam interface `[R]` (the implementer adds it to
//! `crates/reviewgraphen-cli/src/lib.rs`; `#[cfg(test)]` only, absent from
//! non-test builds, thread-local, identity when unset, the guard unsets the
//! hook on drop). It is a separate module, not a new `t1_seam::SwapPoint`
//! variant, so the frozen T1 control's exact `[BeforeRootCreate,
//! AfterRootCreate]` sequence is unchanged:
//!
//! ```ignore
//! #[cfg(test)]
//! pub(crate) mod t1r2_seam {
//!     #[derive(Clone, Copy, Debug, PartialEq, Eq)]
//!     pub(crate) enum CreatedDirectory {
//!         /// The artifact root, fired immediately after `mkdirat(parent, root)`
//!         /// returned Ok.
//!         Root,
//!         /// `records/`, fired immediately after `mkdirat(root, "records")`
//!         /// returned Ok.
//!         Records,
//!     }
//!     /// Fired exactly once per successful `mkdirat` of that directory, BEFORE
//!     /// any `statat`/`openat`/`fstat` of it and before any ownership, mode,
//!     /// emptiness or chain check. The hook receives the kind and the
//!     /// directory's absolute pathname (artifact root, or root + "records").
//!     pub(crate) fn install_created_directory_hook(
//!         hook: Box<dyn FnMut(CreatedDirectory, &Path)>,
//!     ) -> impl Drop;
//! }
//! ```
//!
//! Obligations per route (v2, v3, v4, TS v5):
//! - R2-ctl: no hook → exit 0, frozen bytes; no-op hook → exit 0, same bytes,
//!   the hook fired exactly `[Root]` (these fixtures create no `records/`).
//! - R2-nonempty: at `Root`, the created root is renamed to `out.mine` and a
//!   foreign real directory with the product's mode (`0o777 & !umask`) and one
//!   foreign file is put at `out` → exit 20, empty stdout, the foreign
//!   directory keeps exactly its entries, bytes and mode, and no artifact
//!   exists anywhere in the observed region (`out.mine` may remain empty).
//! - R2-mode: same, the foreign directory is EMPTY, owned by the test user,
//!   with a mode that differs from the product's (0o700, or 0o750 when the
//!   product mode is 0o700) → same expectations; the foreign directory stays.
//! - R2-uid (a foreign owner) is NOT tested: it needs `chown` to another uid,
//!   i.e. root; Acc-T1 ran as uid 1000. Recorded as not covered.
//! - `records/` substitution is NOT reachable with these fixtures (none of the
//!   five routes creates `records/` here); the `Records` kind is specified
//!   for completeness and recorded as not covered.

use crate::g7_r1_sup1_acceptance::cwd_lock;
use crate::run;
use crate::t1r2_seam::{CreatedDirectory, install_created_directory_hook};
use reviewgraphen_core::ContentHash;
use serde_json::json;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    env, fs,
    io::Write as _,
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
};
use tempfile::TempDir;

const FOREIGN: &[u8] = b"foreign bytes that must survive\n";
const ARGUMENT: &str = "nested/out";

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

// ------------------------------------------------------------ snapshots

/// Entry kind, whole-file sha256 or link target, and permission bits.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Entry {
    Directory(u32),
    File(String, u32),
    Symlink(PathBuf),
}

/// Every entry under `base` (relative, recursive, never following symlinks),
/// with `""` for `base` itself. A missing `base` is the empty map.
fn snapshot(base: &Path) -> BTreeMap<String, Entry> {
    let mut out = BTreeMap::new();
    let Ok(metadata) = fs::symlink_metadata(base) else {
        return out;
    };
    out.insert(String::new(), entry(base, &metadata));
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return out;
    }
    let mut stack = vec![base.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for item in fs::read_dir(&directory).expect("read dir") {
            let path = item.expect("entry").path();
            let metadata = fs::symlink_metadata(&path).expect("metadata");
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                stack.push(path.clone());
            }
            let relative = path
                .strip_prefix(base)
                .expect("relative")
                .display()
                .to_string();
            out.insert(relative, entry(&path, &metadata));
        }
    }
    out
}

fn entry(path: &Path, metadata: &fs::Metadata) -> Entry {
    let mode = metadata.permissions().mode() & 0o7777;
    if metadata.file_type().is_symlink() {
        Entry::Symlink(fs::read_link(path).expect("link"))
    } else if metadata.is_dir() {
        Entry::Directory(mode)
    } else {
        let bytes = fs::read(path).expect("file bytes");
        Entry::File(ContentHash::sha256(&bytes).to_string(), mode)
    }
}

fn hashes(root: &Path) -> BTreeMap<String, String> {
    snapshot(root)
        .into_iter()
        .filter_map(|(relative, value)| match value {
            Entry::File(hash, _) => Some((relative, hash)),
            _ => None,
        })
        .collect()
}

fn assert_golden(root: &Path, route: Route, what: &str) {
    let expected: BTreeMap<String, String> = golden(route)
        .iter()
        .map(|(name, hash)| ((*name).to_owned(), format!("sha256:{hash}")))
        .collect();
    let actual = hashes(root);
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

/// The mode `mkdirat(…, 0o777)` yields under the process umask (Linux).
fn product_directory_mode() -> u32 {
    let status = fs::read_to_string("/proc/self/status").expect("proc status");
    let umask = status
        .lines()
        .find_map(|line| line.strip_prefix("Umask:"))
        .map(|value| u32::from_str_radix(value.trim(), 8).expect("octal umask"))
        .expect("Umask line");
    0o777 & !umask
}

// ------------------------------------------------------------ control

fn control(route: Route) {
    let _cwd = cwd_lock();
    let fixture = fixture(route);
    fs::create_dir(fixture.root.join("nested")).expect("nested");
    let plain = review(&fixture, ARGUMENT);
    assert_eq!(plain.exit_code, 0, "{route:?} no hook: {}", plain.stderr);
    assert_golden(&fixture.root.join(ARGUMENT), route, "no hook");

    let calls: Rc<RefCell<Vec<CreatedDirectory>>> = Rc::default();
    let seen = Rc::clone(&calls);
    let outcome = {
        let _guard = install_created_directory_hook(Box::new(move |kind, _path: &Path| {
            seen.borrow_mut().push(kind);
        }));
        review(&fixture, "nested/out2")
    };
    assert_eq!(
        outcome.exit_code, 0,
        "{route:?} no-op hook: {}",
        outcome.stderr
    );
    assert_eq!(outcome.stdout, plain.stdout, "{route:?}: stdout unchanged");
    assert_golden(&fixture.root.join("nested/out2"), route, "no-op hook");
    assert_eq!(
        *calls.borrow(),
        vec![CreatedDirectory::Root],
        "{route:?}: the hook fires once, for the root (escape check)"
    );
}

// ------------------------------------------------------------ substitution

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Foreign {
    /// Product mode, one foreign file inside.
    NonEmpty,
    /// Empty, a mode the product does not create.
    OtherMode,
}

fn plant_foreign(kind: Foreign, directory: &Path) {
    let product = product_directory_mode();
    fs::create_dir(directory).expect("foreign directory");
    let mode = match kind {
        Foreign::NonEmpty => {
            fs::write(directory.join("foreign.txt"), FOREIGN).expect("foreign file");
            product
        }
        Foreign::OtherMode => {
            if product == 0o700 {
                0o750
            } else {
                0o700
            }
        }
    };
    fs::set_permissions(directory, fs::Permissions::from_mode(mode)).expect("chmod");
    let actual = fs::symlink_metadata(directory)
        .expect("meta")
        .permissions()
        .mode()
        & 0o7777;
    assert_eq!(actual, mode, "foreign mode set");
    if kind == Foreign::OtherMode {
        assert_ne!(actual, product, "foreign mode differs from the product's");
    }
}

fn substitution_case(route: Route, kind: Foreign) {
    let _cwd = cwd_lock();
    let fixture = fixture(route);
    fs::create_dir(fixture.root.join("nested")).expect("nested");
    fs::write(fixture.root.join("nested/keep.txt"), FOREIGN).expect("keep file");

    let calls: Rc<RefCell<Vec<CreatedDirectory>>> = Rc::default();
    let after_hook: Rc<RefCell<Option<BTreeMap<String, Entry>>>> = Rc::default();
    let outcome = {
        let seen = Rc::clone(&calls);
        let snap = Rc::clone(&after_hook);
        let nested = fixture.root.join("nested");
        let _guard = install_created_directory_hook(Box::new(move |created, path: &Path| {
            seen.borrow_mut().push(created);
            if created != CreatedDirectory::Root {
                return;
            }
            assert!(
                fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()),
                "hook runs after mkdirat created the root"
            );
            fs::rename(path, path.with_file_name("out.mine")).expect("move created root");
            plant_foreign(kind, path);
            *snap.borrow_mut() = Some(snapshot(&nested));
        }));
        review(&fixture, ARGUMENT)
    };
    let what = format!("{route:?} {kind:?}");
    assert_eq!(
        calls
            .borrow()
            .iter()
            .filter(|created| **created == CreatedDirectory::Root)
            .count(),
        1,
        "{what}: the Root point fired exactly once (escape check), calls {:?}",
        calls.borrow()
    );
    let expected_after_hook = after_hook.borrow_mut().take().expect("hook snapshot");
    let foreign_after_hook: BTreeMap<String, Entry> = expected_after_hook
        .iter()
        .filter_map(|(key, value)| {
            if key == "out" {
                Some((String::new(), value.clone()))
            } else {
                key.strip_prefix("out/")
                    .map(|rest| (rest.to_owned(), value.clone()))
            }
        })
        .collect();

    // Nothing written into (or removed from, or re-moded on) the foreign directory.
    assert_eq!(
        snapshot(&fixture.root.join(ARGUMENT)),
        foreign_after_hook,
        "{what}: the foreign directory must not be adopted: entries, bytes and mode \
         unchanged (exit {}, stderr {:?})",
        outcome.exit_code,
        outcome.stderr
    );
    // Fail closed.
    assert_eq!(
        outcome.exit_code, 20,
        "{what}: a substituted root must fail closed: {}",
        outcome.stderr
    );
    assert!(outcome.stdout.is_empty(), "{what}: no audit on stdout");
    // No artifact anywhere: the observed region equals the post-hook state,
    // except the root this call created (`out.mine`) may be removed.
    let mut now = snapshot(&fixture.root.join("nested"));
    let mut expected = expected_after_hook;
    let mine_entries: Vec<String> = now
        .keys()
        .filter(|key| key.starts_with("out.mine/"))
        .cloned()
        .collect();
    assert!(
        mine_entries.is_empty(),
        "{what}: no artifact may be written into this call's moved root, found {mine_entries:?}"
    );
    now.remove("out.mine");
    expected.remove("out.mine");
    assert_eq!(
        now, expected,
        "{what}: `nested` equals the post-hook state (keep.txt and the foreign directory \
         untouched, no artifact)"
    );
}

macro_rules! t1r2_tests {
    ($($(#[$attr:meta])* $route:ident => $control:ident, $nonempty:ident, $mode:ident;)*) => {
        $(
            $(#[$attr])*
            #[test]
            fn $control() {
                control(Route::$route);
            }
            $(#[$attr])*
            #[test]
            fn $nonempty() {
                substitution_case(Route::$route, Foreign::NonEmpty);
            }
            $(#[$attr])*
            #[test]
            fn $mode() {
                substitution_case(Route::$route, Foreign::OtherMode);
            }
        )*
    };
}

t1r2_tests! {
    V2 => t1r2_v2_control_publishes_frozen_bytes,
        t1r2_v2_nonempty_foreign_root_is_not_adopted,
        t1r2_v2_other_mode_foreign_root_is_not_adopted;
    V3 => t1r2_v3_control_publishes_frozen_bytes,
        t1r2_v3_nonempty_foreign_root_is_not_adopted,
        t1r2_v3_other_mode_foreign_root_is_not_adopted;
    V4 => t1r2_v4_control_publishes_frozen_bytes,
        t1r2_v4_nonempty_foreign_root_is_not_adopted,
        t1r2_v4_other_mode_foreign_root_is_not_adopted;
    TypeScriptV5 => t1r2_ts_v5_control_publishes_frozen_bytes,
        t1r2_ts_v5_nonempty_foreign_root_is_not_adopted,
        t1r2_ts_v5_other_mode_foreign_root_is_not_adopted;
}
