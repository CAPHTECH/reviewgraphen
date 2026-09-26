//! T1 acceptance (Acc-T1): descriptor-relative artifact writes close the
//! artifact-root TOCTOU on every
//! writing route: v2, v3, v4 (`write_admitted_artifacts_or_unwind`), the
//! and the TypeScript v5 route (`generic_review_v5` → `write_artifacts`).
//! Frozen against F `31eba48a` before the seam exists.
//!
//! Seam interface `[R]` (the implementer adds it to
//! `crates/reviewgraphen-cli/src/lib.rs`; `#[cfg(test)]` only, absent from
//! non-test builds, thread-local, identity when unset, the guard unsets the
//! hook on drop):
//!
//! ```ignore
//! #[cfg(test)]
//! pub(crate) mod t1_seam {
//!     #[derive(Clone, Copy, Debug, PartialEq, Eq)]
//!     pub(crate) enum SwapPoint {
//!         /// After every artifact-root check of this invocation has passed
//!         /// (pathname and descriptor walk alike) and immediately before the
//!         /// artifact root directory is created (`mkdirat`).
//!         BeforeRootCreate,
//!         /// After the artifact root was created (and any post-create check
//!         /// passed) and before the first entry inside it (`records/` or the
//!         /// first artifact file) is created.
//!         AfterRootCreate,
//!     }
//!     /// The hook receives the point and the absolute artifact-root pathname
//!     /// (canonical invocation root joined with the `--artifacts` argument).
//!     /// Each point fires at most once per `review` invocation on every
//!     /// writing route (v2, v3, v4, TS v5); on a successful run
//!     /// both fire, in the order above.
//!     pub(crate) fn install_swap_hook(hook: Box<dyn FnMut(SwapPoint, &Path)>) -> impl Drop;
//! }
//! ```
//!
//! If root creation stays in `reviewgraphen-runtime`, the CLI `#[cfg(test)]`
//! hook cannot reach it; the implementer must place the `BeforeRootCreate`
//! call where the CLI can fire it between the last check and the creation
//! (for example by moving descriptor-relative admission into the CLI or by
//! passing a callback), not before the checks.
//!
//! Obligations per route (each a separate test so red/green is per case):
//! - T1-a: after a swap at a seam point, nothing is created or written in the
//!   directory outside the invocation root (its whole tree, bytes and entry
//!   kinds, equals the tree right after the hook).
//! - T1-b: the command fails closed with the existing artifact-root refusal
//!   convention: exit 20, empty stdout (reason text not frozen, as in SUP1).
//! - T1-c: no artifact file remains inside the invocation root: the observed
//!   subtrees equal the post-hook snapshot, except that the root this call
//!   created may be absent or left as an EMPTY directory at its moved
//!   location; the foreign file and the attacker's symlink stay untouched.
//! - T1-ctl: without a hook the route succeeds with the frozen bytes; with a
//!   no-op hook it succeeds with the same bytes and both points fire once,
//!   in order.
//!
//! Swap cases (attacker = a concurrent writer of the output subtree):
//! - `pre_parent`: at `BeforeRootCreate`, rename `nested` → `nested.moved`,
//!   symlink `nested` → OUTSIDE (OUTSIDE has no `out`).
//! - `post_parent`: at `AfterRootCreate`, same rename; OUTSIDE already holds an
//!   empty `out` directory.
//! - `post_root`: at `AfterRootCreate`, rename `nested/out` → `nested/out.moved`,
//!   symlink `nested/out` → OUTSIDE.
//! - `pre_plant`: at `BeforeRootCreate`, plant symlink `nested/out` → OUTSIDE
//!   (a regression guard; already refused at F `31eba48a`).

use crate::g7_r1_sup1_acceptance::cwd_lock;
use crate::run;
use crate::t1_seam::{SwapPoint, install_swap_hook};
use reviewgraphen_core::ContentHash;
use serde_json::json;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    env, fs,
    io::Write as _,
    os::unix::fs::symlink,
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

// ------------------------------------------------------------ tree snapshots

#[derive(Clone, Debug, PartialEq, Eq)]
enum Entry {
    Directory,
    /// Whole-file sha256.
    File(String),
    Symlink(PathBuf),
}

/// Every entry under `base` (relative, recursive, never following symlinks).
/// A missing `base` is the empty map.
fn snapshot(base: &Path) -> BTreeMap<String, Entry> {
    let mut out = BTreeMap::new();
    if fs::symlink_metadata(base).is_err() {
        return out;
    }
    let mut stack = vec![base.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(&directory).expect("read dir") {
            let path = entry.expect("entry").path();
            let relative = path
                .strip_prefix(base)
                .expect("relative")
                .display()
                .to_string();
            let metadata = fs::symlink_metadata(&path).expect("metadata");
            let value = if metadata.file_type().is_symlink() {
                Entry::Symlink(fs::read_link(&path).expect("link"))
            } else if metadata.is_dir() {
                stack.push(path);
                Entry::Directory
            } else {
                Entry::File(ContentHash::sha256(&fs::read(&path).expect("file bytes")).to_string())
            };
            out.insert(relative, value);
        }
    }
    out
}

