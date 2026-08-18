//! Harness-owned acceptance test for m8-impl-local-v1.
//!
//! This file is written by the experiment harness, never by a candidate.
//! It is copied into `crates/reviewgraphen-cli/tests/review_flag_order.rs`
//! of an isolated scratch copy of the repository before the candidate's
//! edit is applied, and again before the baseline measurement.
//!
//! It fails on the pinned pre-change revision (that is the point: the
//! feature is absent) and must pass after a correct implementation.
//!
//! Observability note: `generic_review` reads the request file first, so a
//! request path that does not exist yields exit code 3 ("unable to read
//! input file"). A rejected command line instead yields exit code 2 with
//! the usage string. That difference is what distinguishes "the command
//! form was accepted" from "the command form was rejected" without
//! creating any file.

use reviewgraphen_cli::run;

const MISSING_REQUEST: &str = "/nonexistent/m8-impl-local-v1/request.json";
const MISSING_ARTIFACTS: &str = "/nonexistent/m8-impl-local-v1/artifacts";

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

/// The usage-rejection shape every invalid form must share.
fn assert_rejected(parts: &[&str]) {
    let baseline = run(argv(&["review"]));
    assert_eq!(baseline.exit_code, 2, "baseline `review` must be rejected");
    let outcome = run(argv(parts));
    assert_eq!(outcome.exit_code, 2, "expected rejection for {parts:?}");
    assert!(
        outcome.stdout.is_empty(),
        "rejection must not write stdout for {parts:?}"
    );
    assert_eq!(
        outcome.stderr, baseline.stderr,
        "rejection must use the unchanged usage text for {parts:?}"
    );
}

/// The accepted shape: the command reached `generic_review`, which then
/// failed to read the (deliberately absent) request file.
fn assert_accepted(parts: &[&str]) {
    let outcome = run(argv(parts));
    assert_eq!(
        outcome.exit_code, 3,
        "expected the review command to be accepted and to fail reading the \
         absent request file for {parts:?}"
    );
    assert!(
        outcome.stdout.is_empty(),
        "a failed review must not write stdout for {parts:?}"
    );
}

#[test]
fn request_before_artifacts_is_accepted() {
    assert_accepted(&[
        "review",
        "--request",
        MISSING_REQUEST,
        "--artifacts",
        MISSING_ARTIFACTS,
    ]);
}

#[test]
fn artifacts_before_request_is_accepted() {
    assert_accepted(&[
        "review",
        "--artifacts",
        MISSING_ARTIFACTS,
        "--request",
        MISSING_REQUEST,
    ]);
}

#[test]
fn incomplete_forms_are_rejected() {
    assert_rejected(&["review", "--request", MISSING_REQUEST]);
    assert_rejected(&["review", "--artifacts", MISSING_ARTIFACTS]);
    assert_rejected(&["review", "--request"]);
    assert_rejected(&["review", "--artifacts"]);
}

#[test]
fn duplicate_flags_are_rejected() {
    assert_rejected(&[
        "review",
        "--request",
        MISSING_REQUEST,
        "--request",
        MISSING_REQUEST,
    ]);
    assert_rejected(&[
        "review",
        "--artifacts",
        MISSING_ARTIFACTS,
        "--artifacts",
        MISSING_ARTIFACTS,
    ]);
}

#[test]
fn unknown_flags_are_rejected() {
    assert_rejected(&[
        "review",
        "--request",
        MISSING_REQUEST,
        "--fixture",
        "double-submit",
    ]);
    assert_rejected(&["review", "--fixture", "double-submit"]);
    assert_rejected(&[
        "review",
        "--request",
        MISSING_REQUEST,
        "--artifacts",
        MISSING_ARTIFACTS,
        "--verbose",
    ]);
}

#[test]
fn flag_values_are_not_reinterpreted_as_flags() {
    // `--artifacts` appearing as the *value* of `--request` is still a
    // complete, accepted pair; the parser must consume values positionally
    // rather than scanning for flag-looking tokens.
    assert_accepted(&[
        "review",
        "--request",
        "--artifacts",
        "--artifacts",
        MISSING_ARTIFACTS,
    ]);
}
