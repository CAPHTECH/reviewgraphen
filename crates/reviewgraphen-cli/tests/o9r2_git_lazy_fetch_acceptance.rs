//! O9-R2 acceptance (Acc-O9R2, frozen before production): the Rust review
//! route must never lazily fetch from a promisor remote, and every Git
//! command it spawns must be exactly the one its policy fingerprint names.
//!
//! F2. `crates/reviewgraphen-ingest/src/git.rs` `git_command` clears the
//! environment but does not set `GIT_NO_LAZY_FETCH=1`, unlike the TS
//! route (`runtime/src/generic_v5/admission.rs` builder).
//! In a promisor partial clone whose needed blob is missing, Git's lazy
//! object lookup runs `git fetch` against the promisor remote -- a transport
//! subprocess and an object-store write during what the ingestion contract
//! treats as a read of an immutable snapshot
//! (`docs/20_m2_ingestion_contract.md`, "Deterministic Git command policy").
//! The promisor remote here uses a custom remote-helper scheme
//! (`marker::…`) whose helper, `git-remote-marker` on a temp PATH entry,
//! only records that it ran and exits 1 -- no network, nothing fetched.
//!
//! Required outcome (frozen): the helper never runs, the clone's object
//! store is byte-for-byte unchanged, and -- because the missing blob (base
//! side of the changed `src/lib.rs`) is needed for `changed_lines`, so the
//! review cannot be completed truthfully without it -- the run ends in the
//! route's typed fail-closed refusal: exit 20, empty stdout, no artifact
//! root (the refusal convention frozen by
//! `o9r_git_config_hygiene_acceptance.rs` and `src/t1_toctou_acceptance.rs`;
//! F2 requires asserting marker absence and typed nonzero refusal).
//! Completing successfully while silently skipping the blob is NOT allowed.
//!
//! F1 companion. Every Git subprocess the Rust route spawns is logged by a
//! `git` shim on PATH (argv + environment) and must equal
//! `global_args ++ command_argv[shape]` (placeholders instantiated) with
//! exactly the environment named in
//! `reviewgraphen-ingest/tests/fixtures/o9r2-git-command-policy-v4.expected.json`
//! -- the fingerprint pinned by
//! `reviewgraphen-ingest/tests/o9r2_git_policy_fingerprint_acceptance.rs`.
//! So the fingerprint must name every argv element that actually runs.
//!
//! Expected at F@bd777529 (git 2.47.3): RED
//! `rust_review_never_lazy_fetches_from_a_promisor_remote` (helper marker is
//! written) and `every_rust_route_git_spawn_is_named_by_the_policy_fingerprint`
//! (env lacks `GIT_NO_LAZY_FETCH`); the precondition guard is green.

use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_reviewgraphen");
const ARTIFACTS: &str = ".o9r2-artifacts";
const REFUSAL_EXIT: i32 = 20;
const POLICY_V4: &str = include_str!(
    "../../reviewgraphen-ingest/tests/fixtures/o9r2-git-command-policy-v4.expected.json"
);

fn git_env(command: &mut Command) -> &mut Command {
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Acc-O9R2")
        .env("GIT_AUTHOR_EMAIL", "acc-o9r2@example.invalid")
        .env("GIT_COMMITTER_NAME", "Acc-O9R2")
        .env("GIT_COMMITTER_EMAIL", "acc-o9r2@example.invalid")
        .env("GIT_AUTHOR_DATE", "1700000000 +0000")
        .env("GIT_COMMITTER_DATE", "1700000000 +0000")
}

