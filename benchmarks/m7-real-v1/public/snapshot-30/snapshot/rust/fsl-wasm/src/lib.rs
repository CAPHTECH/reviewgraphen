// SPDX-License-Identifier: Apache-2.0

//! WASM entry point used exclusively inside the browser verification Worker.

use std::collections::BTreeMap;

use fsl_core::{CoreError, FileResolver, KernelModel, model_warnings};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = performance, js_name = now)]
    fn performance_now() -> f64;
}

#[derive(Debug, Deserialize)]
struct Request {
    cmd: String,
    source: String,
    #[serde(default = "default_source_file")]
    source_file: String,
    #[serde(default)]
    files: BTreeMap<String, String>,
    #[serde(default)]
    options: Options,
}

fn default_source_file() -> String {
    "spec.fsl".to_owned()
}

#[derive(Debug, Deserialize)]
struct Options {
    #[serde(default = "default_depth")]
    depth: usize,
    #[serde(default = "default_deadlock")]
    deadlock: String,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            depth: default_depth(),
            deadlock: default_deadlock(),
        }
    }
}

const fn default_depth() -> usize {
    8
}

fn default_deadlock() -> String {
    "warn".to_owned()
}

struct MemoryResolver {
    files: BTreeMap<String, String>,
}

impl FileResolver for MemoryResolver {
    fn read(&self, path: &str) -> Result<String, CoreError> {
        self.files.get(path).cloned().ok_or_else(|| CoreError {
            message: format!("file not found: {path}"),
            line: 1,
            column: 1,
            origin: None,
        })
    }
}

fn envelope(solver_version: &str) -> Map<String, Value> {
    let mut output = Map::new();
    output.insert("fsl".to_owned(), json!("1.0"));
    output.insert(
        "versions".to_owned(),
        fsl_core::version_metadata(
            "fsl-wasm",
            env!("CARGO_PKG_VERSION"),
            "z3-solver-wasm",
            solver_version,
        ),
    );
    output
}

fn error(solver_version: &str, kind: &str, message: impl AsRef<str>) -> Value {
    let mut output = envelope(solver_version);
    output.insert("result".to_owned(), json!("error"));
    output.insert("kind".to_owned(), json!(kind));
    output.insert("message".to_owned(), json!(message.as_ref()));
    Value::Object(output)
}

fn implements_error(
    solver_version: &str,
    failure: &fslc_rust::verification_output::RequirementsImplementsError,
) -> Value {
    let mut output = error(solver_version, "type", &failure.message);
    if let Some(span) = failure.span
        && let Some(object) = output.as_object_mut()
    {
        object.insert("loc".to_owned(), span.python_loc());
        object.insert("span".to_owned(), json!(span));
    }
    output
}

fn build(request: &Request, solver_version: &str) -> Result<(KernelModel, Vec<Value>), Value> {
    let resolver = MemoryResolver {
        files: request.files.clone(),
    };
    let kernel =
        fsl_core::parse_kernel_source_with_file(&request.source, &resolver, &request.source_file)
            .map_err(|failure| error(solver_version, "parse", failure.to_string()))?;
    // Compose-lowering warnings (e.g. `fair_not_inherited`) are computed while
    // lowering, before `build_model` drops the per-component information that
    // produced them, so they must be captured here rather than derived from
    // the checked KernelModel below.
    let diagnostics = kernel.diagnostics().to_vec();
    let model = fsl_core::build_model(kernel).map_err(|failure| {
        fslc_rust::verification_output::render_semantic_error(
            envelope(solver_version),
            &failure.to_string(),
        )
    })?;
    Ok((model, diagnostics))
}

async fn check(request: &Request, solver_version: &str) -> Value {
    if let Some((output, _)) = fslc_rust::frontend_output::ai_project_check_output(
        &request.source,
        &request.source_file,
        envelope(solver_version),
    ) {
        return output;
    }
    if let Err(failure) = fsl_syntax::parse_document(fsl_syntax::SourceFile::new(&request.source)) {
        return fslc_rust::frontend_output::render_surface_parse_error(
            envelope(solver_version),
            &failure,
        );
    }
    let (model, compose_warnings) = match build(request, solver_version) {
        Ok(built) => built,
        Err(error) => return error,
    };
    let has_trace_contract = match fslc_rust::verification_output::validate_requirement_trace_source(
        &envelope(solver_version),
        &request.source,
        &model,
    ) {
        Ok((Some(failure), _)) => return failure,
        Ok((None, has_contract)) => has_contract,
        Err(failure) => return error(solver_version, "semantics", failure),
    };
    let mut output = envelope(solver_version);
    output.insert("result".to_owned(), json!("ok"));
    output.insert("spec".to_owned(), json!(model.name));
    let warnings = compose_warnings
        .into_iter()
        .chain(model_warnings(&model))
        .collect::<Vec<_>>();
    output.insert("warnings".to_owned(), Value::Array(warnings));
    let mut output = add_frontend_metadata(
        request,
        solver_version,
        &model,
        has_trace_contract,
        8,
        Value::Object(output),
    );
    match governance_output(request).await {
        Ok(Some(governance)) => {
            output
                .as_object_mut()
                .expect("check envelope")
                .insert("governance".to_owned(), governance);
        }
        Ok(None) => {}
        Err(failure) => {
            return fslc_rust::verification_output::render_governance_error(
                envelope(solver_version),
                &failure,
            );
        }
    }
    output
}

