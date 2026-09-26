//! O9-R2 acceptance, F1 (Acc-O9R2, frozen before production): the Git
//! command policy fingerprint bound into `adapter_set_hash` must version the
//! ChangedLines argv shape that O9-R changed.
//!
//! F1. `git.rs`
//! `GIT_COMMAND_POLICY_VERSION` is still `"3"` and
//! `git_command_policy_fingerprint()` names no argv element, although
//! commit 57578d65 added `--inter-hunk-context=0 --indent-heuristic
//! --no-renames` to ChangedLines, which changes accepted `changed_lines`
//! facts for a repository whose local config sets those knobs. The
//! policy's own doc (`git.rs` doc on `GIT_COMMAND_POLICY_VERSION`: "Bumped
//! whenever the policy's *shape* changes in a way that could change
//! accepted facts") and `docs/20_m2_ingestion_contract.md` ("Extractor-set
//! provenance binds real tool identity", bullet `git_command_policy`: "a
//! future change to its shape must visibly change the fingerprint too")
//! require the fingerprint to change.
//!
//! Contract frozen here (the recipe the implementer must match and document):
//! the fingerprint is exactly the JSON object in
//! `tests/fixtures/o9r2-git-command-policy-v4.expected.json` -- the seven
//! documented v3 fields, `version` bumped to `"4"`, plus `environment`
//! (cleared env, inherited vars, every variable set on the child incl. the
//! F2 `GIT_NO_LAZY_FETCH=1`), `global_args` (every argument `git_command`
//! prepends) and `command_argv` (the full argv template of every
//! `GitCommand` shape and `git version`, with `<revision>`, `<base>`,
//! `<target>`, `<path>` placeholders for request-derived values). That file
//! is written by hand from the documented policy, never copied from
//! implementation output. The companion CLI test
//! `reviewgraphen-cli/tests/o9r2_git_lazy_fetch_acceptance.rs` checks the
//! same file against the argv/env the binary actually spawns, so the
//! fingerprint cannot drift from the real command shapes.
//!
//! The fingerprint itself is `pub(crate)`; this test observes it through the
//! public `ExtractionReport.adapter_set_hash`, recomputed independently from
//! the documented recipe (`docs/20_m2_ingestion_contract.md`, same section:
//! SHA-256 over the canonical JSON of `contract`, the four adapter ids,
//! `limits`, `tool_versions` {git, cargo, syn, proc_macro2, quote},
//! `git_command_policy`, `cargo_resolver_policy`).
//!
//! Expected at F@bd777529: RED for `adapter_set_hash_binds_the_v4_...` and
//! `adapter_set_hash_is_no_longer_the_v3_...` (the second one's redness
//! also proves this test's recomputation reproduces today's hash exactly);
//! the pure-JSON guards are green.

use reviewgraphen_core::{ContentHash, canonical_json};
use reviewgraphen_ingest::{IngestRequest, ingest};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::process::Command;

const EXPECTED_V4: &str = include_str!("fixtures/o9r2-git-command-policy-v4.expected.json");

fn expected_v4() -> Value {
    serde_json::from_str(EXPECTED_V4).expect("expected v4 fingerprint JSON")
}

/// The v3 fingerprint as documented in `docs/20_m2_ingestion_contract.md`
/// (`git_command_policy` bullet) with the values documented under
/// "Deterministic Git command policy" (myers, 50%, `-l20000`).
fn documented_v3() -> Value {
    json!({
        "version": "3",
        "diff_algorithm": "myers",
        "rename_similarity_percent": 50,
        "rename_limit": 20000,
        "force_text_diff": true,
        "no_replace_objects": true,
        "no_color": true,
    })
}

/// Documented cargo resolver policy (`docs/20_m2_ingestion_contract.md`
/// "Cargo tool admission": version 3, cleared env, the four fixed vars).
/// Not under test; present only because the documented hash binds it.
fn documented_cargo_policy() -> Value {
    json!({
        "version": "3",
        "automatic_path_resolution": false,
        "rustup_invocation": false,
        "admission": "host_absolute_executable",
        "child_environment": "cleared",
        "fixed_env_vars": ["CARGO_HOME", "CARGO_NET_OFFLINE", "CARGO_TERM_COLOR", "LC_ALL"],
    })
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
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
        .output()
        .expect("git is available");
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

/// Versions of `syn`/`proc-macro2`/`quote` the ingest crate resolves in the
/// workspace `Cargo.lock` (read here independently of `build.rs`).
fn locked_version(name: &str) -> String {
    let lock_text = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock"))
        .expect("workspace Cargo.lock");
    let lock: toml::Table = lock_text.parse().expect("Cargo.lock TOML");
    let packages = lock["package"].as_array().expect("package array");
    let ingest = packages
        .iter()
        .find(|p| p["name"].as_str() == Some("reviewgraphen-ingest"))
        .expect("ingest package in lock");
    let entry = ingest["dependencies"]
        .as_array()
        .expect("ingest dependencies")
        .iter()
        .filter_map(|d| d.as_str())
        .find(|d| *d == name || d.starts_with(&format!("{name} ")))
        .unwrap_or_else(|| panic!("ingest depends on {name}"));
    if let Some((_, version)) = entry.split_once(' ') {
        return version.to_owned();
    }
    let mut matching = packages
        .iter()
        .filter(|p| p["name"].as_str() == Some(name))
        .map(|p| p["version"].as_str().expect("version").to_owned());
    let version = matching.next().expect("locked package");
    assert!(
        matching.next().is_none(),
        "{name} is unique when unqualified"
    );
    version
}

struct Run {
    actual: ContentHash,
    git_version: String,
}

fn ingest_fixture() -> Run {
    let workspace = tempfile::tempdir().expect("workspace");
    let root = workspace.path().join("repository");
    fs::create_dir(&root).expect("repository dir");
    git(
        &root,
        &["init", "--quiet", "--object-format=sha1", "--template="],
    );
    fs::create_dir(root.join("src")).expect("src");
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"o9r2\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("manifest");
    fs::write(root.join("src/lib.rs"), "pub fn a() -> u32 {\n    1\n}\n").expect("lib");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "--quiet", "--no-verify", "-m", "base"]);
    let base = git(&root, &["rev-parse", "HEAD"]);
    fs::write(root.join("src/lib.rs"), "pub fn a() -> u32 {\n    2\n}\n").expect("lib");
    git(
        &root,
        &["commit", "--quiet", "--no-verify", "-am", "target"],
    );
    let target = git(&root, &["rev-parse", "HEAD"]);
    // Same `git` on PATH as `ingest` uses; the documented value is the
    // trimmed `git version` output.
    let git_version = git(&root, &["version"]);
    let request = IngestRequest::new(workspace.path(), &root, "o9r2/rust@1", base, target);
    // `cargo_admission` stays at its default (`Disabled`), so the
    // documented cargo tool version is the structured `not_admitted` kind.
    let result = ingest(&request).expect("ingest succeeds");
    Run {
        actual: result.extraction_report.adapter_set_hash,
        git_version,
    }
}