fn git_output(root: &Path, args: &[&str]) -> std::process::Output {
    git_env(Command::new("git").args(args).current_dir(root))
        .output()
        .expect("git is available")
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = git_output(root, args);
    assert!(
        output.status.success(),
        "fixture git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("UTF-8")
        .trim()
        .to_owned()
}

const CARGO: &str = "[package]\nname = \"o9r2fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n";
const LIB_BASE: &str = "pub fn alpha(x: u32) -> u32 {\n    x + 1\n}\n\npub fn beta(x: u32) -> u32 {\n    alpha(x) * 2\n}\n";
const LIB_TARGET: &str = "pub fn alpha(x: u32) -> u32 {\n    x + 10\n}\n\npub fn beta(x: u32) -> u32 {\n    alpha(x) * 2\n}\n";

struct Source {
    _dir: TempDir,
    root: PathBuf,
    base: String,
    target: String,
}

/// Source repository serving partial clones (`uploadpack.allowFilter`).
fn source() -> Source {
    let dir = TempDir::new().expect("source dir");
    let root = dir.path().join("source");
    fs::create_dir(&root).expect("source root");
    git(
        &root,
        &["init", "--quiet", "--object-format=sha1", "--template="],
    );
    git(&root, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    fs::create_dir(root.join("src")).expect("src");
    fs::write(root.join("Cargo.toml"), CARGO).expect("manifest");
    fs::write(root.join("src/lib.rs"), LIB_BASE).expect("lib");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "--quiet", "--no-verify", "-m", "base"]);
    let base = git(&root, &["rev-parse", "HEAD"]);
    fs::write(root.join("src/lib.rs"), LIB_TARGET).expect("lib");
    git(
        &root,
        &["commit", "--quiet", "--no-verify", "-am", "target"],
    );
    let target = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["config", "uploadpack.allowFilter", "true"]);
    git(&root, &["config", "uploadpack.allowAnySHA1InWant", "true"]);
    Source {
        _dir: dir,
        root,
        base,
        target,
    }
}

/// `git clone --filter=blob:none --no-local file://…`: target blobs are
/// fetched by the checkout, the base-only blob of `src/lib.rs` is not.
fn partial_clone(source: &Source, into: &Path) -> PathBuf {
    let url = format!("file://{}", source.root.display());
    git(
        into,
        &[
            "clone",
            "--quiet",
            "--filter=blob:none",
            "--no-local",
            &url,
            "clone",
        ],
    );
    let clone = into.join("clone");
    assert_eq!(git(&clone, &["rev-parse", "HEAD"]), source.target);
    clone
}

/// Blob OID of `rev:path`, read from the tree (never needs the blob).
fn blob_oid(repo: &Path, rev: &str, path: &str) -> String {
    let line = git(repo, &["ls-tree", "--full-tree", rev, "--", path]);
    line.split_whitespace().nth(2).expect("oid").to_owned()
}

/// Missing objects, read with lazy fetch disabled so the probe itself can
/// never fetch (the clone's own checkout above legitimately did).
fn missing_objects(repo: &Path) -> BTreeSet<String> {
    let output = git_env(
        Command::new("git")
            .args(["rev-list", "--objects", "--all", "--missing=print"])
            .current_dir(repo),
    )
    .env("GIT_NO_LAZY_FETCH", "1")
    .output()
    .expect("git");
    assert!(output.status.success(), "rev-list --missing=print");
    String::from_utf8(output.stdout)
        .expect("UTF-8")
        .lines()
        .filter_map(|l| l.strip_prefix('?').map(str::to_owned))
        .collect()
}

/// Every file under `.git/objects` with its bytes: the object store.
fn object_store(repo: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(dir: &Path, base: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).expect("objects dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                walk(&path, base, out);
            } else {
                out.insert(
                    path.strip_prefix(base)
                        .expect("prefix")
                        .display()
                        .to_string(),
                    fs::read(&path).expect("object bytes"),
                );
            }
        }
    }
    let base = repo.join(".git/objects");
    let mut out = BTreeMap::new();
    walk(&base, &base, &mut out);
    out
}

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).expect("script");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("chmod");
}