async fn governance_output(
    request: &Request,
) -> Result<Option<Value>, fslc_rust::verification_output::GovernanceOutputError> {
    let resolver = MemoryResolver {
        files: request.files.clone(),
    };
    let resolver_ref = &resolver;
    fslc_rust::verification_output::governance_output_async(
        &request.source,
        resolver_ref,
        |preservation| {
            let preservation = preservation.clone();
            let resolver = resolver_ref;
            async move {
                let implementation_source = resolver
                    .read(&preservation.after_path)
                    .map_err(|failure| governance_error(failure.to_string(), preservation.span))?;
                let abstraction_source = resolver
                    .read(&preservation.before_path)
                    .map_err(|failure| governance_error(failure.to_string(), preservation.span))?;
                let mapping_source = resolver
                    .read(&preservation.refinement_path)
                    .map_err(|failure| governance_error(failure.to_string(), preservation.span))?;
                let implementation = fsl_core::build_model(
                    fsl_core::parse_kernel_source_with_file(
                        &implementation_source,
                        resolver,
                        &preservation.after_path,
                    )
                    .map_err(|failure| governance_error(failure.to_string(), preservation.span))?,
                )
                .map_err(|failure| governance_error(failure.to_string(), preservation.span))?;
                let abstraction = fsl_core::build_model(
                    fsl_core::parse_kernel_source_with_file(
                        &abstraction_source,
                        resolver,
                        &preservation.before_path,
                    )
                    .map_err(|failure| governance_error(failure.to_string(), preservation.span))?,
                )
                .map_err(|failure| governance_error(failure.to_string(), preservation.span))?;
                let mapping =
                    fsl_core::parse_refinement(&mapping_source, &implementation, &abstraction)
                        .map_err(|failure| governance_error(failure.message, preservation.span))?;
                let checked =
                    fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 8)
                        .map_err(|failure| {
                            governance_error(failure.to_string(), preservation.span)
                        })?;
                if checked.failure.is_some() {
                    return Ok(json!("refinement_failed"));
                }
                if !mapping.progress.is_empty() {
                    let mut solver = fsl_solver_z3js::Z3JsSolver::new();
                    let progress = fsl_verifier::check_refinement_progress(
                        &implementation,
                        &abstraction,
                        &mapping,
                        &mut solver,
                        8,
                    )
                    .await
                    .map_err(|failure| governance_error(failure.to_string(), preservation.span))?;
                    if progress.violation.is_some() {
                        return Ok(json!("refinement_failed"));
                    }
                }
                Ok(json!(if checked.failure.is_some() {
                    "refinement_failed"
                } else {
                    "refines"
                }))
            }
        },
    )
    .await
}

fn governance_error(
    message: impl Into<String>,
    span: fsl_syntax::Span,
) -> fslc_rust::verification_output::GovernanceOutputError {
    fslc_rust::verification_output::GovernanceOutputError::new(
        message,
        span.start.line,
        span.start.column,
    )
}

fn remove_generic_invariant_warning(output: &mut Value) {
    if let Some(warnings) = output.get_mut("warnings").and_then(Value::as_array_mut) {
        warnings.retain(|warning| {
            warning.get("message").and_then(Value::as_str)
                != Some("spec declares no user invariants (only implicit type bounds are checked)")
        });
    }
}

fn add_frontend_metadata(
    request: &Request,
    solver_version: &str,
    model: &KernelModel,
    has_trace_contract: bool,
    depth: usize,
    mut output: Value,
) -> Value {
    if has_trace_contract {
        remove_generic_invariant_warning(&mut output);
    }
    let resolver = MemoryResolver {
        files: request.files.clone(),
    };
    match fslc_rust::verification_output::requirements_implements_output(
        &request.source,
        &resolver,
        model,
        depth,
    ) {
        Ok(Some(implements)) => {
            output
                .as_object_mut()
                .expect("verify envelope")
                .insert("implements".to_owned(), implements);
            remove_generic_invariant_warning(&mut output);
        }
        Ok(None) => {}
        Err(failure) => return implements_error(solver_version, &failure),
    }
    let additions = fslc_rust::frontend_output::implicit_initial_value_warnings(
        &request.source,
        &request.source_file,
    );
    if !additions.is_empty() {
        output
            .as_object_mut()
            .expect("verify envelope")
            .entry("warnings")
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .expect("warnings array")
            .extend(additions);
    }
    output
}