/// `adapter_set_hash` per `docs/20_m2_ingestion_contract.md`
/// ("Extractor-set provenance binds real tool identity").
fn documented_adapter_set_hash(git_version: &str, git_command_policy: Value) -> ContentHash {
    let limits = json!({ "max_files": 20_000, "max_file_bytes": 2 * 1024 * 1024 });
    ContentHash::sha256(
        &canonical_json(&json!({
            "contract": "reviewgraphen-ingest@1",
            "git_adapter": "reviewgraphen.ingest.git@1",
            "rust_adapter": "reviewgraphen.ingest.rust-syn@1",
            "rust_responsibility_signals": "reviewgraphen.ingest.rust-responsibility-signals@1",
            "cargo_adapter": "reviewgraphen.ingest.cargo-metadata@1",
            "limits": limits,
            "tool_versions": {
                "git": git_version,
                "cargo": { "available": false, "unavailable_kind": "not_admitted" },
                "syn": locked_version("syn"),
                "proc_macro2": locked_version("proc-macro2"),
                "quote": locked_version("quote"),
            },
            "git_command_policy": git_command_policy,
            "cargo_resolver_policy": documented_cargo_policy(),
        }))
        .expect("canonical JSON"),
    )
}

#[test]
fn adapter_set_hash_binds_the_v4_git_command_policy_fingerprint() {
    let run = ingest_fixture();
    let expected = documented_adapter_set_hash(&run.git_version, expected_v4());
    assert_eq!(
        run.actual,
        expected,
        "adapter_set_hash must bind the v4 Git command policy fingerprint \
         (tests/fixtures/o9r2-git-command-policy-v4.expected.json); v3-recipe hash would be {}",
        documented_adapter_set_hash(&run.git_version, documented_v3())
    );
}

#[test]
fn adapter_set_hash_is_no_longer_the_v3_policy_hash() {
    // Red today only if this recomputation reproduces the product's hash
    // byte-for-byte -- i.e. this also calibrates the recipe above.
    let run = ingest_fixture();
    assert_ne!(
        run.actual,
        documented_adapter_set_hash(&run.git_version, documented_v3()),
        "adapter_set_hash still binds the v3 Git command policy fingerprint (F1)"
    );
}

#[test]
fn v4_fingerprint_versions_the_changed_lines_argv_and_differs_from_v3() {
    let v4 = expected_v4();
    let v3 = documented_v3();
    assert_ne!(v4, v3);
    let version: u32 = v4["version"]
        .as_str()
        .expect("version is a string")
        .parse()
        .expect("numeric version");
    assert!(version > 3, "policy version must be bumped past 3");
    // Every documented v3 field survives with its documented value.
    for (key, value) in v3.as_object().expect("object") {
        if key != "version" {
            assert_eq!(&v4[key], value, "documented field `{key}` kept");
        }
    }
    let changed_lines: Vec<&str> = v4["command_argv"]["changed_lines"]
        .as_array()
        .expect("changed_lines argv")
        .iter()
        .map(|a| a.as_str().expect("string arg"))
        .collect();
    for flag in [
        "--inter-hunk-context=0",
        "--indent-heuristic",
        "--no-renames",
        "--diff-algorithm=myers",
        "--no-color",
        "--text",
        "--no-ext-diff",
        "--no-textconv",
        "--unified=0",
    ] {
        assert!(
            changed_lines.contains(&flag),
            "ChangedLines argv {flag} fingerprinted"
        );
    }
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
            v4["command_argv"][shape].is_array(),
            "shape `{shape}` fingerprinted"
        );
    }
    assert_eq!(v4["environment"]["set"]["GIT_NO_LAZY_FETCH"], "1");
}
