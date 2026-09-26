//! Shared, self-contained TypeScript v5 fixture for the T1 acceptance
//! modules: a two-commit synthetic repository built with Git plumbing under a
//! pinned identity and clock, so its object IDs and the route's output bytes
//! are reproducible without any external checkout.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use tempfile::TempDir;

fn git(root: &Path, args: &[&str], input: &[u8]) -> String {
    let mut child = Command::new("git")
        .current_dir(root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "T1")
        .env("GIT_COMMITTER_NAME", "T1")
        .env("GIT_AUTHOR_EMAIL", "t1@example.invalid")
        .env("GIT_COMMITTER_EMAIL", "t1@example.invalid")
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
        .expect("utf-8")
        .trim()
        .to_owned()
}

const BASE_FILES: [(&str, &[u8]); 3] = [
    ("package.json", b"{\"name\":\"t1\"}\n"),
    (
        "src/account.ts",
        b"export function formatBalance(n: number): string {\n  return String(n);\n}\n",
    ),
    (
        "src/transfer.ts",
        b"import { formatBalance } from \"./account\";\n\nexport function summary(n: number): string {\n  return formatBalance(n);\n}\n",
    ),
];

const TARGET_ACCOUNT: &[u8] =
    b"export function formatBalance(n: number): string {\n  return `${n / 100} JPY`;\n}\n";

fn commit(root: &Path, files: &[(&str, &[u8])], parent: Option<&str>) -> String {
    for (path, bytes) in files {
        let oid = git(root, &["hash-object", "-w", "--stdin"], bytes);
        git(
            root,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("100644,{oid},{path}"),
            ],
            b"",
        );
    }
    let tree = git(root, &["write-tree"], b"");
    match parent {
        Some(parent) => git(root, &["commit-tree", &tree, "-p", parent], b"t1 target\n"),
        None => git(root, &["commit-tree", &tree], b"t1 base\n"),
    }
}

/// A synthetic two-commit TypeScript repository and the exact r2 v5 request
/// reviewing its base..target change.
pub(crate) fn typescript_fixture() -> (TempDir, Vec<u8>) {
    let dir = TempDir::new().expect("typescript repo");
    let root = dir.path();
    git(
        root,
        &["init", "--quiet", "--object-format=sha1", "--template="],
        b"",
    );
    let base = commit(root, &BASE_FILES, None);
    let target = commit(root, &[("src/account.ts", TARGET_ACCOUNT)], Some(&base));
    git(root, &["update-ref", "HEAD", &target], b"");
    let request = format!(
        concat!(
            "{{\"base_revision\":\"{base}\",\"execution_mode\":\"enumerate_and_defer\",",
            "\"ingest\":{{\"extractor_set_hash\":\"sha256:f5722ef19c0a2a4a5b6ff583f95aa8fdc9d0cf4f3cbc0c5631c041770842b39f\",",
            "\"language\":\"typescript\",\"max_file_bytes\":1048576,\"max_files\":4096,\"max_total_source_bytes\":16777216,",
            "\"producer_id\":\"reviewgraphen.ingest.typescript_tree_sitter@1\",\"profile_id\":\"typescript.production.v1\",",
            "\"profile_version\":\"1\",\"rule_set_hash\":\"sha256:bcce4c84557970e6bc52a519b873ddd3c3eed33d518f49418da101f8c496c1d8\"}},",
            "\"projection_id\":\"typescript.obligation_report@1\",",
            "\"registry_hash\":\"sha256:4e4aa59c25c1c43257eb946de2aa0a99ad0ac193ad21b72a64f2163a1ec99a51\",",
            "\"registry_id\":\"reviewgraphen.source_review_registry.r2\",",
            "\"repository_admission_root\":\".\",\"repository_identity\":\"t1/typescript@1\",",
            "\"schema\":\"reviewgraphen.generic_review_request.v5\",",
            "\"target_revision\":\"{target}\",\"workspace_admission_root\":\".\"}}"
        ),
        base = base,
        target = target
    );
    (dir, request.into_bytes())
}
