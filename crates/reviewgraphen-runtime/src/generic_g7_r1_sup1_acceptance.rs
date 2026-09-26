//! G7-R1 acceptance SUPPLEMENT-1, Q4a/Q4b (Acc-R1, frozen before the seam
//! exists). Kills a construction site that ignores
//! `ValidatedGenericReviewRunV4::new`'s `Err`.
//!
//! Call sites of `ValidatedGenericReviewRunV4::new` at `a7c41af1`
//! (`crates/reviewgraphen-runtime/src/generic.rs`):
//! * site A, `run_generic_review_v4` (the returned value of the fn at :2887),
//!   `ValidatedGenericReviewRunV4::new(serde_json::to_value(run)?)` at :3147;
//! * site B, `decode_generic_review_run_v4_with_basis`,
//!   `ValidatedGenericReviewRunV4::new(wire.canonical_value)` at :5480.
//!
//! Seam interface `[R]` (to be added by the implementer in generic.rs,
//! `#[cfg(test)]` only, absent from non-test builds):
//!
//! ```ignore
//! #[cfg(test)]
//! pub(crate) mod g7_r1_seam {
//!     /// Thread-local; the returned guard unsets the hook on drop.
//!     pub(crate) fn install_run_v4_value_hook(
//!         hook: Box<dyn FnMut(&mut serde_json::Value)>,
//!     ) -> impl Drop;
//! }
//! ```
//!
//! At BOTH sites the value is passed through the hook immediately before it
//! is handed to `new` (identity when no hook is installed; in non-test builds
//! the value is handed over unchanged with no hook code at all). The hook is
//! called exactly once per construction.

use super::g7_r1_seam::install_run_v4_value_hook;
use super::{
    GenericReviewError, GenericReviewRunV4Basis, decode_and_validate_generic_review_request_v4,
    decode_generic_review_run_v4_with_basis, run_generic_review_v4,
};
use reviewgraphen_core::canonical_json;
use serde_json::{Value, json};
use std::{cell::Cell, collections::BTreeMap, fs, path::Path, process::Command, rc::Rc};

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "ReviewGraphen Test")
        .env("GIT_AUTHOR_EMAIL", "reviewgraphen@example.invalid")
        .env("GIT_COMMITTER_NAME", "ReviewGraphen Test")
        .env("GIT_COMMITTER_EMAIL", "reviewgraphen@example.invalid")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git is available");
    assert!(output.status.success(), "git failed: {arguments:?}");
    String::from_utf8(output.stdout)
        .expect("git stdout UTF-8")
        .trim()
        .to_owned()
}

/// Out-of-schema in a way the widening cannot absorb:
/// `authority.trusted_pass` is `const false` in run-v4.
fn forge_trusted_pass(value: &mut Value) {
    value["authority"]["trusted_pass"] = json!(true);
}

fn counting_forger(calls: &Rc<Cell<u32>>) -> Box<dyn FnMut(&mut Value)> {
    let calls = Rc::clone(calls);
    Box::new(move |value: &mut Value| {
        calls.set(calls.get() + 1);
        forge_trusted_pass(value);
    })
}

fn run_site_a() -> Result<(), GenericReviewError> {
    let temporary = tempfile::tempdir().expect("temporary repository");
    let repository = temporary.path().join("repository");
    fs::create_dir(&repository).expect("repository directory");
    git(&repository, &["init", "-q"]);
    fs::write(
        repository.join("lib.rs"),
        "pub fn callee() -> u64 { 1 }\npub fn caller() -> u64 { callee() }\n",
    )
    .expect("base source");
    git(&repository, &["add", "-A"]);
    git(&repository, &["commit", "-q", "-m", "base"]);
    let base = git(&repository, &["rev-parse", "HEAD"]);
    fs::write(
        repository.join("lib.rs"),
        "pub fn callee() -> u64 { 2 }\npub fn caller() -> u64 { callee() }\n",
    )
    .expect("target source");
    git(&repository, &["add", "-A"]);
    git(&repository, &["commit", "-q", "-m", "target"]);
    let target = git(&repository, &["rev-parse", "HEAD"]);
    let mut request: Value = serde_json::from_slice(include_bytes!(
        "../../../schemas/reviewgraphen.generic_review_request.v4.example.json"
    ))
    .expect("request example");
    let root = repository.display().to_string();
    request["workspace_admission_root"] = json!(root);
    request["repository_admission_root"] = json!(root);
    request["repository_identity"] = json!("g7-r1/sup1@1");
    request["base_revision"] = json!(base);
    request["target_revision"] = json!(target);
    request["ingest"]["max_files"] = json!(32);
    request["ingest"]["max_file_bytes"] = json!(1_048_576);
    request["ingest"]["max_total_source_bytes"] = json!(1_048_576);
    request["verifier_descriptor_id"] = Value::Null;
    let request = decode_and_validate_generic_review_request_v4(
        &canonical_json(&request).expect("canonical request"),
    )
    .expect("closed request");
    run_generic_review_v4(&request).map(|_| ())
}

fn run_site_b() -> Result<(), GenericReviewError> {
    // The checked-in example has zero contexts, so an empty basis is complete.
    let value: Value = serde_json::from_slice(include_bytes!(
        "../../../schemas/reviewgraphen.generic_review_run.v4.example.json"
    ))
    .expect("example JSON");
    assert_eq!(value["contexts"], json!([]), "precondition: no contexts");
    let basis = GenericReviewRunV4Basis {
        d_context_bases: BTreeMap::new(),
        node_context_bases: BTreeMap::new(),
    };
    decode_generic_review_run_v4_with_basis(&canonical_json(&value).expect("canonical"), &basis)
        .map(|_| ())
}

#[test]
fn q4a_site_a_run_generic_review_v4_propagates_new_refusal() {
    assert!(run_site_a().is_ok(), "control: unhooked site A succeeds");
    let calls = Rc::new(Cell::new(0));
    {
        let _guard = install_run_v4_value_hook(counting_forger(&calls));
        let result = run_site_a();
        assert_eq!(calls.get(), 1, "the hook sits on site A's path, once");
        assert!(
            matches!(result, Err(GenericReviewError::RunV4SchemaInvalid(_))),
            "site A must return new()'s schema refusal, got {result:?}"
        );
    }
    assert!(
        run_site_a().is_ok(),
        "guard reset: identity again after drop"
    );
    assert_eq!(calls.get(), 1, "no hook call after the guard dropped");
}

#[test]
fn q4b_site_b_decode_with_basis_propagates_new_refusal() {
    assert!(run_site_b().is_ok(), "control: unhooked site B succeeds");
    let calls = Rc::new(Cell::new(0));
    {
        let _guard = install_run_v4_value_hook(counting_forger(&calls));
        let result = run_site_b();
        assert_eq!(calls.get(), 1, "the hook sits on site B's path, once");
        assert!(
            matches!(result, Err(GenericReviewError::RunV4SchemaInvalid(_))),
            "site B must return new()'s schema refusal, got {result:?}"
        );
    }
    assert!(
        run_site_b().is_ok(),
        "guard reset: identity again after drop"
    );
}

/// Escape check: an identity hook changes nothing at either site.
#[test]
fn q4_identity_hook_is_transparent() {
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);
    let _guard = install_run_v4_value_hook(Box::new(move |_value: &mut Value| {
        counter.set(counter.get() + 1);
    }));
    assert!(run_site_b().is_ok());
    assert!(run_site_a().is_ok());
    assert_eq!(calls.get(), 2);
}
