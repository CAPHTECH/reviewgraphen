//! O9-R acceptance (Acc-O9R, frozen before production): repository-local Git
//! configuration must not change review output.
//!
//! A repo-local
//! `diff.interHunkContext 50` merges the Rust route's `-U0` hunks, so an
//! untouched public fn lands inside `changed_lines` and a spurious
//! `relation.changed_public_callee@1` appears (exit 0, schemas valid). The
//! `ChangedLines` argv (`crates/reviewgraphen-ingest/src/git.rs`
//! `build_git_command`) pins `--unified=0`, `--diff-algorithm`, `--no-color`,
//! `--text`, `--no-ext-diff`, `--no-textconv` but not `--inter-hunk-context`
//! nor the indent heuristic.
//!
//! Contract frozen here. For each route (Rust v4, TypeScript v5, Kotlin via source review v6)
//! a small constructed repository is reviewed once with no repo-local config
//! (the baseline) and once per hostile variant written into the clone's own
//! `.git/config` / `.git/info/attributes` / files under `.git/` (never
//! tracked content, so `git status` of the clone stays clean). The variant
//! run must return the baseline exit code, byte-identical stdout, the same
//! artifact names and byte-identical artifacts. Only the two variants that
//! relocate or remove the work tree (`core.worktree`, `core.bare`) may
//! instead end in the typed fail-closed refusal: exit 20, empty stdout, no
//! artifact root. Neither can be neutralized by a `git diff` flag; both make
//! the repository a different repository, and refusing is the safe outcome.
//!
//! Every run uses the real binary as a subprocess with cwd = repository root,
//! a cleared environment (PATH and HOME only) and the request file outside
//! the repository. The fixtures commit with fixed identity and dates, and a
//! guard test checks the commit OIDs are reproducible, so every test's
//! baseline is the same input.
//!
//! Expected at F@31eba48a: the Rust route is RED for
//! `diff.interHunkContext`, the `include.path`/`includeIf` carriers of it,
//! `diff.indentHeuristic=false` (slider fixture `src/slider.rs`) and the
//! combined variant; everything else, and every TypeScript/Kotlin test, is
//! a green regression guard.

use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::OnceLock,
};
use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_reviewgraphen");
const ARTIFACTS: &str = ".o9r-artifacts";
const REFUSAL_EXIT: i32 = 20;

// ---- fixture Git (hermetic: no system/global config, fixed identity/date) --------

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Acc-O9R")
        .env("GIT_AUTHOR_EMAIL", "acc-o9r@example.invalid")
        .env("GIT_COMMITTER_NAME", "Acc-O9R")
        .env("GIT_COMMITTER_EMAIL", "acc-o9r@example.invalid")
        .env("GIT_AUTHOR_DATE", "1700000000 +0000")
        .env("GIT_COMMITTER_DATE", "1700000000 +0000")
        .output()
        .expect("git is available");
    assert!(
        output.status.success(),
        "fixture git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git stdout UTF-8")
        .trim()
        .to_owned()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    Rust,
    TypeScript,
    Kotlin,
}

/// (path, base bytes or None when absent, target bytes or None when absent).
type Leaf = (&'static str, Option<&'static str>, Option<&'static str>);

const RUST_CARGO: &str = "[package]\nname = \"o9rfixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n";

// RX shape from the probe: target changes only `alpha` and `gamma`; `beta`
// (called by `caller`) sits between the two `-U0` hunks. `moved_a.rs` is
// renamed so the name-status/rename path is exercised too.
const RUST_LIB_BASE: &str = "pub mod moved_a;
pub mod slider;

pub fn alpha(x: u32) -> u32 {
    x + 1
}

pub fn beta(x: u32) -> u32 {
    x * 2
}

pub fn gamma(x: u32) -> u32 {
    x - 1
}

pub fn caller(x: u32) -> u32 {
    beta(x)
}
";
const RUST_LIB_TARGET: &str = "pub mod moved_b;
pub mod slider;

pub fn alpha(x: u32) -> u32 {
    x + 10
}

pub fn beta(x: u32) -> u32 {
    x * 2
}

pub fn gamma(x: u32) -> u32 {
    x - 10
}

pub fn caller(x: u32) -> u32 {
    beta(x)
}
";
// Indent-heuristic slider: inserting `/// doc\npub fn two` before the
// identical `/// doc\npub fn three` has two valid `-U0` placements. With
// the heuristic (Git's default) the added range is `+5,5`; without it the
// range slides to `+6,5`, covering `three`'s doc line.
const RUST_SLIDER_BASE: &str = "pub fn one() -> u32 {
    1
}

