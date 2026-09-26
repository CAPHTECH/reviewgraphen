//! T1 SUPPLEMENT-2 acceptance (Acc-T1): pins the descriptor mechanics that
//! were unobservable at the two
//! T1 swap points: the walk refuses symlinks (T1), the root is reopened
//! relative to the held parent without following (T8), files are created
//! relative to the held root (T2), BOTH chain checks in `write_tracked` are
//! load-bearing (T3, T4), and unwind is descriptor-relative (T5). Frozen
//! against F `efcf285f`. The fixtures and goldens are a verbatim copy of
//! `t1_toctou_acceptance.rs` lines 86–414 at `efcf285f` (post O9-R2 rebind),
//! so that frozen file stays byte-identical.
//!
//! Seam interface `[R]` (implementer adds it to `crates/reviewgraphen-cli/src/lib.rs`;
//! `#[cfg(test)]` only, absent from non-test builds, thread-local, identity
//! when unset, the guard unsets the hook on drop). A NEW module, so the frozen
//! `t1_seam` / `t1r2_seam` control call sequences are unchanged:
//!
//! ```ignore
//! #[cfg(test)]
//! pub(crate) mod t1s2_seam {
//!     #[derive(Clone, Copy, Debug, PartialEq, Eq)]
//!     pub(crate) enum Point {
//!         /// During the descriptor walk of the `--artifacts` parents: fired
//!         /// once after each parent component was opened and pushed on the
//!         /// held chain, before the next component is opened. Path: the
//!         /// absolute pathname of the component just opened.
//!         MidWalk,
//!         /// After `mkdirat` of the artifact root and after every identity /
//!         /// ownership sample the implementation takes of the created entry
//!         /// BEFORE opening it, immediately before the root is opened.
//!         /// Path: the artifact-root pathname.
//!         BeforeReopen,
//!         /// Immediately after the root was opened (and any fstat of the
//!         /// opened fd), before any comparison of it with the created entry
//!         /// and before the post-create chain verification. Path: root.
//!         AfterReopen,
//!         /// In the file loop of the writer, before the file with index
//!         /// `written` (0-based, in plan order) is created: after the
//!         /// pre-write chain check and after `records/` handling, so
//!         /// `written = 0` fires once before the first file and `written = k`
//!         /// after the k-th file was fully written. Fires once per planned
//!         /// file. Path: root.
//!         BetweenFiles { written: usize },
//!         /// During unwind, for each created FILE: after its identity check
//!         /// matched and immediately before it is unlinked. Path: the file's
//!         /// pathname (root, or root/records, joined with its name).
//!         UnwindBeforeUnlink,
//!     }
//!     pub(crate) fn install_hook(hook: Box<dyn FnMut(Point, &Path)>) -> impl Drop;
//! }
//! ```
//!
//! Point order on a successful run with parents p1..pm and n planned files:
//! `MidWalk × m, BeforeReopen, AfterReopen, BetweenFiles{0..n-1}` (asserted by
//! the controls). Swap cases (kill target in brackets) — each asserts: the
//! action point fired exactly once (escape check); OUTSIDE equals its
//! expected state at EVERY later hook event and at the end (so a transient
//! create/delete outside is caught); exit 20, empty stdout; no artifact file
//! anywhere under `nested` (foreign files keep their bytes; empty directories
//! and the attacker's symlinks may remain):
//! - `midwalk` [T1]: argument `nested/deep/out`; at MidWalk(`nested`) rename
//!   `nested/deep` → `deep.moved`, symlink `nested/deep` → OUTSIDE.
//! - `reopen_symlink_to_outside` (lead case; regression, T8 survives it by
//!   the identity comparison): at BeforeReopen rename `out` → `out.moved`,
//!   symlink `out` → OUTSIDE.
//! - `reopen_symlink_to_own_root_restored` [T8]: at BeforeReopen rename
//!   `out` → `out.real`, symlink `out` → `out.real` (same inode as created);
//!   at AfterReopen (if reached) restore. A following reopen adopts the root
//!   through the link and then passes every later check.
//! - `between_files_persist` [T2, T4, T5]: at BetweenFiles{1} rename `out` →
//!   `out.moved`, symlink `out` → OUTSIDE, kept. Only the post-write chain
//!   check sees it; later files must still land in the held root; the unwind
//!   must empty the held root although its name is now a link.
//! - `pre_check_swap_restored` [T3]: at `t1_seam::AfterRootCreate` (frozen
//!   seam, used here only as a trigger) rename + symlink to OUTSIDE; at
//!   BetweenFiles{0} (if reached) restore. Only the pre-write check sees it.
//! - `unwind_root_swapped_before_unlink` [T5]: at BetweenFiles{0} plant a
//!   foreign `artifact-manifest.v1.json` in the root (the last write then
//!   fails, as in SUP1 Q10 → unwind); at the first UnwindBeforeUnlink put a
//!   same-name foreign file in OUTSIDE and swap the root name for a symlink to
//!   OUTSIDE. A path-based unwind deletes the file OUTSIDE.
//! - `v3_residual_same_name_file_during_unwind` (`#[ignore]`, V3 residual of
//!   the T1 threat scope): at UnwindBeforeUnlink rename the real
//!   file away inside the root and plant a same-name foreign file; asserts
//!   the foreign file survives. Expected to FAIL on name-based `unlinkat`;
//!   kept ignored so it documents the residual without blocking.
#![allow(dead_code)]

