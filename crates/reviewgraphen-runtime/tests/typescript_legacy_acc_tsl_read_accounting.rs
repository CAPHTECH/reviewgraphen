//! Acc-TSL guard for legacy TS v5 read accounting (G8-C survivor M6b).
//!
//! Contract (admission design): the owner's bytes_read flag is true for
//! Complete, false for NotRead, and true for Failed only when the actually
//! received byte_count > 0; a source hash is attached only to FullBlob. A
//! Failed read's outcome stays UnreadBound.
//! Where the two disagree, this frozen contract wins over the product.
//!
//! Trigger: a target tree entry whose blob OID is absent from the object
//! database. A0 records it as `SourceReadClaimV1::Failed { byte_count: 0,
//! cause: MissingObject }` (runtime `admission.rs:1138` via
//! `literal_object_size`, and `:1182` via `read_literal_object`); both sites
//! set `byte_count: 0`. The file record is emitted by `generic_v5/mod.rs`
//! `file_records`.
//!
//! Expected literals come from that contract only, never from product output.

use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn git(root: &Path, args: &[&str], input: &[u8]) -> String {
    let mut child = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Acc-TSL")
        .env("GIT_COMMITTER_NAME", "Acc-TSL")
        .env("GIT_AUTHOR_EMAIL", "acc-tsl@example.invalid")
        .env("GIT_COMMITTER_EMAIL", "acc-tsl@example.invalid")
        .env("GIT_AUTHOR_DATE", "1700000000 +0000")
        .env("GIT_COMMITTER_DATE", "1700000000 +0000")
        .env("LC_ALL", "C")
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
    let out = child.wait_with_output().expect("git wait");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .expect("UTF-8")
        .trim()
        .to_owned()
}

#[test]
fn acc_tsl_zero_byte_failed_read_is_not_counted_as_bytes_read() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let repo = workspace.path().join("repository");
    fs::create_dir(&repo).expect("repository dir");
    git(
        &repo,
        &["init", "--quiet", "--object-format=sha1", "--template="],
        b"",
    );
    let good_source = b"export function good(){return 1;}\n";
    let good = git(&repo, &["hash-object", "-w", "--stdin"], good_source);
    let base_src = git(
        &repo,
        &["mktree"],
        format!("100644 blob {good}\tgood.ts\n").as_bytes(),
    );
    let base_tree = git(
        &repo,
        &["mktree"],
        format!("040000 tree {base_src}\tsrc\n").as_bytes(),
    );
    let base = git(&repo, &["commit-tree", &base_tree], b"acc-tsl base\n");
    // This OID is never written; the parent tree is readable.
    let missing = "fedcba9876543210fedcba9876543210fedcba98";
    let target_src = git(
        &repo,
        &["mktree", "--missing"],
        format!("100644 blob {good}\tgood.ts\n100644 blob {missing}\tmissing.ts\n").as_bytes(),
    );
    let target_tree = git(
        &repo,
        &["mktree"],
        format!("040000 tree {target_src}\tsrc\n").as_bytes(),
    );
    let target = git(
        &repo,
        &["commit-tree", &target_tree, "-p", &base],
        b"acc-tsl target with a missing blob\n",
    );
    git(&repo, &["update-ref", "HEAD", &target], b"");

    let artifacts = reviewgraphen_runtime::generic_v5::typescript_enumerate_and_defer_artifacts(
        workspace.path(),
        &repo,
        &base,
        &target,
        10,
        1024,
        4096,
        b"acc-tsl legacy TS read accounting request",
    )
    .expect("a missing child blob is partial read material, not a route failure");
    let extraction: Value = serde_json::from_slice(&artifacts.extraction).expect("extraction JSON");
    let record = |path: &str| -> Value {
        let rows = extraction["file_records"]
            .as_array()
            .expect("file_records array")
            .iter()
            .filter(|row| row["path"] == path)
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 1, "exactly one file record for {path}");
        rows[0].clone()
    };

    let failed = record("src/missing.ts");
    assert_eq!(
        failed["outcome"],
        json!("unread_bound"),
        "the contract keeps a Failed read's outcome as UnreadBound"
    );
    assert_eq!(
        failed["source_hash"],
        Value::Null,
        "a source hash is attached only to a FullBlob"
    );
    assert_eq!(
        failed["bytes_read"],
        json!(false),
        "a Failed read is bytes_read=true only when byte_count>0; this one received 0 bytes"
    );

    // Control on the same run: a Complete read stays bytes_read=true.
    let good_row = record("src/good.ts");
    assert_eq!(good_row["outcome"], json!("parsed"));
    assert_eq!(good_row["bytes_read"], json!(true));
}