/// doc
pub fn three() -> u32 {
    3
}

pub fn uses_three() -> u32 {
    three()
}
";
const RUST_SLIDER_TARGET: &str = "pub fn one() -> u32 {
    1
}

/// doc
pub fn two() -> u32 {
    2
}

/// doc
pub fn three() -> u32 {
    3
}

pub fn uses_three() -> u32 {
    three()
}
";
const RUST_MOVED: &str = "pub fn moved(x: u32) -> u32 {\n    x + 3\n}\n\npub fn moved_twice(x: u32) -> u32 {\n    moved(moved(x))\n}\n";

const RUST_LEAVES: [Leaf; 6] = [
    ("Cargo.toml", Some(RUST_CARGO), Some(RUST_CARGO)),
    ("src/lib.rs", Some(RUST_LIB_BASE), Some(RUST_LIB_TARGET)),
    (
        "src/slider.rs",
        Some(RUST_SLIDER_BASE),
        Some(RUST_SLIDER_TARGET),
    ),
    ("src/moved_a.rs", Some(RUST_MOVED), None),
    ("src/moved_b.rs", None, Some(RUST_MOVED)),
    ("README.txt", Some("fixture\n"), Some("fixture\n")),
];

const TS_A_BASE: &str = "export function alpha(x: number): number {\n  return x + 1;\n}\n\nexport function beta(x: number): number {\n  return x * 2;\n}\n\nexport function gamma(x: number): number {\n  return x - 1;\n}\n";
const TS_A_TARGET: &str = "export function alpha(x: number): number {\n  return x + 10;\n}\n\nexport function beta(x: number): number {\n  return x * 2;\n}\n\nexport function gamma(x: number): number {\n  return x - 10;\n}\n";
const TS_B: &str = "import { beta } from \"./a\";\n\nexport function caller(x: number): number {\n  return beta(x);\n}\n";
const TS_MOVED: &str = "export function moved(x: number): number {\n  return x + 3;\n}\n";
const TS_LEAVES: [Leaf; 4] = [
    ("src/a.ts", Some(TS_A_BASE), Some(TS_A_TARGET)),
    ("src/b.ts", Some(TS_B), Some(TS_B)),
    ("src/moved_a.ts", Some(TS_MOVED), None),
    ("src/moved_b.ts", None, Some(TS_MOVED)),
];

const KT_A_BASE: &str = "package p\n\nfun alpha(x: Int): Int {\n    return x + 1\n}\n\nfun beta(x: Int): Int {\n    return x * 2\n}\n\nfun gamma(x: Int): Int {\n    return x - 1\n}\n";
const KT_A_TARGET: &str = "package p\n\nfun alpha(x: Int): Int {\n    return x + 10\n}\n\nfun beta(x: Int): Int {\n    return x * 2\n}\n\nfun gamma(x: Int): Int {\n    return x - 10\n}\n";
const KT_B: &str = "package p\n\nfun caller(x: Int): Int = beta(x)\n";
const KT_MOVED: &str = "package p\n\nfun moved(x: Int): Int = x + 3\n";
const KT_LEAVES: [Leaf; 5] = [
    ("build.gradle.kts", Some("// cfg\n"), Some("// cfg\n")),
    ("src/main/kotlin/A.kt", Some(KT_A_BASE), Some(KT_A_TARGET)),
    ("src/main/kotlin/B.kt", Some(KT_B), Some(KT_B)),
    ("src/main/kotlin/MovedA.kt", Some(KT_MOVED), None),
    ("src/main/kotlin/MovedB.kt", None, Some(KT_MOVED)),
];