use crate::g7_r1_sup1_acceptance::cwd_lock;
use crate::run;
use crate::t1_seam::{SwapPoint, install_swap_hook};
use crate::t1s2_seam::{Point, install_hook};
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
const FOREIGN_MANIFEST: &[u8] = b"foreign manifest that must survive\n";
const FOREIGN_OUTSIDE: &[u8] = b"foreign outside file that must survive\n";

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

#[derive(Clone, Debug, PartialEq, Eq)]
enum Entry {
    Directory,
    File(String),
    Symlink(PathBuf),
}

/// Every entry under `base` (relative, recursive, never following symlinks).
fn snapshot(base: &Path) -> BTreeMap<String, Entry> {
    let mut out = BTreeMap::new();
    if fs::symlink_metadata(base).is_err() {
        return out;
    }
    let mut stack = vec![base.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for item in fs::read_dir(&directory).expect("read dir") {
            let path = item.expect("entry").path();
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
                Entry::File(ContentHash::sha256(&fs::read(&path).expect("bytes")).to_string())
            };
            out.insert(relative, value);
        }
    }
    out
}

fn sha(bytes: &[u8]) -> String {
    ContentHash::sha256(bytes).to_string()
}

fn assert_golden(root: &Path, route: Route, what: &str) {
    let actual: BTreeMap<String, String> = snapshot(root)
        .into_iter()
        .filter_map(|(relative, value)| match value {
            Entry::File(hash) => Some((relative, hash)),
            _ => None,
        })
        .collect();
    let expected: BTreeMap<String, String> = golden(route)
        .iter()
        .map(|(name, hash)| ((*name).to_owned(), format!("sha256:{hash}")))
        .collect();
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

// ------------------------------------------------------------ harness

type Events = Rc<RefCell<Vec<(Point, PathBuf)>>>;

/// Shared state of one swap case: the OUTSIDE directory, its expected
/// snapshot (updated when the case itself plants something there), and
/// every deviation seen at a hook event.
struct Watch {
    outside: PathBuf,
    expected: RefCell<BTreeMap<String, Entry>>,
    deviations: RefCell<Vec<String>>,
}

impl Watch {
    fn check(&self, at: &str) {
        let now = snapshot(&self.outside);
        if now != *self.expected.borrow() {
            self.deviations
                .borrow_mut()
                .push(format!("at {at}: OUTSIDE is {now:?}"));
        }
    }

    fn replant_expected(&self) {
        *self.expected.borrow_mut() = snapshot(&self.outside);
    }
}

struct Case {
    fixture: Fixture,
    _outside: TempDir,
    watch: Rc<Watch>,
    events: Events,
}

fn setup(route: Route, deep: bool) -> Case {
    let fixture = fixture(route);
    fs::create_dir(fixture.root.join("nested")).expect("nested");
    fs::write(fixture.root.join("nested/keep.txt"), FOREIGN).expect("keep");
    if deep {
        fs::create_dir(fixture.root.join("nested/deep")).expect("deep");
    }
    let outside = TempDir::new().expect("outside");
    fs::write(outside.path().join("sentinel"), FOREIGN).expect("sentinel");
    let watch = Rc::new(Watch {
        outside: outside.path().to_path_buf(),
        expected: RefCell::new(snapshot(outside.path())),
        deviations: RefCell::default(),
    });
    Case {
        fixture,
        _outside: outside,
        watch,
        events: Rc::default(),
    }
}

/// Moves the name `path` to `moved` and puts a symlink to `target` there.
fn swap_for_link(path: &Path, moved: &Path, target: &Path) {
    fs::rename(path, moved).expect("move away");
    symlink(target, path).expect("plant link");
}

fn restore(path: &Path, moved: &Path) {
    fs::remove_file(path).expect("remove link");
    fs::rename(moved, path).expect("move back");
}

/// Final assertions shared by every swap case.
fn assert_fail_closed(case: &Case, outcome: &crate::CommandOutcome, what: &str, allowed: &[&[u8]]) {
    case.watch.check("end");
    assert!(
        case.watch.deviations.borrow().is_empty(),
        "{what}: OUTSIDE changed during or after the run: {:?} (exit {}, stderr {:?})",
        case.watch.deviations.borrow(),
        outcome.exit_code,
        outcome.stderr
    );
    assert_eq!(
        outcome.exit_code, 20,
        "{what}: must fail closed: {}",
        outcome.stderr
    );
    assert!(outcome.stdout.is_empty(), "{what}: no audit on stdout");
    let allowed: Vec<String> = allowed.iter().map(|bytes| sha(bytes)).collect();
    let foreign = sha(FOREIGN);
    let stray: Vec<(String, Entry)> = snapshot(&case.fixture.root.join("nested"))
        .into_iter()
        .filter(|(_, value)| match value {
            Entry::File(hash) => *hash != foreign && !allowed.contains(hash),
            _ => false,
        })
        .collect();
    assert!(
        stray.is_empty(),
        "{what}: no artifact file may remain under `nested`, found {stray:?}"
    );
    assert_eq!(
        fs::read(case.fixture.root.join("nested/keep.txt")).expect("keep kept"),
        FOREIGN,
        "{what}: foreign file untouched"
    );
}

fn fired(events: &Events, point: Point) -> usize {
    events.borrow().iter().filter(|(p, _)| *p == point).count()
}

// ------------------------------------------------------------ controls

fn control(route: Route) {
    let _cwd = cwd_lock();
    let fixture = fixture(route);
    fs::create_dir(fixture.root.join("nested")).expect("nested");
    let plain = review(&fixture, "nested/out");
    assert_eq!(plain.exit_code, 0, "{route:?} no hook: {}", plain.stderr);
    assert_golden(&fixture.root.join("nested/out"), route, "no hook");
    let events: Events = Rc::default();
    let seen = Rc::clone(&events);
    let outcome = {
        let _guard = install_hook(Box::new(move |point, path: &Path| {
            seen.borrow_mut().push((point, path.to_path_buf()));
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
    let root = fixture.root.canonicalize().expect("canonical root");
    let mut expected = vec![
        (Point::MidWalk, root.join("nested")),
        (Point::BeforeReopen, root.join("nested/out2")),
        (Point::AfterReopen, root.join("nested/out2")),
    ];
    for written in 0..golden(route).len() {
        expected.push((Point::BetweenFiles { written }, root.join("nested/out2")));
    }
    assert_eq!(
        *events.borrow(),
        expected,
        "{route:?}: exact seam point sequence (escape check)"
    );
}

// ------------------------------------------------------------ swap cases

/// [T1] A parent component swapped for a symlink during the walk.
fn midwalk(route: Route) {
    let _cwd = cwd_lock();
    let case = setup(route, true);
    let outcome = {
        let (events, watch) = (Rc::clone(&case.events), Rc::clone(&case.watch));
        let _guard = install_hook(Box::new(move |point, path: &Path| {
            events.borrow_mut().push((point, path.to_path_buf()));
            if point == Point::MidWalk && path.ends_with("nested") {
                swap_for_link(&path.join("deep"), &path.join("deep.moved"), &watch.outside);
            }
            watch.check(&format!("{point:?}"));
        }));
        review(&case.fixture, "nested/deep/out")
    };
    let what = format!("{route:?} midwalk");
    assert_eq!(
        case.events
            .borrow()
            .iter()
            .filter(|(p, path)| *p == Point::MidWalk && path.ends_with("nested"))
            .count(),
        1,
        "{what}: MidWalk(nested) fired once (escape check): {:?}",
        case.events.borrow()
    );
    assert_fail_closed(&case, &outcome, &what, &[]);
}

/// Lead case: the root name replaced by a symlink to OUTSIDE before reopen.
fn reopen_symlink_to_outside(route: Route) {
    let _cwd = cwd_lock();
    let case = setup(route, false);
    let outcome = {
        let (events, watch) = (Rc::clone(&case.events), Rc::clone(&case.watch));
        let _guard = install_hook(Box::new(move |point, path: &Path| {
            events.borrow_mut().push((point, path.to_path_buf()));
            if point == Point::BeforeReopen {
                swap_for_link(path, &path.with_file_name("out.moved"), &watch.outside);
            }
            watch.check(&format!("{point:?}"));
        }));
        review(&case.fixture, "nested/out")
    };
    let what = format!("{route:?} reopen_symlink_to_outside");
    assert_eq!(
        fired(&case.events, Point::BeforeReopen),
        1,
        "{what}: escape check"
    );
    assert_fail_closed(&case, &outcome, &what, &[]);
}

/// [T8] The root name replaced by a symlink to the created root itself before
/// reopen, restored right after the reopen: only a no-follow reopen refuses.
fn reopen_symlink_to_own_root_restored(route: Route) {
    let _cwd = cwd_lock();
    let case = setup(route, false);
    let outcome = {
        let (events, watch) = (Rc::clone(&case.events), Rc::clone(&case.watch));
        let _guard = install_hook(Box::new(move |point, path: &Path| {
            events.borrow_mut().push((point, path.to_path_buf()));
            match point {
                Point::BeforeReopen => {
                    swap_for_link(
                        path,
                        &path.with_file_name("out.real"),
                        Path::new("out.real"),
                    );
                }
                Point::AfterReopen
                    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) =>
                {
                    restore(path, &path.with_file_name("out.real"));
                }
                _ => {}
            }
            watch.check(&format!("{point:?}"));
        }));
        review(&case.fixture, "nested/out")
    };
    let what = format!("{route:?} reopen_symlink_to_own_root_restored");
    assert_eq!(
        fired(&case.events, Point::BeforeReopen),
        1,
        "{what}: escape check"
    );
    assert_fail_closed(&case, &outcome, &what, &[]);
}

/// [T2, T4, T5] The root name swapped for a symlink to OUTSIDE after the
/// first file, kept.
fn between_files_persist(route: Route) {
    let _cwd = cwd_lock();
    let case = setup(route, false);
    let outcome = {
        let (events, watch) = (Rc::clone(&case.events), Rc::clone(&case.watch));
        let _guard = install_hook(Box::new(move |point, path: &Path| {
            events.borrow_mut().push((point, path.to_path_buf()));
            if point == (Point::BetweenFiles { written: 1 }) {
                swap_for_link(path, &path.with_file_name("out.moved"), &watch.outside);
            }
            watch.check(&format!("{point:?}"));
        }));
        review(&case.fixture, "nested/out")
    };
    let what = format!("{route:?} between_files_persist");
    assert_eq!(
        fired(&case.events, Point::BetweenFiles { written: 1 }),
        1,
        "{what}: escape check: {:?}",
        case.events.borrow()
    );
    assert_fail_closed(&case, &outcome, &what, &[]);
}

/// [T3] Swapped before the pre-write chain check, restored before the first
/// file: only the pre-write check can see it.
fn pre_check_swap_restored(route: Route) {
    let _cwd = cwd_lock();
    let case = setup(route, false);
    let outcome = {
        let (events, watch) = (Rc::clone(&case.events), Rc::clone(&case.watch));
        let trigger_watch = Rc::clone(&case.watch);
        let triggered = Rc::new(RefCell::new(0_usize));
        let triggered_in_hook = Rc::clone(&triggered);
        let _t1 = install_swap_hook(Box::new(move |point, path: &Path| {
            if point == SwapPoint::AfterRootCreate {
                *triggered_in_hook.borrow_mut() += 1;
                swap_for_link(
                    path,
                    &path.with_file_name("out.moved"),
                    &trigger_watch.outside,
                );
            }
        }));
        let _guard = install_hook(Box::new(move |point, path: &Path| {
            events.borrow_mut().push((point, path.to_path_buf()));
            if point == (Point::BetweenFiles { written: 0 })
                && fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
            {
                restore(path, &path.with_file_name("out.moved"));
            }
            watch.check(&format!("{point:?}"));
        }));
        let outcome = review(&case.fixture, "nested/out");
        assert_eq!(*triggered.borrow(), 1, "AfterRootCreate trigger fired once");
        outcome
    };
    let what = format!("{route:?} pre_check_swap_restored");
    assert_fail_closed(&case, &outcome, &what, &[]);
}

/// [T5] During unwind, the root name is swapped for a symlink to OUTSIDE that
/// holds a same-name foreign file.
fn unwind_root_swapped_before_unlink(route: Route) {
    let _cwd = cwd_lock();
    let case = setup(route, false);
    let outcome = {
        let (events, watch) = (Rc::clone(&case.events), Rc::clone(&case.watch));
        let _guard = install_hook(Box::new(move |point, path: &Path| {
            events.borrow_mut().push((point, path.to_path_buf()));
            let unwinds = events
                .borrow()
                .iter()
                .filter(|(p, _)| *p == Point::UnwindBeforeUnlink)
                .count();
            match point {
                Point::BetweenFiles { written: 0 } => {
                    fs::write(path.join("artifact-manifest.v1.json"), FOREIGN_MANIFEST)
                        .expect("foreign manifest");
                }
                Point::UnwindBeforeUnlink if unwinds == 1 => {
                    let name = path.file_name().expect("entry name");
                    let root = path.parent().expect("root");
                    fs::write(watch.outside.join(name), FOREIGN_OUTSIDE).expect("outside twin");
                    watch.replant_expected();
                    swap_for_link(root, &root.with_file_name("out.moved"), &watch.outside);
                }
                _ => {}
            }
            watch.check(&format!("{point:?}"));
        }));
        review(&case.fixture, "nested/out")
    };
    let what = format!("{route:?} unwind_root_swapped_before_unlink");
    assert!(
        fired(&case.events, Point::UnwindBeforeUnlink) >= 1,
        "{what}: escape check: unwind reached: {:?} (exit {}, stderr {:?})",
        case.events.borrow(),
        outcome.exit_code,
        outcome.stderr
    );
    // Existing per-route reasons: v2–v4 "… artifact manifest".
    assert!(
        outcome
            .stderr
            .starts_with("unable to write generic review artifact"),
        "{what}: the planted manifest made the last write fail: {:?}",
        outcome.stderr
    );
    assert_fail_closed(&case, &outcome, &what, &[FOREIGN_MANIFEST]);
}

/// V3 residual (ignored): a same-name foreign file planted between the
/// identity check and the unlink.
fn v3_residual_same_name_file_during_unwind(route: Route) {
    let _cwd = cwd_lock();
    let case = setup(route, false);
    let planted: Rc<RefCell<Option<PathBuf>>> = Rc::default();
    let outcome = {
        let (events, planted) = (Rc::clone(&case.events), Rc::clone(&planted));
        let _guard = install_hook(Box::new(move |point, path: &Path| {
            events.borrow_mut().push((point, path.to_path_buf()));
            let first_unwind = point == Point::UnwindBeforeUnlink && planted.borrow().is_none();
            match point {
                Point::BetweenFiles { written: 0 } => {
                    fs::write(path.join("artifact-manifest.v1.json"), FOREIGN_MANIFEST)
                        .expect("foreign manifest");
                }
                _ if first_unwind => {
                    let mut moved = path.as_os_str().to_owned();
                    moved.push(".mine");
                    fs::rename(path, &moved).expect("move own file");
                    fs::write(path, FOREIGN).expect("same-name foreign file");
                    *planted.borrow_mut() = Some(path.to_path_buf());
                }
                _ => {}
            }
        }));
        review(&case.fixture, "nested/out")
    };
    let path = planted.borrow().clone().expect("unwind reached");
    assert_eq!(outcome.exit_code, 20, "fails closed");
    assert_eq!(
        fs::read(&path).ok().as_deref(),
        Some(FOREIGN),
        "{route:?}: a same-name foreign file planted between the identity check and the \
         unlink survives (V3; name-based unlinkat cannot guarantee this)"
    );
}

macro_rules! s2_tests {
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

s2_tests! {
    t1s2_v2_control_point_sequence_and_frozen_bytes => control(V2);
    t1s2_v2_midwalk_symlinked_component_is_not_followed => midwalk(V2);
    t1s2_v2_reopen_symlink_to_outside_fails_closed => reopen_symlink_to_outside(V2);
    t1s2_v2_reopen_does_not_follow_link_to_own_root => reopen_symlink_to_own_root_restored(V2);
    t1s2_v2_between_files_swap_writes_only_into_held_root => between_files_persist(V2);
    t1s2_ts_v5_between_files_swap_writes_only_into_held_root => between_files_persist(TypeScriptV5);
    t1s2_v2_pre_write_check_catches_restored_swap => pre_check_swap_restored(V2);
    t1s2_v2_unwind_is_descriptor_relative => unwind_root_swapped_before_unlink(V2);
}

#[test]
#[ignore = "V3 residual (T1 threat scope): name-based unlinkat between identity check and unlink"]
fn t1s2_v2_v3_residual_same_name_file_planted_during_unwind() {
    v3_residual_same_name_file_during_unwind(Route::V2);
}