async fn verify(request: &Request, solver_version: &str) -> Value {
    let started = performance_now();
    if let Err(failure) = fsl_syntax::parse_surface_document(&request.source) {
        // [benchmark issue metadata redacted]
        // [benchmark issue metadata redacted]
        // [benchmark issue metadata redacted]
        return fslc_rust::frontend_output::render_surface_parse_error(
            envelope(solver_version),
            &failure,
        );
    }
    let (model, compose_warnings) = match build(request, solver_version) {
        Ok(built) => built,
        Err(error) => return error,
    };
    let has_trace_contract = match fslc_rust::verification_output::validate_requirement_trace_source(
        &envelope(solver_version),
        &request.source,
        &model,
    ) {
        Ok((Some(failure), _)) => return failure,
        Ok((None, has_contract)) => has_contract,
        Err(failure) => return error(solver_version, "semantics", failure),
    };
    let deadlock =
        match fslc_rust::verification_output::DeadlockMode::parse(&request.options.deadlock) {
            Ok(deadlock) => deadlock,
            Err(message) => return error(solver_version, "usage", message),
        };
    // [benchmark issue metadata redacted]
    // [benchmark issue metadata redacted]
    // [benchmark issue metadata redacted]
    // [benchmark issue metadata redacted]
    // [benchmark issue metadata redacted]
    // [benchmark issue metadata redacted]
    if fsl_runtime::deterministic_initial_state(&model).is_ok() {
        match fsl_runtime::find_boundary_violation(model.clone(), request.options.depth) {
            Ok(Some((violation, trace))) => {
                let statistics = fsl_solver::VerificationStatistics::default();
                return fslc_rust::verification_output::render_boundary_output(
                    envelope(solver_version),
                    &model,
                    &violation,
                    &trace,
                    &fslc_rust::verification_output::BmcOutputOptions {
                        depth: request.options.depth,
                        deadlock,
                        checked_bounds: None,
                        elapsed_s: (performance_now() - started) / 1000.0,
                        statistics: &statistics,
                    },
                )
                .0;
            }
            Ok(None) => {}
            Err(failure) => {
                return fslc_rust::verification_output::render_semantic_error(
                    envelope(solver_version),
                    &failure.to_string(),
                );
            }
        }
    }
    let mut solver = fsl_solver_z3js::Z3JsSolver::new();
    let result =
        match fsl_verifier::verify_bounded(&model, &mut solver, request.options.depth).await {
            Ok(result) => result,
            Err(failure) => {
                return fslc_rust::verification_output::render_semantic_error(
                    envelope(solver_version),
                    &failure.to_string(),
                );
            }
        };
    if let Err(failure) =
        fslc_rust::verification_output::replay_bmc_witnesses(&model, &result, None)
    {
        return error(solver_version, "internal", failure);
    }
    let statistics = fsl_solver::SmtSolver::statistics(&solver);
    let (mut output, _) = fslc_rust::verification_output::render_bmc_output(
        envelope(solver_version),
        &model,
        &result,
        fslc_rust::verification_output::BmcOutputOptions {
            depth: request.options.depth,
            deadlock,
            checked_bounds: None,
            elapsed_s: (performance_now() - started) / 1000.0,
            statistics: &statistics,
        },
    );
    if !compose_warnings.is_empty()
        && let Some(object) = output.as_object_mut()
        && object.get("result").and_then(Value::as_str) != Some("error")
        && let Some(Value::Array(warnings)) = object.get_mut("warnings")
    {
        // See the matching comment in native `run_verify` (rust/fslc/src/main.rs):
        // compose-lowering warnings must be captured before `build_model` drops
        // per-component fairness information, so they cannot come from `model`.
        warnings.splice(0..0, compose_warnings);
    }
    add_frontend_metadata(
        request,
        solver_version,
        &model,
        has_trace_contract,
        request.options.depth,
        output,
    )
}

/// Execute one Worker request and return the stable JSON envelope as text.
///
/// # Panics
///
/// Panics only if an in-memory `serde_json::Value` cannot be serialized.
#[wasm_bindgen]
pub async fn run(request_json: String) -> String {
    let solver_version = fsl_solver_z3js::version();
    let request = match serde_json::from_str::<Request>(&request_json) {
        Ok(request) => request,
        Err(failure) => {
            return error(
                &solver_version,
                "io",
                format!("invalid request JSON: {failure}"),
            )
            .to_string();
        }
    };
    let output = match request.cmd.as_str() {
        "check" => check(&request, &solver_version).await,
        "verify" => verify(&request, &solver_version).await,
        command => error(
            &solver_version,
            "usage",
            format!("command '{command}' is not available in the browser Worker"),
        ),
    };
    serde_json::to_string_pretty(&output).expect("JSON values serialize")
}

/// Render an internal verifier error after the Worker solver runtime initialized.
///
/// # Panics
///
/// Panics only if an in-memory `serde_json::Value` cannot be serialized.
#[wasm_bindgen]
#[must_use]
pub fn internal_error(message: String) -> String {
    let output = error(&fsl_solver_z3js::version(), "internal", message);
    serde_json::to_string_pretty(&output).expect("JSON values serialize")
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