struct Fixture {
    _dir: TempDir,
    root: PathBuf,
    base: String,
    target: String,
}

fn leaves(route: Route) -> &'static [Leaf] {
    match route {
        Route::Rust => &RUST_LEAVES,
        Route::TypeScript => &TS_LEAVES,
        Route::Kotlin => &KT_LEAVES,
    }
}

/// Fresh repository: base commit (root) -> target commit, HEAD at target,
/// clean work tree. Built from scratch per call; commits are deterministic.
fn fixture(route: Route) -> Fixture {
    let dir = TempDir::new().expect("fixture dir");
    let root = dir.path().join("repository");
    fs::create_dir(&root).expect("repository dir");
    git(
        &root,
        &["init", "--quiet", "--object-format=sha1", "--template="],
    );
    git(&root, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    let write = |select: fn(&Leaf) -> Option<&'static str>| {
        for leaf in leaves(route) {
            let path = root.join(leaf.0);
            match select(leaf) {
                Some(bytes) => {
                    fs::create_dir_all(path.parent().expect("parent")).expect("leaf dir");
                    fs::write(&path, bytes).expect("leaf");
                }
                None => {
                    let _ = fs::remove_file(&path);
                }
            }
        }
        git(&root, &["add", "-A"]);
    };
    write(|leaf| leaf.1);
    git(&root, &["commit", "--quiet", "--no-verify", "-m", "base"]);
    let base = git(&root, &["rev-parse", "HEAD"]);
    write(|leaf| leaf.2);
    git(&root, &["commit", "--quiet", "--no-verify", "-m", "target"]);
    let target = git(&root, &["rev-parse", "HEAD"]);
    assert!(
        git(&root, &["status", "--porcelain", "--untracked-files=all"]).is_empty(),
        "fixture work tree is clean"
    );
    Fixture {
        _dir: dir,
        root,
        base,
        target,
    }
}

fn request(route: Route, fixture: &Fixture) -> Vec<u8> {
    let value = match route {
        Route::Rust => {
            let mut request: Value = serde_json::from_slice(include_bytes!(
                "../../../schemas/reviewgraphen.generic_review_request.v4.example.json"
            ))
            .expect("request v4 example");
            request["workspace_admission_root"] = json!(".");
            request["repository_admission_root"] = json!(".");
            request["repository_identity"] = json!("o9r/rust@1");
            request["base_revision"] = json!(fixture.base);
            request["target_revision"] = json!(fixture.target);
            request["ingest"]["max_files"] = json!(64);
            request["ingest"]["max_file_bytes"] = json!(1_048_576);
            request["ingest"]["max_total_source_bytes"] = json!(1_048_576);
            request["verifier_descriptor_id"] = Value::Null;
            request
        }
        Route::TypeScript => {
            let mut request: Value = serde_json::from_slice(include_bytes!(
                "../../../schemas/reviewgraphen.generic_review_request.v5.example.json"
            ))
            .expect("request v5 example");
            request["repository_identity"] = json!("o9r/typescript@1");
            request["base_revision"] = json!(fixture.base);
            request["target_revision"] = json!(fixture.target);
            request
        }
        // Kotlin is reviewed through source review v6 (ADR 0053).
        Route::Kotlin => json!({
            "schema": "reviewgraphen.source_review_request.v6",
            "language": "kotlin",
            "base_revision": fixture.base,
            "target_revision": fixture.target,
        }),
    };
    reviewgraphen_core::canonical_json(&value).expect("canonical request")
}

// ---- hostile repo-local configuration -------------------------------------------