/// The invocation-root region a swap can touch: the top-level names of the
/// invocation root plus the full `nested` and `nested.moved` subtrees.
fn region(invocation_root: &Path) -> BTreeMap<String, Entry> {
    let mut out = BTreeMap::new();
    for entry in fs::read_dir(invocation_root).expect("read invocation root") {
        let name = entry
            .expect("entry")
            .file_name()
            .to_string_lossy()
            .into_owned();
        out.insert(format!("top:{name}"), Entry::Directory);
    }
    for sub in ["nested", "nested.moved"] {
        for (relative, value) in snapshot(&invocation_root.join(sub)) {
            out.insert(format!("{sub}/{relative}"), value);
        }
    }
    out
}

fn hashes(root: &Path) -> BTreeMap<String, String> {
    snapshot(root)
        .into_iter()
        .filter_map(|(relative, value)| match value {
            Entry::File(hash) => Some((relative, hash)),
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

// ------------------------------------------------------------ controls

/// T1-ctl: no hook → success with frozen bytes; no-op hook → success with the
/// same bytes and both seam points fired once, in order.
fn control(route: Route) {
    let _cwd = cwd_lock();
    let fixture = fixture(route);
    fs::create_dir(fixture.root.join("nested")).expect("nested");
    let plain = review(&fixture, ARGUMENT);
    assert_eq!(plain.exit_code, 0, "{route:?} no hook: {}", plain.stderr);
    assert_golden(&fixture.root.join(ARGUMENT), route, "no hook");

    let calls: Rc<RefCell<Vec<SwapPoint>>> = Rc::default();
    let seen = Rc::clone(&calls);
    let outcome = {
        let _guard = install_swap_hook(Box::new(move |point, _root: &Path| {
            seen.borrow_mut().push(point);
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
        vec![SwapPoint::BeforeRootCreate, SwapPoint::AfterRootCreate],
        "{route:?}: both seam points fire once, in order (escape check)"
    );
}

// ------------------------------------------------------------ swap cases

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Swap {
    PreParent,
    PostParent,
    PostRoot,
    PrePlant,
}

impl Swap {
    fn point(self) -> SwapPoint {
        match self {
            Swap::PreParent | Swap::PrePlant => SwapPoint::BeforeRootCreate,
            Swap::PostParent | Swap::PostRoot => SwapPoint::AfterRootCreate,
        }
    }

    /// Where the root this call created ends up after the swap (region key).
    fn moved_root(self) -> Option<&'static str> {
        match self {
            Swap::PreParent | Swap::PostParent => Some("nested.moved/out"),
            Swap::PostRoot => Some("nested/out.moved"),
            Swap::PrePlant => None,
        }
    }
}

fn apply_swap(swap: Swap, root: &Path, outside: &Path) {
    let parent = root.parent().expect("artifact root parent");
    let moved_parent = parent.with_file_name("nested.moved");
    match swap {
        Swap::PreParent | Swap::PostParent => {
            fs::rename(parent, &moved_parent).expect("move parent away");
            symlink(outside, parent).expect("parent -> outside");
        }
        Swap::PostRoot => {
            fs::rename(root, root.with_file_name("out.moved")).expect("move root away");
            symlink(outside, root).expect("root -> outside");
        }
        Swap::PrePlant => symlink(outside, root).expect("plant root -> outside"),
    }
}

/// T1-a/b/c for one route and one swap case.
fn swap_case(route: Route, swap: Swap) {
    let _cwd = cwd_lock();
    let fixture = fixture(route);
    let outside = TempDir::new().expect("outside dir");
    fs::write(outside.path().join("sentinel"), FOREIGN).expect("outside sentinel");
    if swap == Swap::PostParent {
        // R2 interleaving B: the replacement parent already holds `out`.
        fs::create_dir(outside.path().join("out")).expect("outside out");
    }
    fs::create_dir(fixture.root.join("nested")).expect("nested");
    fs::write(fixture.root.join("nested/keep.txt"), FOREIGN).expect("foreign file");

    type Snapshots = (BTreeMap<String, Entry>, BTreeMap<String, Entry>);
    let calls: Rc<RefCell<Vec<SwapPoint>>> = Rc::default();
    let after_hook: Rc<RefCell<Option<Snapshots>>> = Rc::default();
    let outcome = {
        let seen = Rc::clone(&calls);
        let snap = Rc::clone(&after_hook);
        let outside_path = outside.path().to_path_buf();
        let invocation_root = fixture.root.clone();
        let _guard = install_swap_hook(Box::new(move |point, root: &Path| {
            seen.borrow_mut().push(point);
            if point != swap.point() {
                return;
            }
            apply_swap(swap, root, &outside_path);
            *snap.borrow_mut() = Some((snapshot(&outside_path), region(&invocation_root)));
        }));
        review(&fixture, ARGUMENT)
    };
    let what = format!("{route:?} {swap:?}");
    assert_eq!(
        calls
            .borrow()
            .iter()
            .filter(|point| **point == swap.point())
            .count(),
        1,
        "{what}: the seam point fired exactly once (escape check), calls {:?}",
        calls.borrow()
    );
    let (outside_after_hook, region_after_hook) =
        after_hook.borrow_mut().take().expect("hook snapshot");

    // T1-a: nothing created or written outside the invocation root.
    assert_eq!(
        snapshot(outside.path()),
        outside_after_hook,
        "{what}: nothing may be created or written outside the invocation root \
         (exit {}, stderr {:?})",
        outcome.exit_code,
        outcome.stderr
    );
    // T1-b: fail closed with the artifact-root refusal convention.
    assert_eq!(
        outcome.exit_code, 20,
        "{what}: a swapped artifact-root component must fail closed: {}",
        outcome.stderr
    );
    assert!(outcome.stdout.is_empty(), "{what}: no audit on stdout");
    // T1-c: no artifact file remains; foreign entries untouched.
    let mut region_now = region(&fixture.root);
    let mut expected = region_after_hook;
    if let Some(moved) = swap.moved_root() {
        let prefix = format!("{moved}/");
        let left: Vec<String> = region_now
            .keys()
            .filter(|key| key.starts_with(&prefix))
            .cloned()
            .collect();
        assert!(
            left.is_empty(),
            "{what}: no artifact entry may remain in the created root, found {left:?}"
        );
        // The created root itself may be removed or remain as an empty directory.
        if let Some(value) = region_now.remove(moved) {
            assert_eq!(value, Entry::Directory, "{what}: moved root kind");
        }
        expected.retain(|key, _| key != moved && !key.starts_with(&prefix));
    }
    assert_eq!(
        region_now, expected,
        "{what}: invocation-root region equals the post-hook state (foreign file and \
         attacker symlink untouched, no partial output)"
    );
    assert_eq!(
        fs::read(
            fixture
                .root
                .join(if matches!(swap, Swap::PreParent | Swap::PostParent) {
                    "nested.moved/keep.txt"
                } else {
                    "nested/keep.txt"
                })
        )
        .expect("foreign file kept"),
        FOREIGN,
        "{what}: foreign bytes untouched"
    );
}

macro_rules! t1_tests {
    ($($(#[$attr:meta])* $route:ident => $control:ident, $pre_parent:ident, $post_parent:ident, $post_root:ident, $pre_plant:ident;)*) => {
        $(
            $(#[$attr])*
            #[test]
            fn $control() {
                control(Route::$route);
            }
            $(#[$attr])*
            #[test]
            fn $pre_parent() {
                swap_case(Route::$route, Swap::PreParent);
            }
            $(#[$attr])*
            #[test]
            fn $post_parent() {
                swap_case(Route::$route, Swap::PostParent);
            }
            $(#[$attr])*
            #[test]
            fn $post_root() {
                swap_case(Route::$route, Swap::PostRoot);
            }
            $(#[$attr])*
            #[test]
            fn $pre_plant() {
                swap_case(Route::$route, Swap::PrePlant);
            }
        )*
    };
}

t1_tests! {
    V2 => t1_v2_control_no_hook_publishes_frozen_bytes,
        t1_v2_parent_swapped_before_root_create_writes_nothing_outside,
        t1_v2_parent_swapped_after_root_create_writes_nothing_outside,
        t1_v2_root_swapped_after_root_create_writes_nothing_outside,
        t1_v2_root_planted_before_root_create_writes_nothing_outside;
    V3 => t1_v3_control_no_hook_publishes_frozen_bytes,
        t1_v3_parent_swapped_before_root_create_writes_nothing_outside,
        t1_v3_parent_swapped_after_root_create_writes_nothing_outside,
        t1_v3_root_swapped_after_root_create_writes_nothing_outside,
        t1_v3_root_planted_before_root_create_writes_nothing_outside;
    V4 => t1_v4_control_no_hook_publishes_frozen_bytes,
        t1_v4_parent_swapped_before_root_create_writes_nothing_outside,
        t1_v4_parent_swapped_after_root_create_writes_nothing_outside,
        t1_v4_root_swapped_after_root_create_writes_nothing_outside,
        t1_v4_root_planted_before_root_create_writes_nothing_outside;
    TypeScriptV5 => t1_ts_v5_control_no_hook_publishes_frozen_bytes,
        t1_ts_v5_parent_swapped_before_root_create_writes_nothing_outside,
        t1_ts_v5_parent_swapped_after_root_create_writes_nothing_outside,
        t1_ts_v5_root_swapped_after_root_create_writes_nothing_outside,
        t1_ts_v5_root_planted_before_root_create_writes_nothing_outside;
}
