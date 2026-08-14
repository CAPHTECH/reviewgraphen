// SPDX-License-Identifier: Apache-2.0

//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]
//! [benchmark issue metadata redacted]

use serde_json::Value;

/// The two classes the Verdict Conservation Law is stated over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutcomeClass {
    /// Exit zero is permitted.
    Success,
    /// Exit zero is forbidden.
    Failure,
}

impl OutcomeClass {
    /// Whether this class permits a zero exit status.
    #[must_use]
    pub fn is_success(self) -> bool {
        matches!(self, Self::Success)
    }
}

/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
#[must_use]
#[allow(clippy::match_same_arms)]
pub fn outcome_class(output: &Value) -> OutcomeClass {
    let Some(result) = output.get("result").and_then(Value::as_str) else {
        // No `result` field at all is not a passing verdict. `chain` layer
        // entries and the raw `fmt` source path are the only outputs without
        // one, and neither is classified here.
        return OutcomeClass::Failure;
    };
    match result {
        // ---- Values whose verdict lives in a sibling field ---------------
        //
        // These are why this function takes `&Value`. Each mirrors the exit
        // code its producing site computes today.

        // [benchmark issue metadata redacted]
        // [benchmark issue metadata redacted]
        // [benchmark issue metadata redacted]
        // [benchmark issue metadata redacted]
        "approval_check" => {
            class_of(output.get("status").and_then(Value::as_str) != Some("signature-invalid"))
        }
        // `main.rs` `run_fmt_check`: `i32::from(any_changed)`.
        "format_check" => class_of(output.get("changed") != Some(&Value::Bool(true))),
        // `main.rs` `run_lint`: `i32::from(finding_count > 0)`.
        "lint" => class_of(
            output
                .get("finding_count")
                .and_then(Value::as_u64)
                .is_none_or(|count| count == 0),
        ),
        // `main.rs` `run_diff`: `i32::from(!violations.is_empty())`.
        "semantic_diff" => class_of(
            output
                .get("violations")
                .and_then(Value::as_array)
                .is_none_or(Vec::is_empty),
        ),
        // `main.rs` `run_diff_git` batch: the same gate, aggregated. The
        // envelope publishes the decision as `gate.passed`.
        "semantic_diff_batch" => class_of(
            output
                .get("gate")
                .and_then(|gate| gate.get("passed"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ),

        // ---- Success class ----------------------------------------------
        //
        // Every value below exits 0 at its producing site.

        // Core verification and checking verdicts.
        "ok"
        | "verified"
        | "proved"
        | "refines"
        | "conformant"
        // Bounded sweep grid over a clean spec.
        | "sweep_passed"
        // Derived artifacts: `ledger`, `testgen`, `html`, `document`,
        // `domain generate`, `db import`, `approval record`.
        | "generated"
        | "created"
        | "imported"
        | "imported_with_warnings"
        // Analyses and projections that either succeed or error out.
        | "analyzed"
        | "expanded"
        | "explained"
        | "kernel"
        | "typestate"
        | "scenarios"
        | "mutated"
        | "migrated"
        | "compared"
        | "compat_profile_generated"
        // Conformance projections and replay evidence.
        | "conformance"
        | "conformance_coverage"
        | "testgen_trace"
        | "conformance_checked"
        | "document_conformant"
        | "observed_conformant"
        | "replay_conformant"
        // [benchmark issue metadata redacted]
        // [benchmark issue metadata redacted]
        | "observed_supported"
        // Specialized dialects: a clean verdict under the declared
        // assumptions (`fsl-tools` `ai.rs`, `domain.rs`, `db.rs`, `agent.rs`).
        | "verified_under_assumptions"
        | "agent_analyzed"
        | "ai_project_analyzed"
        // Causal dialect (`fsl-tools/src/causal_analysis.rs`, `causal.rs`).
        | "causal_analyzed"
        | "causal_model_checked"
        | "causal_diffed"
        | "causal_ledger"
        | "causal_expectations_checked"
        | "causal_expectations_observed"
        // `fsl-ai` statistical evaluation that met its requirement.
        | "statistically_supported"
        // A `chain` layer the manifest did not request. Its own entry
        // declares `exit_code: 0` (`main.rs` `skipped_layer_entry`).
        | "skipped" => OutcomeClass::Success,

        // ---- Failure class ----------------------------------------------
        //
        // Listed explicitly rather than left to the `_` arm so that a typo in
        // one of these names surfaces as an unregistered value rather than
        // silently landing in the same class by accident.

        // Spec/usage/internal errors. The *code* (2 vs 3) is chosen by the
        // caller; the class is the same.
        "error"
        // Kernel verification verdicts that are not a pass.
        | "violated"
        | "reachable_failed"
        | "unknown_cti"
        | "unknown_budget"
        // Refinement, conformance, and sweep failures.
        | "refinement_failed"
        | "nonconformant"
        | "impl_violated"
        | "sweep_failed"
        // Dialect-level failures.
        | "observed_mismatch"
        | "replay_nonconformant"
        | "document_drifted"
        | "migration_refused"
        // `approval diff` only ever publishes this `result` on its
        // signature-invalid path (`main.rs` `run_approval_diff`); a valid
        // signature returns the underlying `semantic_diff` envelope instead.
        | "approval_diff"
        // [benchmark issue metadata redacted]
        // [benchmark issue metadata redacted]
        | "statistically_unsupported"
        | "dataset_invalid"
        | "evaluator_untrusted"
        | "slice_missing"
        | "insufficient_samples"
        | "inconclusive"
        // A refinement mapping the auto-mapper could not decide.
        | "unknown" => OutcomeClass::Failure,

        // [benchmark issue metadata redacted]
        // [benchmark issue metadata redacted]
        // [benchmark issue metadata redacted]
        _ => OutcomeClass::Failure,
    }
}

fn class_of(success: bool) -> OutcomeClass {
    if success {
        OutcomeClass::Success
    } else {
        OutcomeClass::Failure
    }
}

/// Whether a `verify` envelope is a settled verdict worth writing to the
/// on-disk verification cache.
///
/// **This is not [`outcome_class`] and must not be folded into it.**
/// Cacheability asks "is this verdict settled enough to replay", not "did it
/// pass": `violated`, `reachable_failed`, `unknown_cti`, and `unknown_budget`
/// are all failure-class *and* cacheable. The two predicates live in the same
/// file so the vocabulary has one home, and stay distinct so that a new result
/// value forces an explicit decision in both.
#[must_use]
pub fn verify_cache_admits(output: &Value) -> bool {
    matches!(
        output.get("result").and_then(Value::as_str),
        Some(
            "verified"
                | "proved"
                | "violated"
                | "reachable_failed"
                | "unknown_cti"
                | "unknown_budget"
        )
    )
}

/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
#[must_use]
pub fn is_definitive_kernel_verdict(status: i32) -> bool {
    status == 0 || status == 1
}

/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
/// [benchmark issue metadata redacted]
#[must_use]
pub fn exit_status(output: &Value, error_status: i32) -> i32 {
    // Row 0 of the table is exactly the success class; that is the part this
    // function no longer restates.
    if outcome_class(output).is_success() {
        return 0;
    }
    match output.get("result").and_then(Value::as_str) {
        Some("error") => error_status,
        // Row 1, restricted to the members a baseline `verify` envelope can
        // actually carry. The row's remaining members (`nonconformant`,
        // `refinement_failed`, `sweep_failed`, `observed_mismatch`) belong to
        // other commands and cannot appear here.
        Some("violated" | "reachable_failed" | "unknown_cti" | "unknown_budget") => 1,
        // A failure-class value outside this command's vocabulary, or one
        // nobody registered at all, is an internal inconsistency -- never a
        // silent success.
        _ => 3,
    }
}

// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
// [benchmark test code redacted]