/// One hostile variant. `{GIT_DIR}` in any config value or file body is
/// replaced by the clone's absolute `.git` path; `{EMPTY_DIR}` by an empty
/// directory outside the clone.
#[derive(Default)]
struct Hostile {
    config: Vec<(&'static str, &'static str)>,
    /// Written to `.git/info/attributes` (repo-local, never tracked).
    attributes: Option<&'static str>,
    /// (name under `.git/`, body, executable).
    files: Vec<(&'static str, &'static str, bool)>,
    /// Only `core.worktree`/`core.bare`: typed refusal is also acceptable.
    may_refuse: bool,
}

fn cfg(pairs: &[(&'static str, &'static str)]) -> Hostile {
    Hostile {
        config: pairs.to_vec(),
        ..Hostile::default()
    }
}

const EXT_DIFF: &str =
    "#!/bin/sh\necho '@@ -1,9999 +1,9999 @@ hostile'\necho 'diff --git a/x b/x'\nexit 0\n";
const TEXTCONV: &str = "#!/bin/sh\nsed -e 's/1/9/g' -e 's/^/X/' \"$1\"\n";
const FILTER: &str = "#!/bin/sh\nsed -e 's/1/9/g'\n";
const INCLUDE_IHC: &str = "[diff]\n\tinterHunkContext = 50\n";
const INCLUDE_IH: &str = "[diff]\n\tindentHeuristic = false\n";
const ORDER_FILE: &str = "src/slider.rs\n*.kt\n*moved*\nsrc/b.ts\n*\n";
const DRIVER_ATTRIBUTES: &str = "* diff=o9r\n";
const EOL_ATTRIBUTES: &str = "* text eol=crlf filter=o9r ident\n";