/// Points the promisor remote at `marker::…` and installs `git-remote-marker`
/// (records its argv, exits 1) in a fresh PATH directory.
fn arm_marker_remote(clone: &Path, scratch: &Path) -> (PathBuf, PathBuf) {
    let bin = scratch.join("helper-bin");
    fs::create_dir(&bin).expect("helper bin");
    let marker = scratch.join("remote-helper-invoked.marker");
    write_executable(
        &bin.join("git-remote-marker"),
        &format!("#!/bin/sh\necho \"$@\" >> '{}'\nexit 1\n", marker.display()),
    );
    git(
        clone,
        &["config", "remote.origin.url", "marker::o9r2-no-network"],
    );
    (bin, marker)
}

fn path_with(first: &Path) -> std::ffi::OsString {
    let mut paths = vec![first.to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    std::env::join_paths(paths).expect("PATH")
}

fn request(base: &str, target: &str) -> Vec<u8> {
    let mut request: Value = serde_json::from_slice(include_bytes!(
        "../../../schemas/reviewgraphen.generic_review_request.v4.example.json"
    ))
    .expect("request v4 example");
    request["workspace_admission_root"] = json!(".");
    request["repository_admission_root"] = json!(".");
    request["repository_identity"] = json!("o9r2/rust@1");
    request["base_revision"] = json!(base);
    request["target_revision"] = json!(target);
    request["ingest"]["max_files"] = json!(64);
    request["ingest"]["max_file_bytes"] = json!(1_048_576);
    request["ingest"]["max_total_source_bytes"] = json!(1_048_576);
    request["verifier_descriptor_id"] = Value::Null;
    reviewgraphen_core::canonical_json(&request).expect("canonical request")
}

struct RunResult {
    exit: Option<i32>,
    stdout: Vec<u8>,
    artifacts_exist: bool,
    stderr: String,
}

fn review(repo: &Path, base: &str, target: &str, path: &std::ffi::OsStr) -> RunResult {
    let request_dir = TempDir::new().expect("request dir");
    let request_path = request_dir.path().join("request.json");
    fs::write(&request_path, request(base, target)).expect("request");
    let mut command = Command::new(BIN);
    command
        .args(["review", "--request"])
        .arg(&request_path)
        .args(["--artifacts", ARTIFACTS])
        .current_dir(repo)
        .env_clear()
        .env("PATH", path);
    if let Some(home) = std::env::var_os("HOME") {
        command.env("HOME", home);
    }
    let output = command.output().expect("run reviewgraphen");
    RunResult {
        exit: output.status.code(),
        stdout: output.stdout,
        artifacts_exist: repo.join(ARTIFACTS).exists(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

// ---- F2 ------------------------------------------------------------------------

#[test]
fn partial_clone_fixture_really_misses_the_needed_base_blob() {
    // Guard: the precondition F2 relies on is real, not declared.
    let source = source();
    let dir = TempDir::new().expect("dir");
    let clone = partial_clone(&source, dir.path());
    let base_blob = blob_oid(&clone, &source.base, "src/lib.rs");
    let target_blob = blob_oid(&clone, &source.target, "src/lib.rs");
    assert_ne!(base_blob, target_blob);
    let missing = missing_objects(&clone);
    assert!(missing.contains(&base_blob), "base blob is missing locally");
    assert!(
        !missing.contains(&target_blob),
        "target blob was checked out"
    );
    assert_eq!(git(&clone, &["config", "remote.origin.promisor"]), "true");
    assert_eq!(
        git(&clone, &["config", "remote.origin.partialclonefilter"]),
        "blob:none"
    );
    // Plain Git without GIT_NO_LAZY_FETCH does run the marker helper: the
    // F2 path is reachable with this Git.
    let (bin, marker) = arm_marker_remote(&clone, dir.path());
    let lazy = Command::new("git")
        .args(["cat-file", "-p", &base_blob])
        .current_dir(&clone)
        .env_clear()
        .env("PATH", path_with(&bin))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git");
    assert!(!lazy.status.success());
    assert!(
        marker.exists(),
        "lazy fetch invokes the promisor remote helper"
    );
}

#[test]
fn rust_review_never_lazy_fetches_from_a_promisor_remote() {
    let source = source();
    let dir = TempDir::new().expect("dir");
    let clone = partial_clone(&source, dir.path());
    let base_blob = blob_oid(&clone, &source.base, "src/lib.rs");
    let (bin, marker) = arm_marker_remote(&clone, dir.path());
    let before_store = object_store(&clone);
    let before_missing = missing_objects(&clone);
    assert!(before_missing.contains(&base_blob));

    let run = review(&clone, &source.base, &source.target, &path_with(&bin));

    let helper_argv = fs::read_to_string(&marker).ok();
    assert!(
        helper_argv.is_none(),
        "Rust review lazily fetched: promisor remote helper ran with {helper_argv:?} \
         (exit {:?}); stderr tail: {}",
        run.exit,
        run.stderr.lines().last().unwrap_or("")
    );
    assert!(
        object_store(&clone) == before_store,
        "object store changed during review"
    );
    assert_eq!(missing_objects(&clone), before_missing);
    assert_eq!(
        run.exit,
        Some(REFUSAL_EXIT),
        "a snapshot whose needed blob is absent must end in the typed refusal: {}",
        run.stderr
    );
    assert!(
        run.stdout.is_empty() && !run.artifacts_exist,
        "a refusal writes no stdout and no artifact root"
    );
}

// ---- F1 companion: spawned argv/env == fingerprint ------------------------------

struct Call {
    argv: Vec<String>,
    env: BTreeMap<String, String>,
}

fn real_git() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join("git"))
        .find(|candidate| candidate.is_file())
        .expect("git on PATH")
}

fn install_shim(scratch: &Path) -> (PathBuf, PathBuf) {
    let bin = scratch.join("shim-bin");
    let log = scratch.join("shim-log");
    fs::create_dir(&bin).expect("shim bin");
    fs::create_dir(&log).expect("shim log");
    // Environment is captured by the real `env -0` before anything else;
    // argv NUL-separated. Then the real git runs unchanged.
    write_executable(
        &bin.join("git"),
        &format!(
            "#!/bin/sh\nf=$(mktemp '{log}/call.XXXXXXXX')\n\
             /usr/bin/env -0 > \"$f.env\"\n\
             for a in \"$@\"; do printf '%s\\0' \"$a\"; done > \"$f.argv\"\n\
             exec '{git}' \"$@\"\n",
            log = log.display(),
            git = real_git().display()
        ),
    );
    (bin, log)
}

fn read_calls(log: &Path) -> Vec<Call> {
    let split = |bytes: Vec<u8>| -> Vec<String> {
        bytes
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8(s.to_vec()).expect("UTF-8"))
            .collect()
    };
    let mut calls = Vec::new();
    for entry in fs::read_dir(log).expect("log dir") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "argv") {
            let env_path = path.with_extension("env");
            calls.push(Call {
                argv: split(fs::read(&path).expect("argv")),
                env: split(fs::read(&env_path).expect("env"))
                    .into_iter()
                    .map(|kv| {
                        let (k, v) = kv.split_once('=').expect("k=v");
                        (k.to_owned(), v.to_owned())
                    })
                    .collect(),
            });
        }
    }
    calls
}

