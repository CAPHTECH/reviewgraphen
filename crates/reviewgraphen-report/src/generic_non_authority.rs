//! Deterministic, non-authority human projections of generic run-v2 audit JSON.
//!
//! This module consumes a validated canonical audit document and produces only
//! a closed manifest and Markdown rendering. Neither output is an input to an
//! authority-bearing API.

use reviewgraphen_core::{ContentHash, canonical_json};
use reviewgraphen_runtime::generic::{
    ValidatedGenericReviewRunV3, decode_and_validate_generic_review_run_v2,
};
use serde_json::{Map, Value, json};
use thiserror::Error;

const HUMAN_REPORT_SCHEMA: &str = "reviewgraphen.generic_review_human_report.v1";
const HUMAN_REPORT_V2_SCHEMA: &str = "reviewgraphen.generic_review_human_report.v2";
const MAX_RENDERED_MARKDOWN_BYTES: usize = 1_048_576;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenericHumanReport {
    pub manifest_bytes: Vec<u8>,
    pub markdown_bytes: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum GenericHumanReportError {
    #[error("generic human report audit is not canonical JSON")]
    NonCanonicalAudit,
    #[error("generic human report audit is missing {0}")]
    Missing(&'static str),
    #[error("generic human report audit has an invalid {0}")]
    Invalid(&'static str),
    #[error("generic human report audit has an invalid {name}: {detail}")]
    SchemaViolation { name: &'static str, detail: String },
    #[error("generic human report rendering exceeds {limit} bytes (observed {observed})")]
    RenderBound { limit: usize, observed: usize },
    #[error("generic human report JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("generic human report canonical JSON failed: {0}")]
    Core(#[from] reviewgraphen_core::DomainError),
    #[error("generic human report audit validation failed: {0}")]
    Audit(#[from] reviewgraphen_runtime::generic::GenericReviewError),
    #[error("generic human report manifest does not match its audit projection")]
    ManifestMismatch,
    #[error("generic human report Markdown does not match its manifest projection")]
    MarkdownMismatch,
}

/// Projects one canonical, validated generic run-v2 audit document.
pub fn generate_generic_human_report(
    audit_bytes: &[u8],
) -> Result<GenericHumanReport, GenericHumanReportError> {
    let audit = decode_and_validate_generic_review_run_v2(audit_bytes)?;
    if canonical_json(&audit)? != audit_bytes {
        return Err(GenericHumanReportError::NonCanonicalAudit);
    }
    validate_run_schema(&audit)?;
    build_report(audit_bytes, &audit)
}

/// Projects one basis-bound, semantically validated generic run-v3.
///
/// This is intentionally a separate public entry point from
/// [`generate_generic_human_report`]: each report major has exactly one run
/// major and neither entry point performs an upcast, downcast, or fallback.
/// A bytes-only wire value cannot cross this capability boundary:
///
/// ```compile_fail
/// use reviewgraphen_report::generate_generic_human_report_v3;
/// use reviewgraphen_runtime::generic::UnvalidatedGenericReviewRunV3;
///
/// fn cannot_report(wire: &UnvalidatedGenericReviewRunV3) {
///     let _ = generate_generic_human_report_v3(wire);
/// }
/// ```
pub fn generate_generic_human_report_v3(
    run: &ValidatedGenericReviewRunV3,
) -> Result<GenericHumanReport, GenericHumanReportError> {
    let audit_bytes = run.canonical_bytes()?;
    let audit = run.value();
    validate_run_v3_schema(audit)?;
    build_report_v3(&audit_bytes, audit)
}

fn validate_run_schema(audit: &Value) -> Result<(), GenericHumanReportError> {
    validate_schema(
        audit,
        include_str!("../../../schemas/reviewgraphen.generic_review_run.v2.schema.json"),
        "run schema",
    )
}

fn validate_run_v3_schema(audit: &Value) -> Result<(), GenericHumanReportError> {
    validate_schema(
        audit,
        include_str!("../../../schemas/reviewgraphen.generic_review_run.v3.schema.json"),
        "run v3 schema",
    )
}

fn validate_human_report_schema(manifest: &Value) -> Result<(), GenericHumanReportError> {
    validate_schema(
        manifest,
        include_str!("../../../schemas/reviewgraphen.generic_review_human_report.v1.schema.json"),
        "human report schema",
    )
}

fn validate_human_report_v2_schema(manifest: &Value) -> Result<(), GenericHumanReportError> {
    validate_schema(
        manifest,
        include_str!("../../../schemas/reviewgraphen.generic_review_human_report.v2.schema.json"),
        "human report v2 schema",
    )
}

fn validate_schema(
    value: &Value,
    schema_bytes: &str,
    name: &'static str,
) -> Result<(), GenericHumanReportError> {
    let schema: Value = serde_json::from_str(schema_bytes)?;
    let validator =
        jsonschema::validator_for(&schema).map_err(|_| GenericHumanReportError::Invalid(name))?;
    match validator.iter_errors(value).next() {
        None => Ok(()),
        Some(error) => Err(GenericHumanReportError::SchemaViolation {
            name,
            detail: format!(
                "instance {} violates schema {}: {}",
                error.instance_path,
                error.schema_path,
                error.masked()
            ),
        }),
    }
}

/// Verifies a manifest and Markdown rendering by rebuilding both from the sole
/// canonical audit input. This is verification of a projection, never import.
pub fn validate_generic_human_report(
    audit_bytes: &[u8],
    manifest_bytes: &[u8],
    markdown_bytes: &[u8],
) -> Result<(), GenericHumanReportError> {
    let expected = generate_generic_human_report(audit_bytes)?;
    if expected.manifest_bytes != manifest_bytes {
        return Err(GenericHumanReportError::ManifestMismatch);
    }
    if expected.markdown_bytes != markdown_bytes {
        return Err(GenericHumanReportError::MarkdownMismatch);
    }
    Ok(())
}

/// Verifies a v2 manifest and Markdown rendering by regenerating both from a
/// basis-bound validated run-v3. It never parses Markdown as state.
pub fn validate_generic_human_report_v3(
    run: &ValidatedGenericReviewRunV3,
    manifest_bytes: &[u8],
    markdown_bytes: &[u8],
) -> Result<(), GenericHumanReportError> {
    let expected = generate_generic_human_report_v3(run)?;
    if expected.manifest_bytes != manifest_bytes {
        return Err(GenericHumanReportError::ManifestMismatch);
    }
    if expected.markdown_bytes != markdown_bytes {
        return Err(GenericHumanReportError::MarkdownMismatch);
    }
    Ok(())
}

fn build_report(
    audit_bytes: &[u8],
    audit: &Value,
) -> Result<GenericHumanReport, GenericHumanReportError> {
    let root = object(audit, "audit")?;
    let plan = object(field(root, "plan")?, "plan")?;
    let legacy = object(field(root, "legacy_ingestion")?, "legacy_ingestion")?;
    let coverage = object(field(root, "coverage")?, "coverage")?;
    let authority = object(field(root, "authority")?, "authority")?;
    let ingestion = object(field(root, "ingestion_report_v2")?, "ingestion_report_v2")?;
    let limitation = object(
        field(ingestion, "global_direct_calls_limitation")?,
        "limitation",
    )?;
    let source_occurrence_summaries = array(ingestion, "source_occurrence_summaries")?;

    let run_id = string(root, "run_id")?;
    let snapshot_id = string(legacy, "snapshot_id")?;
    let universe_id = string(plan, "universe_id")?;
    let audit_hash = ContentHash::sha256(audit_bytes).to_string();
    let observations = array(root, "observations")?;
    let contexts = array(root, "contexts")?;

    let mut proposed_claims = Vec::new();
    let mut abstentions = Vec::new();
    let mut malformed_outputs = Vec::new();
    let mut provider_failures = Vec::new();
    let mut source_ids = vec![Value::String(snapshot_id.to_owned())];
    let mut window_ids = Vec::new();
    let mut context_ids = Vec::new();

    extend_ids(&mut source_ids, array(limitation, "source_ids")?);
    for summary in source_occurrence_summaries {
        let summary = object(summary, "source occurrence summary")?;
        source_ids.push(Value::String(string(summary, "file_source_id")?.to_owned()));
    }
    for context in contexts {
        let context = object(context, "context")?;
        context_ids.push(Value::String(string(context, "context_id")?.to_owned()));
        extend_ids(&mut source_ids, array(context, "candidate_source_ids")?);
        extend_ids(&mut window_ids, array(context, "window_ids")?);
    }
    for observation in observations {
        let observation = object(observation, "observation")?;
        let obligation_id = string(observation, "obligation_id")?;
        let execution_id = string(observation, "execution_id")?;
        match string(observation, "kind")? {
            "proposed_claim" => {
                for proposal in array(observation, "proposals")? {
                    let proposal = object(proposal, "proposal")?;
                    let proposal_sources = array(proposal, "source_ids")?;
                    extend_ids(&mut source_ids, proposal_sources);
                    proposed_claims.push(json!({
                        "obligation_id": obligation_id,
                        "execution_id": execution_id,
                        "property_id": string(proposal, "property_id")?,
                        "target_refs": array(proposal, "target_refs")?,
                        "polarity": string(proposal, "polarity")?,
                        "summary": string(proposal, "summary")?,
                        "source_ids": proposal_sources,
                        "status": "proposed"
                    }));
                }
            }
            "deterministic_abstain" => abstentions.push(json!({
                "obligation_id": obligation_id,
                "execution_id": execution_id,
                "reason": string(observation, "reason")?,
                "detail": string(observation, "detail")?
            })),
            "malformed" => malformed_outputs.push(json!({
                "obligation_id": obligation_id,
                "execution_id": execution_id,
                "reason": string(observation, "reason")?,
                "diagnostic": string(observation, "diagnostic")?
            })),
            "provider_failure" => provider_failures.push(json!({
                "obligation_id": obligation_id,
                "execution_id": execution_id,
                "retryable": boolean(observation, "retryable")?,
                "diagnostic": string(observation, "diagnostic")?
            })),
            _ => return Err(GenericHumanReportError::Invalid("observation kind")),
        }
    }

    source_ids.sort_by(value_string_order);
    source_ids.dedup();
    window_ids.sort_by(value_string_order);
    window_ids.dedup();
    context_ids.sort_by(value_string_order);
    context_ids.dedup();

    let verifier = match field(root, "verifier")? {
        Value::Null => json!({"status": "not_selected"}),
        value => {
            let verifier = object(value, "verifier")?;
            json!({
                "status": "unsupported",
                "id": string(verifier, "id")?,
                "descriptor_id": string(verifier, "descriptor_id")?,
                "reason": string(verifier, "reason")?,
                "process_started": boolean(verifier, "process_started")?,
                "executable_resolved": boolean(verifier, "executable_resolved")?
            })
        }
    };

    let mut manifest = json!({
        "schema": HUMAN_REPORT_SCHEMA,
        "audit": {
            "schema": string(root, "schema")?,
            "run_id": run_id,
            "snapshot_id": snapshot_id,
            "universe_id": universe_id,
            "canonical_sha256": audit_hash
        },
        "authority": {
            "classification": string(authority, "classification")?,
            "trusted_pass": boolean(authority, "trusted_pass")?,
            "result_status": string(authority, "result_status")?,
            "incomplete_reasons": array(authority, "incomplete_reasons")?
        },
        "coverage": {
            "resolved_target_obligation_ids": array(coverage, "resolved_target_obligation_ids")?,
            "stage_ids": plan_stage_ids(plan)?,
            "candidate_space": {
                "capabilities": field(coverage, "enumeration_capability_states")?,
                "limitation_ids": array(coverage, "enumeration_limitation_ids")?,
                "gap_obligation_ids": array(coverage, "candidate_space_gap_obligation_ids")?,
                "obstruction_summary_ids": array(coverage, "enumeration_obstruction_summary_ids")?,
                "obstruction_ids": array(coverage, "enumeration_obstruction_ids")?,
                "observed_unresolved_call_occurrence_count": unsigned(coverage, "observed_unresolved_call_occurrence_count")?,
                "occurrence_id_set_sha256": string(coverage, "occurrence_id_set_sha256")?,
                "source_occurrence_summaries": source_occurrence_summaries,
                "global_direct_calls_limitation": field(ingestion, "global_direct_calls_limitation")?,
                "call_graph_complete": boolean(coverage, "call_graph_complete")?,
                "global_call_coverage_claim": string(coverage, "global_call_coverage_claim")?
            }
        },
        "proposed_claims": proposed_claims,
        "abstentions": abstentions,
        "malformed_outputs": malformed_outputs,
        "provider_failures": provider_failures,
        "verifier": verifier,
        "exclusions": [],
        "deferrals": array(coverage, "deferred_obligation_ids")?,
        "source_window_trace": {
            "source_ids": source_ids,
            "window_ids": window_ids,
            "context_ids": context_ids
        },
        "information_loss": [{
            "kind": "bounded_audit_reduction",
            "meaningful": true,
            "reason": "Raw observer responses, full obligation contracts, and unrendered audit details are omitted from this human projection.",
            "recovery_ref": audit_hash,
            "source_ids": [run_id]
        }]
    });
    let markdown_bytes = render_markdown(&manifest)?;
    object_mut(&mut manifest, "manifest")?.insert(
        "rendered_markdown_sha256".to_owned(),
        Value::String(ContentHash::sha256(&markdown_bytes).to_string()),
    );
    validate_human_report_schema(&manifest)?;
    let manifest_bytes = canonical_json(&manifest)?;
    Ok(GenericHumanReport {
        manifest_bytes,
        markdown_bytes,
    })
}

fn build_report_v3(
    audit_bytes: &[u8],
    audit: &Value,
) -> Result<GenericHumanReport, GenericHumanReportError> {
    let root = object(audit, "audit")?;
    let plan = object(field(root, "plan")?, "plan")?;
    let legacy = object(field(root, "legacy_ingestion")?, "legacy_ingestion")?;
    let coverage = object(field(root, "coverage")?, "coverage")?;
    let authority = object(field(root, "authority")?, "authority")?;
    let ingestion = object(field(root, "ingestion_report_v2")?, "ingestion report")?;
    let limitation = object(
        field(ingestion, "global_direct_calls_limitation")?,
        "global direct-calls limitation",
    )?;
    let source_occurrence_summaries = array(ingestion, "source_occurrence_summaries")?;
    let run_id = string(root, "run_id")?;
    let snapshot_id = string(legacy, "snapshot_id")?;
    let universe_id = string(plan, "universe_id")?;
    let audit_hash = ContentHash::sha256(audit_bytes).to_string();

    let mut source_ids = vec![Value::String(snapshot_id.to_owned())];
    let mut window_ids = Vec::new();
    let mut context_ids = Vec::new();
    extend_ids(&mut source_ids, array(limitation, "source_ids")?);
    for summary in source_occurrence_summaries {
        let summary = object(summary, "source occurrence summary")?;
        source_ids.push(Value::String(string(summary, "file_source_id")?.to_owned()));
    }

    let mut contexts = Vec::new();
    for row in array(root, "contexts")? {
        let row = object(row, "v3 context row")?;
        let context = object(field(row, "context")?, "v3 context")?;
        let policy = object(field(context, "context_policy")?, "v3 context policy")?;
        let materialized_sources = array(context, "materialized_sources")?;
        let windows = array(context, "windows")?;
        let unknowns = array(context, "unknowns")?;
        for source in materialized_sources {
            let source = object(source, "materialized source")?;
            source_ids.push(Value::String(string(source, "artifact_id")?.to_owned()));
        }
        for window in windows {
            let window = object(window, "admitted window")?;
            window_ids.push(Value::String(string(window, "id")?.to_owned()));
            source_ids.push(Value::String(
                string(window, "source_artifact_id")?.to_owned(),
            ));
        }
        for unknown in unknowns {
            let unknown = object(unknown, "v3 context unknown")?;
            extend_ids(&mut source_ids, array(unknown, "source_ids")?);
        }
        let context_id = string(context, "context_id")?;
        context_ids.push(Value::String(context_id.to_owned()));
        contexts.push(json!({
            "wave_id": string(row, "wave_id")?,
            "context_id": context_id,
            "context_policy_id": string(policy, "policy_id")?,
            "context_policy_hash": string(context, "context_policy_hash")?,
            "denominator_commitments": {
                "accepted_file": field(context, "accepted_file_denominator")?,
                "reached_file": field(context, "reached_file_denominator")?,
                "materialized_source": field(context, "materialized_source_denominator")?,
                "support_anchor": field(context, "support_anchor_denominator")?
            },
            "latent_cardinality": field(context, "latent_cardinality")?,
            "subject_outcomes": array(context, "subject_outcomes")?,
            "admitted_windows": windows,
            "support_loss_summaries": array(context, "support_loss_summaries")?,
            "unknowns": unknowns
        }));
    }

    let (proposed_claims, abstentions, malformed_outputs, provider_failures) =
        project_observations(root, &mut source_ids)?;
    source_ids.sort_by(value_string_order);
    source_ids.dedup();
    window_ids.sort_by(value_string_order);
    window_ids.dedup();
    context_ids.sort_by(value_string_order);
    context_ids.dedup();

    let verifier = project_verifier(root)?;
    let mut manifest = json!({
        "schema": HUMAN_REPORT_V2_SCHEMA,
        "audit": {
            "schema": string(root, "schema")?,
            "run_id": run_id,
            "snapshot_id": snapshot_id,
            "universe_id": universe_id,
            "canonical_sha256": audit_hash
        },
        "authority": {
            "classification": string(authority, "classification")?,
            "trusted_pass": boolean(authority, "trusted_pass")?,
            "result_status": string(authority, "result_status")?,
            "incomplete_reasons": array(authority, "incomplete_reasons")?
        },
        "coverage": {
            "resolved_target_obligation_ids": array(coverage, "resolved_target_obligation_ids")?,
            "stage_ids": plan_stage_ids(plan)?,
            "candidate_space": {
                "capabilities": field(coverage, "enumeration_capability_states")?,
                "limitation_ids": array(coverage, "enumeration_limitation_ids")?,
                "gap_obligation_ids": array(coverage, "candidate_space_gap_obligation_ids")?,
                "obstruction_summary_ids": array(coverage, "enumeration_obstruction_summary_ids")?,
                "obstruction_ids": array(coverage, "enumeration_obstruction_ids")?,
                "observed_unresolved_call_occurrence_count": unsigned(coverage, "observed_unresolved_call_occurrence_count")?,
                "occurrence_id_set_sha256": string(coverage, "occurrence_id_set_sha256")?,
                "source_occurrence_summaries": source_occurrence_summaries,
                "global_direct_calls_limitation": field(ingestion, "global_direct_calls_limitation")?,
                "call_graph_complete": boolean(coverage, "call_graph_complete")?,
                "global_call_coverage_claim": string(coverage, "global_call_coverage_claim")?
            }
        },
        "contexts": contexts,
        "proposed_claims": proposed_claims,
        "abstentions": abstentions,
        "malformed_outputs": malformed_outputs,
        "provider_failures": provider_failures,
        "provider_free_packet_bindings": array(root, "provider_free_packet_bindings")?,
        "verifier": verifier,
        "exclusions": [],
        "deferrals": array(coverage, "deferred_obligation_ids")?,
        "source_window_trace": {
            "source_ids": source_ids,
            "window_ids": window_ids,
            "context_ids": context_ids
        },
        "information_loss": [{
            "kind": "bounded_audit_reduction",
            "meaningful": true,
            "reason": "Raw observer responses, full obligation contracts, and unrendered audit details are omitted from this human projection.",
            "recovery_ref": audit_hash,
            "source_ids": [run_id]
        }]
    });
    let markdown_bytes = render_markdown_v3(&manifest)?;
    object_mut(&mut manifest, "manifest")?.insert(
        "rendered_markdown_sha256".to_owned(),
        Value::String(ContentHash::sha256(&markdown_bytes).to_string()),
    );
    validate_human_report_v2_schema(&manifest)?;
    Ok(GenericHumanReport {
        manifest_bytes: canonical_json(&manifest)?,
        markdown_bytes,
    })
}

type ProjectedOutcomes = (Vec<Value>, Vec<Value>, Vec<Value>, Vec<Value>);

fn project_observations(
    root: &Map<String, Value>,
    source_ids: &mut Vec<Value>,
) -> Result<ProjectedOutcomes, GenericHumanReportError> {
    let mut proposed_claims = Vec::new();
    let mut abstentions = Vec::new();
    let mut malformed_outputs = Vec::new();
    let mut provider_failures = Vec::new();
    for observation in array(root, "observations")? {
        let observation = object(observation, "observation")?;
        let obligation_id = string(observation, "obligation_id")?;
        let execution_id = string(observation, "execution_id")?;
        match string(observation, "kind")? {
            "proposed_claim" => {
                for proposal in array(observation, "proposals")? {
                    let proposal = object(proposal, "proposal")?;
                    let proposal_sources = array(proposal, "source_ids")?;
                    extend_ids(source_ids, proposal_sources);
                    proposed_claims.push(json!({
                        "obligation_id": obligation_id,
                        "execution_id": execution_id,
                        "property_id": string(proposal, "property_id")?,
                        "target_refs": array(proposal, "target_refs")?,
                        "polarity": string(proposal, "polarity")?,
                        "summary": string(proposal, "summary")?,
                        "source_ids": proposal_sources,
                        "status": "proposed"
                    }));
                }
            }
            "deterministic_abstain" => abstentions.push(json!({
                "obligation_id": obligation_id,
                "execution_id": execution_id,
                "reason": string(observation, "reason")?,
                "detail": string(observation, "detail")?
            })),
            "malformed" => malformed_outputs.push(json!({
                "obligation_id": obligation_id,
                "execution_id": execution_id,
                "reason": string(observation, "reason")?,
                "diagnostic": string(observation, "diagnostic")?
            })),
            "provider_failure" => provider_failures.push(json!({
                "obligation_id": obligation_id,
                "execution_id": execution_id,
                "retryable": boolean(observation, "retryable")?,
                "diagnostic": string(observation, "diagnostic")?
            })),
            _ => return Err(GenericHumanReportError::Invalid("observation kind")),
        }
    }
    Ok((
        proposed_claims,
        abstentions,
        malformed_outputs,
        provider_failures,
    ))
}

fn project_verifier(root: &Map<String, Value>) -> Result<Value, GenericHumanReportError> {
    match field(root, "verifier")? {
        Value::Null => Ok(json!({"status": "not_selected"})),
        value => {
            let verifier = object(value, "verifier")?;
            Ok(json!({
                "status": "unsupported",
                "id": string(verifier, "id")?,
                "descriptor_id": string(verifier, "descriptor_id")?,
                "reason": string(verifier, "reason")?,
                "process_started": boolean(verifier, "process_started")?,
                "executable_resolved": boolean(verifier, "executable_resolved")?
            }))
        }
    }
}

fn render_markdown_v3(manifest: &Value) -> Result<Vec<u8>, GenericHumanReportError> {
    let root = object(manifest, "manifest")?;
    let audit = object(field(root, "audit")?, "audit")?;
    let authority = object(field(root, "authority")?, "authority")?;
    let coverage = object(field(root, "coverage")?, "coverage")?;
    let trace = object(field(root, "source_window_trace")?, "source window trace")?;
    let verifier = object(field(root, "verifier")?, "verifier")?;
    let mut rendered = String::from("# Generic review projection\n\n");
    rendered.push_str("This is a non-authority projection of canonical run-v3 audit JSON.\n\n");
    rendered.push_str(&format!(
        "- Audit: `{}`\n",
        cell(string(audit, "canonical_sha256")?)
    ));
    rendered.push_str(&format!("- Run: `{}`\n", cell(string(audit, "run_id")?)));
    rendered.push_str(&format!(
        "- Snapshot: `{}`\n",
        cell(string(audit, "snapshot_id")?)
    ));
    rendered.push_str(&format!(
        "- Universe: `{}`\n",
        cell(string(audit, "universe_id")?)
    ));
    rendered.push_str(&format!(
        "- trusted_pass = `{}`\n",
        boolean(authority, "trusted_pass")?
    ));
    rendered.push_str("\n## Coverage\n\n");
    rendered.push_str("Resolved-target obligations are listed below; candidate-space enumeration remains partial and no global call coverage claim is made.\n\n");
    list_values(
        &mut rendered,
        array(coverage, "resolved_target_obligation_ids")?,
    );
    rendered.push_str("\n## Context commitments\n\n");
    list_v3_contexts(&mut rendered, array(root, "contexts")?)?;
    rendered.push_str("\n## Proposed claims\n\n");
    list_objects(&mut rendered, array(root, "proposed_claims")?, "summary")?;
    rendered.push_str("\n## Abstentions\n\n");
    list_objects(&mut rendered, array(root, "abstentions")?, "detail")?;
    rendered.push_str("\n## Malformed outputs\n\n");
    list_objects(
        &mut rendered,
        array(root, "malformed_outputs")?,
        "diagnostic",
    )?;
    rendered.push_str("\n## Provider failures\n\n");
    list_objects(
        &mut rendered,
        array(root, "provider_failures")?,
        "diagnostic",
    )?;
    rendered.push_str("\n## Verifier\n\n");
    rendered.push_str(&format!(
        "- Status: `{}`\n",
        cell(string(verifier, "status")?)
    ));
    rendered.push_str("- No verifier observation was run.\n");
    rendered.push_str("\n## Source and window trace\n\nSources:\n");
    list_values(&mut rendered, array(trace, "source_ids")?);
    rendered.push_str("\nWindows:\n");
    list_values(&mut rendered, array(trace, "window_ids")?);
    if rendered.len() > MAX_RENDERED_MARKDOWN_BYTES {
        return Err(GenericHumanReportError::RenderBound {
            limit: MAX_RENDERED_MARKDOWN_BYTES,
            observed: rendered.len(),
        });
    }
    Ok(rendered.into_bytes())
}

fn list_v3_contexts(
    rendered: &mut String,
    contexts: &[Value],
) -> Result<(), GenericHumanReportError> {
    if contexts.is_empty() {
        rendered.push_str("- None\n");
        return Ok(());
    }
    for context in contexts {
        let context = object(context, "v3 context projection")?;
        rendered.push_str(&format!(
            "- `{}`: policy `{}` (`{}`)\n",
            cell(string(context, "context_id")?),
            cell(string(context, "context_policy_id")?),
            cell(string(context, "context_policy_hash")?)
        ));
        let commitments = object(
            field(context, "denominator_commitments")?,
            "denominator commitments",
        )?;
        for (label, field_name) in [
            ("accepted-file", "accepted_file"),
            ("reached-file", "reached_file"),
            ("materialized-source", "materialized_source"),
            ("support-anchor", "support_anchor"),
        ] {
            let commitment = object(field(commitments, field_name)?, "denominator commitment")?;
            rendered.push_str(&format!(
                "  - {} denominator: `{}` (`{}`)\n",
                label,
                unsigned(commitment, "observed_count")?,
                cell(string(commitment, "sorted_id_set_sha256")?)
            ));
        }
        let latent = object(field(context, "latent_cardinality")?, "latent cardinality")?;
        rendered.push_str(&format!(
            "  - Latent cardinality: `{}`\n",
            cell(string(latent, "state")?)
        ));
        for summary in array(context, "support_loss_summaries")? {
            let summary = object(summary, "support loss summary")?;
            rendered.push_str(&format!(
                "  - Support loss `{}`: `{}` (`{}`)\n",
                cell(string(summary, "reason")?),
                unsigned(summary, "observed_count")?,
                cell(string(summary, "sorted_anchor_id_set_sha256")?)
            ));
        }
    }
    Ok(())
}

fn render_markdown(manifest: &Value) -> Result<Vec<u8>, GenericHumanReportError> {
    let root = object(manifest, "manifest")?;
    let audit = object(field(root, "audit")?, "audit")?;
    let authority = object(field(root, "authority")?, "authority")?;
    let coverage = object(field(root, "coverage")?, "coverage")?;
    let candidate_space = object(field(coverage, "candidate_space")?, "candidate space")?;
    let trace = object(field(root, "source_window_trace")?, "source_window_trace")?;
    let verifier = object(field(root, "verifier")?, "verifier")?;
    let mut rendered = String::from("# Generic review projection\n\n");
    rendered.push_str("This is a non-authority projection of canonical audit JSON.\n\n");
    rendered.push_str(&format!(
        "- Audit: `{}`\n",
        cell(string(audit, "canonical_sha256")?)
    ));
    rendered.push_str(&format!("- Run: `{}`\n", cell(string(audit, "run_id")?)));
    rendered.push_str(&format!(
        "- Snapshot: `{}`\n",
        cell(string(audit, "snapshot_id")?)
    ));
    rendered.push_str(&format!(
        "- Universe: `{}`\n",
        cell(string(audit, "universe_id")?)
    ));
    rendered.push_str(&format!(
        "- trusted_pass = `{}`\n",
        boolean(authority, "trusted_pass")?
    ));
    rendered.push_str("\n## Coverage\n\n");
    rendered.push_str("Resolved-target obligations are listed below; candidate-space enumeration remains partial and no global call coverage claim is made.\n\n");
    list_values(
        &mut rendered,
        array(coverage, "resolved_target_obligation_ids")?,
    );
    rendered.push_str("\n## Candidate-space summaries\n\n");
    rendered.push_str(&format!(
        "- Observed unresolved call occurrences: `{}`\n",
        unsigned(candidate_space, "observed_unresolved_call_occurrence_count")?
    ));
    rendered.push_str(&format!(
        "- Occurrence ID-set hash: `{}`\n",
        cell(string(candidate_space, "occurrence_id_set_sha256")?)
    ));
    let global_limitation = object(
        field(candidate_space, "global_direct_calls_limitation")?,
        "global direct-calls limitation",
    )?;
    rendered.push_str(&format!(
        "- Macro latent occurrence count: `{}` (unknown is retained as unknown)\n",
        cell(string(global_limitation, "latent_occurrence_count")?)
    ));
    list_candidate_space_summaries(
        &mut rendered,
        array(candidate_space, "source_occurrence_summaries")?,
    )?;
    rendered.push_str("\n## Proposed claims\n\n");
    list_objects(&mut rendered, array(root, "proposed_claims")?, "summary")?;
    rendered.push_str("\n## Abstentions\n\n");
    list_objects(&mut rendered, array(root, "abstentions")?, "detail")?;
    rendered.push_str("\n## Malformed outputs\n\n");
    list_objects(
        &mut rendered,
        array(root, "malformed_outputs")?,
        "diagnostic",
    )?;
    rendered.push_str("\n## Provider failures\n\n");
    list_objects(
        &mut rendered,
        array(root, "provider_failures")?,
        "diagnostic",
    )?;
    rendered.push_str("\n## Verifier\n\n");
    rendered.push_str(&format!(
        "- Status: `{}`\n",
        cell(string(verifier, "status")?)
    ));
    rendered.push_str("- No verifier observation was run.\n");
    rendered.push_str("\n## Source and window trace\n\nSources:\n");
    list_values(&mut rendered, array(trace, "source_ids")?);
    rendered.push_str("\nWindows:\n");
    list_values(&mut rendered, array(trace, "window_ids")?);
    if rendered.len() > MAX_RENDERED_MARKDOWN_BYTES {
        return Err(GenericHumanReportError::RenderBound {
            limit: MAX_RENDERED_MARKDOWN_BYTES,
            observed: rendered.len(),
        });
    }
    Ok(rendered.into_bytes())
}

fn list_values(rendered: &mut String, values: &[Value]) {
    if values.is_empty() {
        rendered.push_str("- None\n");
    } else {
        for value in values {
            rendered.push_str(&format!(
                "- `{}`\n",
                cell(value.as_str().unwrap_or("invalid"))
            ));
        }
    }
}

fn list_objects(
    rendered: &mut String,
    values: &[Value],
    detail: &'static str,
) -> Result<(), GenericHumanReportError> {
    if values.is_empty() {
        rendered.push_str("- None\n");
        return Ok(());
    }
    for value in values {
        let value = object(value, "rendered row")?;
        rendered.push_str(&format!(
            "- `{}` / `{}`: {}\n",
            cell(string(value, "obligation_id")?),
            cell(string(value, "execution_id")?),
            quoted(string(value, detail)?)
        ));
    }
    Ok(())
}

fn list_candidate_space_summaries(
    rendered: &mut String,
    summaries: &[Value],
) -> Result<(), GenericHumanReportError> {
    if summaries.is_empty() {
        rendered.push_str("- No source occurrence summaries\n");
        return Ok(());
    }
    for summary in summaries {
        let summary = object(summary, "source occurrence summary")?;
        rendered.push_str(&format!(
            "- `{}` / `{}`: `{}` observed occurrences\n",
            cell(string(summary, "file_source_id")?),
            cell(string(summary, "path")?),
            unsigned(summary, "observed_occurrence_count")?
        ));
        for bucket in array(summary, "buckets")? {
            let bucket = object(bucket, "source occurrence summary bucket")?;
            rendered.push_str(&format!(
                "  - `{}` / `{}`: `{}` (`{}`)\n",
                cell(string(bucket, "call_kind")?),
                cell(string(bucket, "reason")?),
                unsigned(bucket, "observed_occurrence_count")?,
                cell(string(bucket, "occurrence_id_set_sha256")?)
            ));
        }
    }
    Ok(())
}

fn quoted(value: &str) -> String {
    serde_json::to_string(value)
        .unwrap_or_else(|_| "\"unrenderable text\"".to_owned())
        .replace('`', "\\`")
}

fn cell(value: &str) -> String {
    value
        .replace('`', "\\`")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn plan_stage_ids(plan: &Map<String, Value>) -> Result<Vec<Value>, GenericHumanReportError> {
    array(plan, "waves")?
        .iter()
        .map(|wave| {
            Ok(Value::String(
                string(object(wave, "wave")?, "id")?.to_owned(),
            ))
        })
        .collect()
}

fn extend_ids(destination: &mut Vec<Value>, values: &[Value]) {
    destination.extend(values.iter().cloned());
}

fn value_string_order(left: &Value, right: &Value) -> std::cmp::Ordering {
    left.as_str().cmp(&right.as_str())
}

fn object<'a>(
    value: &'a Value,
    name: &'static str,
) -> Result<&'a Map<String, Value>, GenericHumanReportError> {
    value
        .as_object()
        .ok_or(GenericHumanReportError::Invalid(name))
}

fn object_mut<'a>(
    value: &'a mut Value,
    name: &'static str,
) -> Result<&'a mut Map<String, Value>, GenericHumanReportError> {
    value
        .as_object_mut()
        .ok_or(GenericHumanReportError::Invalid(name))
}

fn array<'a>(
    object: &'a Map<String, Value>,
    name: &'static str,
) -> Result<&'a [Value], GenericHumanReportError> {
    field(object, name)?
        .as_array()
        .map(Vec::as_slice)
        .ok_or(GenericHumanReportError::Invalid(name))
}

fn string<'a>(
    object: &'a Map<String, Value>,
    name: &'static str,
) -> Result<&'a str, GenericHumanReportError> {
    field(object, name)?
        .as_str()
        .ok_or(GenericHumanReportError::Invalid(name))
}

fn boolean(
    object: &Map<String, Value>,
    name: &'static str,
) -> Result<bool, GenericHumanReportError> {
    field(object, name)?
        .as_bool()
        .ok_or(GenericHumanReportError::Invalid(name))
}

fn unsigned(
    object: &Map<String, Value>,
    name: &'static str,
) -> Result<u64, GenericHumanReportError> {
    field(object, name)?
        .as_u64()
        .ok_or(GenericHumanReportError::Invalid(name))
}

fn field<'a>(
    object: &'a Map<String, Value>,
    name: &'static str,
) -> Result<&'a Value, GenericHumanReportError> {
    object
        .get(name)
        .ok_or(GenericHumanReportError::Missing(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_violation_reports_paths_without_echoing_the_large_instance() {
        let marker = "instance-payload-must-not-be-echoed";
        let value = json!({"items": [marker, marker, marker]});
        let error = validate_schema(
            &value,
            r#"{"type":"object","properties":{"items":{"type":"array","maxItems":2}}}"#,
            "test schema",
        )
        .expect_err("oversized array");
        let message = error.to_string();
        assert!(message.contains("/items"));
        assert!(message.contains("maxItems"));
        assert!(!message.contains(marker));
    }
}