fn v_inter_hunk_context() -> Hostile {
    cfg(&[("diff.interHunkContext", "50")])
}
fn v_context() -> Hostile {
    cfg(&[("diff.context", "20")])
}
fn v_algorithm_patience() -> Hostile {
    cfg(&[("diff.algorithm", "patience")])
}
fn v_algorithm_histogram() -> Hostile {
    cfg(&[("diff.algorithm", "histogram")])
}
fn v_algorithm_minimal() -> Hostile {
    cfg(&[("diff.algorithm", "minimal")])
}
fn v_indent_heuristic_off() -> Hostile {
    cfg(&[("diff.indentHeuristic", "false")])
}
fn v_indent_heuristic_on() -> Hostile {
    cfg(&[("diff.indentHeuristic", "true")])
}
fn v_renames_off() -> Hostile {
    cfg(&[("diff.renames", "false")])
}
fn v_renames_copies_limit() -> Hostile {
    cfg(&[("diff.renames", "copies"), ("diff.renameLimit", "1")])
}
fn v_prefixes() -> Hostile {
    cfg(&[
        ("diff.noprefix", "true"),
        ("diff.mnemonicPrefix", "true"),
        ("diff.srcPrefix", "SRC/"),
        ("diff.dstPrefix", "DST/"),
    ])
}
fn v_suppress_blank_empty() -> Hostile {
    cfg(&[("diff.suppressBlankEmpty", "true")])
}
fn v_color() -> Hostile {
    cfg(&[
        ("color.ui", "always"),
        ("color.diff", "always"),
        ("diff.colorMoved", "zebra"),
        ("diff.colorMovedWS", "ignore-all-space"),
        ("diff.wsErrorHighlight", "all"),
        (
            "core.whitespace",
            "trailing-space,space-before-tab,cr-at-eol",
        ),
    ])
}
fn v_external_diff() -> Hostile {
    Hostile {
        config: vec![
            ("diff.external", "{GIT_DIR}/o9r-ext.sh"),
            ("diff.o9r.command", "{GIT_DIR}/o9r-ext.sh"),
        ],
        attributes: Some(DRIVER_ATTRIBUTES),
        files: vec![("o9r-ext.sh", EXT_DIFF, true)],
        ..Hostile::default()
    }
}
fn v_diff_driver() -> Hostile {
    Hostile {
        config: vec![
            ("diff.o9r.textconv", "{GIT_DIR}/o9r-textconv.sh"),
            ("diff.o9r.binary", "true"),
            ("diff.o9r.xfuncname", "^(.*)$"),
            ("diff.o9r.algorithm", "patience"),
            ("diff.o9r.wordRegex", "."),
        ],
        attributes: Some(DRIVER_ATTRIBUTES),
        files: vec![("o9r-textconv.sh", TEXTCONV, true)],
        ..Hostile::default()
    }
}
fn v_quote_path_abbrev() -> Hostile {
    cfg(&[("core.quotePath", "false"), ("core.abbrev", "4")])
}
fn v_relative() -> Hostile {
    cfg(&[("diff.relative", "true")])
}
fn v_order_file() -> Hostile {
    Hostile {
        config: vec![("diff.orderFile", "{GIT_DIR}/o9r-order")],
        files: vec![("o9r-order", ORDER_FILE, false)],
        ..Hostile::default()
    }
}
fn v_eol_filter() -> Hostile {
    Hostile {
        config: vec![
            ("core.autocrlf", "true"),
            ("core.eol", "crlf"),
            ("core.safecrlf", "false"),
            ("filter.o9r.clean", "{GIT_DIR}/o9r-filter.sh"),
            ("filter.o9r.smudge", "{GIT_DIR}/o9r-filter.sh"),
            ("filter.o9r.required", "true"),
        ],
        attributes: Some(EOL_ATTRIBUTES),
        files: vec![("o9r-filter.sh", FILTER, true)],
        ..Hostile::default()
    }
}
fn v_big_file_threshold() -> Hostile {
    cfg(&[("core.bigFileThreshold", "1")])
}
fn v_include_path() -> Hostile {
    Hostile {
        config: vec![("include.path", "{GIT_DIR}/o9r-include.cfg")],
        files: vec![("o9r-include.cfg", INCLUDE_IHC, false)],
        ..Hostile::default()
    }
}
fn v_include_if() -> Hostile {
    Hostile {
        // `gitdir:/` matches every repository (`/**`).
        config: vec![("includeIf.gitdir:/.path", "{GIT_DIR}/o9r-include-if.cfg")],
        files: vec![("o9r-include-if.cfg", INCLUDE_IH, false)],
        ..Hostile::default()
    }
}
fn v_log_show() -> Hostile {
    cfg(&[
        ("log.showSignature", "true"),
        ("log.decorate", "full"),
        ("log.follow", "true"),
        ("log.showRoot", "false"),
        ("format.pretty", "raw"),
        ("pretty.o9r", "%H"),
    ])
}
fn v_submodule() -> Hostile {
    cfg(&[
        ("diff.ignoreSubmodules", "none"),
        ("diff.submodule", "diff"),
        ("submodule.recurse", "true"),
        ("status.submoduleSummary", "true"),
    ])
}
fn v_core_worktree() -> Hostile {
    Hostile {
        config: vec![("core.worktree", "{EMPTY_DIR}")],
        may_refuse: true,
        ..Hostile::default()
    }
}
fn v_core_bare() -> Hostile {
    Hostile {
        config: vec![("core.bare", "true")],
        may_refuse: true,
        ..Hostile::default()
    }
}
/// Every non-refusing variant at once (one value per key).
fn v_combined() -> Hostile {
    let parts = [
        v_inter_hunk_context(),
        v_context(),
        v_algorithm_patience(),
        v_indent_heuristic_off(),
        v_renames_copies_limit(),
        v_prefixes(),
        v_suppress_blank_empty(),
        v_color(),
        v_external_diff(),
        v_diff_driver(),
        v_quote_path_abbrev(),
        v_relative(),
        v_order_file(),
        v_eol_filter(),
        v_big_file_threshold(),
        v_include_path(),
        v_include_if(),
        v_log_show(),
        v_submodule(),
    ];
    let mut combined = Hostile::default();
    let mut attributes = String::new();
    for part in parts {
        for pair in part.config {
            if !combined.config.iter().any(|(key, _)| *key == pair.0) {
                combined.config.push(pair);
            }
        }
        if let Some(text) = part.attributes
            && !attributes.contains(text)
        {
            attributes.push_str(text);
        }
        combined.files.extend(part.files);
    }
    // Both driver and eol/filter attributes, last line wins per attribute.
    combined.attributes = Some(Box::leak(attributes.into_boxed_str()));
    combined
}