/// Matches `argv` against a template; placeholders bind to any single
/// argument (embedded ones such as `<revision>^{commit}` match by the fixed
/// suffix/prefix around them).
fn matches_template(argv: &[String], template: &[Value]) -> bool {
    argv.len() == template.len()
        && argv.iter().zip(template).all(|(arg, t)| {
            let t = t.as_str().expect("template string");
            match t.find('<') {
                None => arg == t,
                Some(start) => {
                    let end = t.rfind('>').expect("placeholder end") + 1;
                    arg.len() >= start + (t.len() - end)
                        && arg.starts_with(&t[..start])
                        && arg.ends_with(&t[end..])
                }
            }
        })
}

#[test]
fn every_rust_route_git_spawn_is_named_by_the_policy_fingerprint() {
    let policy: Value = serde_json::from_str(POLICY_V4).expect("policy JSON");
    let global: Vec<Value> = policy["global_args"].as_array().expect("global").clone();
    let source = source();
    let dir = TempDir::new().expect("dir");
    // Full clone: the ordinary route, every shape exercised.
    git(
        dir.path(),
        &[
            "clone",
            "--quiet",
            "--no-hardlinks",
            source.root.to_str().expect("UTF-8"),
            "clone",
        ],
    );
    let clone = dir.path().join("clone");
    let (shim, log) = install_shim(dir.path());
    let run = review(&clone, &source.base, &source.target, &path_with(&shim));
    assert_eq!(
        run.exit,
        Some(0),
        "baseline review succeeds: {}",
        run.stderr
    );

    let calls = read_calls(&log);
    assert!(
        !calls.is_empty(),
        "the shim observed the route's git spawns"
    );
    let mut expected_env: BTreeMap<String, String> = policy["environment"]["set"]
        .as_object()
        .expect("env set")
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().expect("string").to_owned()))
        .collect();
    for name in policy["environment"]["inherited"]
        .as_array()
        .expect("inherited")
    {
        let name = name.as_str().expect("name");
        expected_env.insert(
            name.to_owned(),
            path_with(&shim).into_string().expect("UTF-8"),
        );
    }
    let mut seen = BTreeSet::new();
    let mut unmatched = Vec::new();
    for call in &calls {
        let shape = policy["command_argv"]
            .as_object()
            .expect("command_argv")
            .iter()
            .find(|(_, template)| {
                let mut full = global.clone();
                full.extend(template.as_array().expect("template").iter().cloned());
                matches_template(&call.argv, &full)
            })
            .map(|(shape, _)| shape.clone());
        match shape {
            Some(shape) => {
                seen.insert(shape);
                // POSIX `sh` (the shim) may itself export PWD = its cwd,
                // tolerated only when it names the clone (compared after
                // resolving symlinks: macOS reports `/private/var/...`).
                // Some shells (macOS `/bin/sh`) also export SHLVL and `_`;
                // the product clears the child environment, so these can
                // only come from the shim itself.
                let mut env = call.env.clone();
                let canonical = |p: &Path| fs::canonicalize(p).ok();
                if env
                    .get("PWD")
                    .is_some_and(|pwd| canonical(Path::new(pwd)) == canonical(&clone))
                {
                    env.remove("PWD");
                }
                env.remove("SHLVL");
                env.remove("_");
                let extra: Vec<_> = env
                    .iter()
                    .filter(|(k, v)| expected_env.get(*k) != Some(*v))
                    .collect();
                let absent: Vec<_> = expected_env
                    .iter()
                    .filter(|(k, v)| env.get(*k) != Some(*v))
                    .collect();
                assert!(
                    extra.is_empty() && absent.is_empty(),
                    "environment of spawned `git {:?}` must be exactly the fingerprinted one; \
                     unexpected/different: {extra:?}; missing/different: {absent:?}",
                    call.argv
                );
            }
            None => unmatched.push(call.argv.clone()),
        }
    }
    assert!(
        unmatched.is_empty(),
        "git spawns whose argv the policy fingerprint does not name: {unmatched:#?}"
    );
    for shape in [
        "version",
        "root",
        "resolve_commit",
        "tree_hash",
        "list_tree",
        "show_file",
        "changes",
        "changed_lines",
    ] {
        assert!(
            seen.contains(shape),
            "shape `{shape}` exercised: seen {seen:?}"
        );
    }
}