// ---- runs ------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
struct RunResult {
    exit: Option<i32>,
    stdout: Vec<u8>,
    /// None when the artifact root does not exist.
    artifacts: Option<BTreeMap<String, Vec<u8>>>,
    stderr: String,
}

fn review(route: Route, fixture: &Fixture, repository: &Path) -> RunResult {
    let request_dir = TempDir::new().expect("request dir");
    let request_path = request_dir.path().join("request.json");
    fs::write(&request_path, request(route, fixture)).expect("request");
    let mut command = Command::new(BIN);
    command
        .args(["review", "--request"])
        .arg(&request_path)
        .args(["--artifacts", ARTIFACTS])
        .current_dir(repository)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default());
    if let Some(home) = std::env::var_os("HOME") {
        command.env("HOME", home);
    }
    let output: Output = command.output().expect("run reviewgraphen");
    let root = repository.join(ARTIFACTS);
    let artifacts = root.is_dir().then(|| {
        fs::read_dir(&root)
            .expect("artifact root")
            .map(|entry| {
                let entry = entry.expect("artifact entry");
                (
                    entry.file_name().into_string().expect("UTF-8 name"),
                    fs::read(entry.path()).expect("artifact bytes"),
                )
            })
            .collect()
    });
    RunResult {
        exit: output.status.code(),
        stdout: output.stdout,
        artifacts,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// Private clone of the fixture (so the fixture itself is never configured).
fn clone(fixture: &Fixture, into: &Path) -> PathBuf {
    let clone = into.join("clone");
    git(
        into,
        &[
            "clone",
            "--quiet",
            "--no-hardlinks",
            fixture.root.to_str().expect("UTF-8 path"),
            "clone",
        ],
    );
    assert_eq!(git(&clone, &["rev-parse", "HEAD"]), fixture.target);
    clone
}

fn apply(clone: &Path, empty_dir: &Path, hostile: &Hostile) {
    let git_dir = clone.join(".git");
    let substitute = |text: &str| {
        text.replace("{GIT_DIR}", git_dir.to_str().expect("UTF-8 git dir"))
            .replace("{EMPTY_DIR}", empty_dir.to_str().expect("UTF-8 empty dir"))
    };
    for (name, body, executable) in &hostile.files {
        let path = git_dir.join(name);
        fs::write(&path, substitute(body)).expect("hostile file");
        if *executable {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
        }
    }
    if let Some(attributes) = hostile.attributes {
        fs::create_dir_all(git_dir.join("info")).expect("info dir");
        fs::write(git_dir.join("info/attributes"), attributes).expect("attributes");
    }
    // Written last: once `core.bare`/`core.worktree` is set, `git config`
    // itself still works on the repository-local file.
    for (key, value) in &hostile.config {
        git(
            clone,
            &["config", "--local", "--add", key, &substitute(value)],
        );
    }
}

fn baseline(route: Route) -> &'static RunResult {
    static RUST: OnceLock<RunResult> = OnceLock::new();
    static TS: OnceLock<RunResult> = OnceLock::new();
    static KT: OnceLock<RunResult> = OnceLock::new();
    let cell = match route {
        Route::Rust => &RUST,
        Route::TypeScript => &TS,
        Route::Kotlin => &KT,
    };
    cell.get_or_init(|| {
        let fixture = fixture(route);
        let dir = TempDir::new().expect("baseline dir");
        let clone = clone(&fixture, dir.path());
        let result = review(route, &fixture, &clone);
        assert_eq!(
            result.exit,
            Some(0),
            "{route:?} baseline must succeed: {}",
            result.stderr
        );
        assert!(
            result.artifacts.as_ref().is_some_and(|a| !a.is_empty()),
            "{route:?} baseline publishes artifacts"
        );
        result
    })
}

fn count(bytes: &[u8], needle: &str) -> usize {
    String::from_utf8_lossy(bytes).matches(needle).count()
}

fn check(route: Route, hostile: Hostile) {
    let expected = baseline(route);
    let fixture = fixture(route);
    let dir = TempDir::new().expect("variant dir");
    let empty_dir = dir.path().join("empty-worktree");
    fs::create_dir(&empty_dir).expect("empty dir");
    let clone = clone(&fixture, dir.path());
    apply(&clone, &empty_dir, &hostile);
    let actual = review(route, &fixture, &clone);

    if hostile.may_refuse && actual.exit == Some(REFUSAL_EXIT) {
        eprintln!("{route:?}: typed refusal: {}", actual.stderr.trim());
        assert!(
            actual.stdout.is_empty() && actual.artifacts.is_none(),
            "{route:?}: a refusal writes no stdout and no artifact root: {}",
            actual.stderr
        );
        return;
    }
    let differing: Vec<String> = match (&expected.artifacts, &actual.artifacts) {
        (Some(want), Some(got)) => want
            .keys()
            .chain(got.keys())
            .filter(|name| want.get(*name) != got.get(*name))
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect(),
        _ => vec!["<artifact root presence>".to_owned()],
    };
    let callee = "\"relation.changed_public_callee@1\"";
    let audit = |run: &RunResult| {
        run.artifacts
            .as_ref()
            .and_then(|a| {
                a.iter()
                    .find(|(name, _)| name.starts_with("audit.run."))
                    .map(|(_, bytes)| count(bytes, callee))
            })
            .unwrap_or(0)
    };
    assert!(
        actual.exit == expected.exit && actual.stdout == expected.stdout && differing.is_empty(),
        "{route:?}: repo-local config changed review output \
         (exit {:?} -> {:?}; stdout identical: {}; differing artifacts: {:?}; \
         `{callee}` mentions in audit {} -> {}); stderr: {}",
        expected.exit,
        actual.exit,
        actual.stdout == expected.stdout,
        differing,
        audit(expected),
        audit(&actual),
        actual.stderr.lines().last().unwrap_or("")
    );
}

// ---- fixture guards --------------------------------------------------------------

#[test]
fn o9r_fixtures_are_deterministic_and_the_rust_slider_is_real() {
    // Same commits every time (fixed identity and dates), so one baseline
    // per route serves every test.
    for route in [Route::Rust, Route::TypeScript, Route::Kotlin] {
        let first = fixture(route);
        let second = fixture(route);
        assert_eq!(
            (&first.base, &first.target),
            (&second.base, &second.target),
            "{route:?} fixture commits are deterministic"
        );
    }
    // The fixture really exercises both unpinned knobs at the Git level
    // (independent of the product): hunk merging and the slider.
    let rust = fixture(Route::Rust);
    let hunks = |extra: &[&str]| {
        let mut args = vec!["-c", "core.attributesFile=/dev/null"];
        args.extend_from_slice(extra);
        args.extend_from_slice(&[
            "diff",
            "--no-color",
            "--diff-algorithm=myers",
            "--unified=0",
            &rust.base,
            &rust.target,
        ]);
        let text = git(&rust.root, &args);
        text.lines()
            .filter(|line| line.starts_with("@@ "))
            .map(|line| line.split(" @@").next().unwrap_or(line).to_owned())
            .collect::<Vec<_>>()
    };
    let plain = hunks(&["-c", "diff.indentHeuristic=true"]);
    let merged = hunks(&["-c", "diff.interHunkContext=50"]);
    let slid = hunks(&["-c", "diff.indentHeuristic=false"]);
    assert!(
        merged.len() < plain.len(),
        "interHunkContext merges: {plain:?} vs {merged:?}"
    );
    assert!(
        plain.iter().any(|h| h == "@@ -4,0 +5,5") && slid.iter().any(|h| h == "@@ -5,0 +6,5"),
        "indentHeuristic=false slides the slider hunk: {plain:?} vs {slid:?}"
    );
}

// ---- Rust v4 route (RED at F@31eba48a for interHunkContext / indentHeuristic) ---

macro_rules! hostile_tests {
    ($route:ident; $($name:ident => $variant:ident,)*) => {
        $(
            #[test]
            fn $name() {
                check(Route::$route, $variant());
            }
        )*
    };
}

hostile_tests! { Rust;
    o9r_rust_diff_inter_hunk_context => v_inter_hunk_context,
    o9r_rust_diff_context => v_context,
    o9r_rust_diff_algorithm_patience => v_algorithm_patience,
    o9r_rust_diff_algorithm_histogram => v_algorithm_histogram,
    o9r_rust_diff_algorithm_minimal => v_algorithm_minimal,
    o9r_rust_diff_indent_heuristic_off => v_indent_heuristic_off,
    o9r_rust_diff_indent_heuristic_on => v_indent_heuristic_on,
    o9r_rust_diff_renames_off => v_renames_off,
    o9r_rust_diff_renames_copies_limit => v_renames_copies_limit,
    o9r_rust_diff_prefixes => v_prefixes,
    o9r_rust_diff_suppress_blank_empty => v_suppress_blank_empty,
    o9r_rust_color => v_color,
    o9r_rust_external_diff => v_external_diff,
    o9r_rust_diff_driver_textconv_binary_funcname => v_diff_driver,
    o9r_rust_quote_path_abbrev => v_quote_path_abbrev,
    o9r_rust_diff_relative => v_relative,
    o9r_rust_diff_order_file => v_order_file,
    o9r_rust_eol_filter_ident => v_eol_filter,
    o9r_rust_big_file_threshold => v_big_file_threshold,
    o9r_rust_include_path_inter_hunk_context => v_include_path,
    o9r_rust_include_if_indent_heuristic => v_include_if,
    o9r_rust_log_show_format => v_log_show,
    o9r_rust_submodule => v_submodule,
    o9r_rust_core_worktree_identical_or_refused => v_core_worktree,
    o9r_rust_core_bare_identical_or_refused => v_core_bare,
    o9r_rust_all_combined => v_combined,
}

// ---- TypeScript v5 / Kotlin (source review v6) routes (regression guards) --------

hostile_tests! { TypeScript;
    o9r_ts_diff_inter_hunk_context => v_inter_hunk_context,
    o9r_ts_diff_indent_heuristic_off => v_indent_heuristic_off,
    o9r_ts_diff_renames_copies_limit => v_renames_copies_limit,
    o9r_ts_external_diff => v_external_diff,
    o9r_ts_diff_driver_textconv_binary_funcname => v_diff_driver,
    o9r_ts_quote_path_abbrev => v_quote_path_abbrev,
    o9r_ts_diff_order_file => v_order_file,
    o9r_ts_eol_filter_ident => v_eol_filter,
    o9r_ts_include_path_inter_hunk_context => v_include_path,
    o9r_ts_core_worktree_identical_or_refused => v_core_worktree,
    o9r_ts_core_bare_identical_or_refused => v_core_bare,
    o9r_ts_all_combined => v_combined,
}

hostile_tests! { Kotlin;
    o9r_kotlin_diff_inter_hunk_context => v_inter_hunk_context,
    o9r_kotlin_diff_indent_heuristic_off => v_indent_heuristic_off,
    o9r_kotlin_diff_renames_copies_limit => v_renames_copies_limit,
    o9r_kotlin_external_diff => v_external_diff,
    o9r_kotlin_diff_driver_textconv_binary_funcname => v_diff_driver,
    o9r_kotlin_quote_path_abbrev => v_quote_path_abbrev,
    o9r_kotlin_diff_order_file => v_order_file,
    o9r_kotlin_eol_filter_ident => v_eol_filter,
    o9r_kotlin_include_path_inter_hunk_context => v_include_path,
    o9r_kotlin_core_worktree_identical_or_refused => v_core_worktree,
    o9r_kotlin_core_bare_identical_or_refused => v_core_bare,
    o9r_kotlin_all_combined => v_combined,
}
