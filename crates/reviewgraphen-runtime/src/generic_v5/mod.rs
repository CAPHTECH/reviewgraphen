//! Generic-review v5 owners and the closed TypeScript enumerate/defer route.

pub mod accounting;
pub mod admission;
pub(crate) mod g3;
pub mod ingestion;

#[cfg(test)]
#[path = "g3/acceptance/mod.rs"]
mod generic_rc2_g3_acceptance;

use admission::{
    SourceAdmissionBoundsV1, SourceAdmissionSubmissionV1, SourceProfileClaimV1, SourceReadClaimV1,
    SourceReadExtentV1, admit_typescript_revision_pair,
};
use reviewgraphen_core::source_review::{
    basis::{SourceFileOutcome, SourceSyntaxRole},
    ids::{
        CanonicalFileKey, SnapshotBinding, SourceFileId, SourceWitnessKeyV1, SyntaxKeyV1,
        registry_tuple_hash,
    },
    reasons::{RecordOutcomeV1, ResolutionOutcomeV1},
    registry::{TypeScriptRegistryBinding, typescript_registry_binding},
};
use reviewgraphen_core::{ContentHash, canonical_json};
use reviewgraphen_ingest::typescript::payload::{
    CallableOutcomeV1, SyntaxRole, TypeScriptOutcomeV1, TypeScriptPayload,
    TypeScriptPrimaryReasonV1, TypeScriptReasonsV1, encode_payload_draft,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

/// The five public product bytes placed beside the artifact manifest.
pub struct TypeScriptV5Artifacts {
    pub extraction: Vec<u8>,
    pub ingestion: Vec<u8>,
    pub audit: Vec<u8>,
    pub human_manifest: Vec<u8>,
    pub human_markdown: Vec<u8>,
    pub request_id: String,
    pub run_id: String,
    pub snapshot_id: String,
    pub universe_id: String,
}

/// Enumerates the literal Git pair and deliberately defers every obligation.
/// The raw submission below is only an output projection: A1 accepts payloads
/// by rebuilding through `ReconstructionContextV1`'s private A0 source view.
#[allow(clippy::too_many_arguments)]
pub fn typescript_enumerate_and_defer_artifacts(
    workspace: &Path,
    repository: &Path,
    base_revision: &str,
    target_revision: &str,
    max_files: usize,
    max_file_bytes: usize,
    max_total_source_bytes: usize,
    request_bytes: &[u8],
) -> Result<TypeScriptV5Artifacts, String> {
    let (submission, context) = admit_typescript_revision_pair(
        workspace.to_path_buf(),
        repository.to_path_buf(),
        base_revision.to_owned(),
        target_revision.to_owned(),
        SourceAdmissionBoundsV1 {
            max_files,
            max_file_bytes,
            max_total_source_bytes,
        },
    )
    .map_err(|error| format!("typescript v5 source admission failed: {error}"))?;
    context
        .check_g3_integrity()
        .map_err(|error| format!("typescript v5 G3 integrity refused: {error}"))?;
    let binding = typescript_registry_binding();

    // This is the sole accepted payload input: the context builds it from its
    // sealed A0 provenance. The public raw source-view implementation never
    // participates in this call.
    let rebuilt = context
        .rebuild_payload_catalogue_from_a0()
        .map_err(|error| format!("typescript v5 payload reconstruction failed: {error}"))?;
    let parsed_file_keys = rebuilt.parsed_file_keys();
    let parsed_file_count = rebuilt.parsed_file_count();
    let mut syntax_records = Vec::new();
    let mut eligible_node_keys = Vec::new();
    let mut resolved_local_calls = 0_u64;
    let mut resolved_relative_calls = 0_u64;
    for (file_id, admitted) in context.seal_rebuilt_payload_catalogue_from_a0(rebuilt) {
        let admitted = admitted.into_inner();
        let record = syntax_record(&binding, context.snapshot_binding(), &file_id, &admitted)?;
        if is_eligible_node(&admitted) {
            eligible_node_keys.push(
                record["key"]
                    .as_str()
                    .ok_or("typescript v5 node key missing")?
                    .to_owned(),
            );
        }
        if record["record_role"] == "call" && record["outcome"] == "resolved" {
            match record["payload"]["data"]["resolution_kind"].as_str() {
                Some("syntactic_unique") => resolved_local_calls += 1,
                Some("syntactic_unique_relative_import@1") => resolved_relative_calls += 1,
                _ => {}
            }
        }
        syntax_records.push(record);
    }
    syntax_records.sort_by(|left, right| left["key"].as_str().cmp(&right["key"].as_str()));
    eligible_node_keys.sort();
    eligible_node_keys.dedup();

    let registry = registry_binding_value(&binding);
    let snapshot = json!({
        "repository_identity": "admitted_by_git_a0",
        "base_revision": context.base_commit_oid(),
        "target_revision": context.target_commit_oid(),
        "registry_hash": binding.registry_hash,
        "tuple_hash": registry_tuple_hash(&binding),
        "profile_hash": ContentHash::sha256(b"typescript.production.v1@1").to_string(),
        "extractor_set_hash": binding.tuple.extractor_set_hash,
        "rule_set_hash": binding.tuple.rule_set_hash,
    });
    let extraction = json!({
        "schema": "reviewgraphen.extraction_report.v2",
        "registry_binding": registry,
        "snapshot_binding": snapshot,
        "file_records": file_records(&submission)?,
        "inventory_complete": submission.material.inventory_complete,
        "file_partitions": file_partitions(&submission),
        "syntax_records": syntax_records,
        "syntax_partitions": {"parsed_file_keys": parsed_file_keys, "inventory_complete": submission.material.inventory_complete},
        "capability_records": [
            {"stage":"a0", "status":"admitted_from_literal_git"},
            {"stage":"a1", "status":"payloads_rebuilt_and_compared"}
        ]
    });
    let extraction_bytes = canonical_json(&extraction)
        .map_err(|error| format!("typescript v5 extraction serialization failed: {error}"))?;
    let extraction_hash = ContentHash::sha256(&extraction_bytes).to_string();
    let snapshot_id = ContentHash::sha256(
        &canonical_json(&extraction["snapshot_binding"])
            .map_err(|error| format!("typescript v5 snapshot serialization failed: {error}"))?,
    )
    .to_string();
    let ingestion = json!({
        "schema": "reviewgraphen.ingestion_report.v3",
        "registry_binding": registry_binding_value(&binding),
        "snapshot_binding": extraction["snapshot_binding"],
        "extraction_binding": {"artifact_hash": extraction_hash, "registry_binding": registry_binding_value(&binding)},
        "d_pairs": [],
        "d_partition": {"eligible_keys": [], "ineligible_keys": [], "unknown": 1, "reason": "change_witnesses_not_materialized_in_v5_vertical"}
    });
    let ingestion_bytes = canonical_json(&ingestion)
        .map_err(|error| format!("typescript v5 ingestion serialization failed: {error}"))?;
    let ingestion_hash = ContentHash::sha256(&ingestion_bytes).to_string();
    let mut obligations = eligible_node_keys
        .iter()
        .map(|key| {
            deferred_obligation(
                &binding,
                "node.public_function_contract@2",
                "typescript.public_function_contract_review@1",
                None,
                "node",
                key,
            )
        })
        .collect::<Vec<_>>();
    let gap_key = ContentHash::sha256(b"typescript-v5-change-witness-gap@1").to_string();
    obligations.push(deferred_obligation(
        &binding,
        "capability_gap.origin_rule@1",
        "typescript.callee_contract_review@1",
        Some("relation.changed_public_callee@2"),
        "subgraph",
        &gap_key,
    ));
    obligations.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    let deferred_ids = obligations
        .iter()
        .filter_map(|row| row["id"].as_str())
        .collect::<Vec<_>>();
    let obligation_count = obligations.len();
    let universe_id = ContentHash::sha256(
        &canonical_json(&obligations)
            .map_err(|error| format!("typescript v5 obligation serialization failed: {error}"))?,
    )
    .to_string();
    let request_id = format!("request:{}", ContentHash::sha256(request_bytes));
    let run_id = format!("run:{}", ContentHash::sha256(universe_id.as_bytes()));
    let audit = json!({
        "schema": "reviewgraphen.generic_review_run.v5",
        "registry_binding": registry_binding_value(&binding), "run_id": run_id, "request_id": request_id,
        "basis_binding": {"registry_binding": registry_binding_value(&binding), "artifact_hash": ContentHash::sha256(snapshot_id.as_bytes()).to_string()},
        "extraction_binding": {"registry_binding": registry_binding_value(&binding), "artifact_hash": extraction_hash},
        "ingestion_binding": {"registry_binding": registry_binding_value(&binding), "artifact_hash": ingestion_hash},
        "obligations": obligations,
        "obligation_partition": {"node_ids": eligible_node_keys, "relation_ids": [], "gap_ids": [gap_key]},
        "plan_partition": {"planned_ids": [], "deferred_ids": deferred_ids},
        "coverage": {"denominator": obligation_count, "visited": 0, "completed": 0, "evidence_supported": 0, "verified": 0, "human_accepted": 0},
        "source_trace": {"base_revision": context.base_commit_oid(), "target_revision": context.target_commit_oid(), "inventory_complete": submission.material.inventory_complete, "parsed_files": parsed_file_count, "resolved_local_calls": resolved_local_calls, "resolved_relative_calls": resolved_relative_calls, "d_change_witnesses": "unavailable"},
        "execution": {"mode":"enumerate_and_defer", "context_projection":"unavailable", "reviewer_execution":"not_run", "verifier_execution":"not_run"},
        "authority": {"trusted_pass": false}
    });
    let audit_bytes = canonical_json(&audit)
        .map_err(|error| format!("typescript v5 audit serialization failed: {error}"))?;
    let markdown = format!(
        "# ReviewGraphen TypeScript enumerate-and-defer\n\nRead files: {parsed_file_count}\n\ntrusted_pass: false\n"
    );
    let human_manifest = json!({
        "schema": "reviewgraphen.generic_review_human_report.v4", "registry_binding": registry_binding_value(&binding),
        "run_id": run_id, "run_hash": ContentHash::sha256(&audit_bytes).to_string(),
        "projection_id": binding.tuple.projection_id, "markdown_hash": ContentHash::sha256(markdown.as_bytes()).to_string()
    });
    Ok(TypeScriptV5Artifacts {
        extraction: extraction_bytes,
        ingestion: ingestion_bytes,
        audit: audit_bytes,
        human_manifest: canonical_json(&human_manifest).map_err(|error| {
            format!("typescript v5 human manifest serialization failed: {error}")
        })?,
        human_markdown: markdown.into_bytes(),
        request_id,
        run_id,
        snapshot_id,
        universe_id,
    })
}

fn registry_binding_value(binding: &TypeScriptRegistryBinding) -> Value {
    json!({"registry_id": binding.registry_id, "registry_hash": binding.registry_hash,
        "arm_id": binding.arm_id, "arm_hash": binding.arm_hash, "tuple_hash": registry_tuple_hash(binding)})
}

fn source_file_id(path: &str) -> Result<SourceFileId, String> {
    CanonicalFileKey::from_basis_path(path)
        .map(SourceFileId::from_basis_file_key)
        .map_err(|_| format!("A0 emitted invalid inventory path {path}"))
}

fn file_records(submission: &SourceAdmissionSubmissionV1) -> Result<Vec<Value>, String> {
    submission
        .material
        .target_inventory
        .iter()
        .map(|entry| {
            let (bytes_read, source_hash) = match &entry.read {
                SourceReadClaimV1::Complete { .. } => {
                    let bytes = submission
                        .material
                        .target_read_material
                        .iter()
                        .find(|read| {
                            read.path == entry.path && read.extent == SourceReadExtentV1::FullBlob
                        })
                        .map(|read| read.bytes.as_slice())
                        .ok_or_else(|| {
                            format!("A0 complete file missing bytes for {}", entry.path)
                        })?;
                    (true, Some(ContentHash::sha256(bytes).to_string()))
                }
                // A Failed read counts as
                // bytes_read only when bytes were actually received
                // (byte_count > 0); a zero-byte failure read nothing.
                SourceReadClaimV1::Failed { byte_count, .. } => (*byte_count > 0, None),
                SourceReadClaimV1::NotRead { .. } => (false, None),
            };
            Ok(
                json!({"key": source_file_id(&entry.path)?.canonical_key(), "path": entry.path,
            "language": (entry.outcome == SourceFileOutcome::Parsed).then_some("typescript"),
            "outcome": file_outcome_wire(entry.outcome), "bytes_read": bytes_read,
            "reason_ids": profile_reasons(&entry.profile), "source_hash": source_hash}),
            )
        })
        .collect()
}

fn file_outcome_wire(outcome: SourceFileOutcome) -> &'static str {
    match outcome {
        SourceFileOutcome::Parsed => "parsed",
        SourceFileOutcome::ParseFailed => "parse_failed",
        SourceFileOutcome::NonTargetExtension => "non_target_extension",
        SourceFileOutcome::ProfileExcluded => "profile_excluded",
        SourceFileOutcome::UnreadBound => "unread_bound",
        SourceFileOutcome::UnsupportedEntry => "unsupported_entry",
    }
}

fn profile_reasons(profile: &SourceProfileClaimV1) -> Vec<String> {
    match profile {
        SourceProfileClaimV1::Excluded { reasons, .. } => reasons.clone(),
        SourceProfileClaimV1::Included | SourceProfileClaimV1::NonTargetExtension => Vec::new(),
    }
}

fn file_partitions(submission: &SourceAdmissionSubmissionV1) -> Value {
    let mut partitions = BTreeMap::<&str, Vec<String>>::new();
    for entry in &submission.material.target_inventory {
        partitions
            .entry(file_outcome_wire(entry.outcome))
            .or_default()
            .push(entry.path.clone());
    }
    json!(partitions)
}

fn syntax_record(
    binding: &TypeScriptRegistryBinding,
    snapshot: &SnapshotBinding,
    file_id: &SourceFileId,
    payload: &TypeScriptPayload,
) -> Result<Value, String> {
    let draft = encode_payload_draft(payload);
    let source_role = match draft.role {
        SyntaxRole::Callable => SourceSyntaxRole::Callable,
        SyntaxRole::Call => SourceSyntaxRole::Call,
        SyntaxRole::Binding => SourceSyntaxRole::Binding,
        SyntaxRole::Surface => SourceSyntaxRole::Surface,
        SyntaxRole::Scope => SourceSyntaxRole::Scope,
    };
    let key = SyntaxKeyV1::derive_from_source(
        binding,
        snapshot,
        file_id,
        source_role,
        &draft.kind,
        draft.range,
    );
    Ok(
        json!({"key": key.wire_literal(), "record_role": syntax_role_wire(draft.role), "kind_id": draft.kind.wire_literal(), "file_key": file_id.canonical_key(),
        "range": {"start": draft.range.start(), "end": draft.range.end()}, "outcome": outcome_wire(&draft.outcome), "reasons": reason_wires(&draft.reasons), "primary_reason": primary_reason_wire(draft.primary_reason.as_ref()),
        "refs": {"visibility_witness_refs": draft.refs.visibility_witness_refs.iter().map(|key| key.wire_literal()).collect::<Vec<_>>(), "change_witness_refs": draft.refs.change_witness_refs.iter().map(|key| key.wire_literal()).collect::<Vec<_>>(), "support_refs": draft.refs.support_refs.iter().map(source_witness_wire).collect::<Vec<_>>()},
        "payload": {"descriptor_id": draft.descriptor_id.wire_literal(), "descriptor_hash": draft.descriptor_hash.wire_literal(), "data": draft.data}}),
    )
}

fn syntax_role_wire(role: SyntaxRole) -> &'static str {
    match role {
        SyntaxRole::Callable => "callable",
        SyntaxRole::Call => "call",
        SyntaxRole::Binding => "binding",
        SyntaxRole::Surface => "surface",
        SyntaxRole::Scope => "scope",
    }
}
fn outcome_wire(outcome: &TypeScriptOutcomeV1) -> &'static str {
    match outcome {
        TypeScriptOutcomeV1::Callable(CallableOutcomeV1::EligiblePublic) => "eligible_public",
        TypeScriptOutcomeV1::Callable(CallableOutcomeV1::NonPublic) => "non_public",
        TypeScriptOutcomeV1::Callable(CallableOutcomeV1::NotRuntimeCallable) => {
            "not_runtime_callable"
        }
        TypeScriptOutcomeV1::Callable(CallableOutcomeV1::OutOfScope) => "out_of_scope",
        TypeScriptOutcomeV1::Callable(CallableOutcomeV1::Unsupported) => "unsupported",
        TypeScriptOutcomeV1::Call(ResolutionOutcomeV1::Resolved)
        | TypeScriptOutcomeV1::Binding(ResolutionOutcomeV1::Resolved) => "resolved",
        TypeScriptOutcomeV1::Call(ResolutionOutcomeV1::Unresolved)
        | TypeScriptOutcomeV1::Binding(ResolutionOutcomeV1::Unresolved) => "unresolved",
        TypeScriptOutcomeV1::Surface(RecordOutcomeV1::Recorded)
        | TypeScriptOutcomeV1::Scope(RecordOutcomeV1::Recorded) => "recorded",
        TypeScriptOutcomeV1::Surface(RecordOutcomeV1::Unsupported)
        | TypeScriptOutcomeV1::Scope(RecordOutcomeV1::Unsupported) => "unsupported",
    }
}
fn reason_wires(reasons: &TypeScriptReasonsV1) -> Vec<String> {
    match reasons {
        TypeScriptReasonsV1::Callable(set) => set
            .all()
            .iter()
            .map(|reason| reason.wire_literal().to_owned())
            .collect(),
        TypeScriptReasonsV1::Call(set) => set
            .all()
            .iter()
            .map(|reason| reason.wire_literal().to_owned())
            .collect(),
        TypeScriptReasonsV1::Binding(set) => set
            .all()
            .iter()
            .map(|reason| reason.wire_literal().to_owned())
            .collect(),
        TypeScriptReasonsV1::Surface(set) => set
            .all()
            .iter()
            .map(|reason| reason.wire_literal().to_owned())
            .collect(),
        TypeScriptReasonsV1::Scope(set) => set
            .all()
            .iter()
            .map(|reason| reason.wire_literal().to_owned())
            .collect(),
    }
}
fn primary_reason_wire(reason: Option<&TypeScriptPrimaryReasonV1>) -> Option<String> {
    reason.map(|reason| match reason {
        TypeScriptPrimaryReasonV1::Callable(reason) => reason.wire_literal().to_owned(),
        TypeScriptPrimaryReasonV1::Call(reason) => reason.wire_literal().to_owned(),
        TypeScriptPrimaryReasonV1::Binding(reason) => reason.wire_literal().to_owned(),
        TypeScriptPrimaryReasonV1::Surface(reason) => reason.wire_literal().to_owned(),
        TypeScriptPrimaryReasonV1::Scope(reason) => reason.wire_literal().to_owned(),
    })
}
fn source_witness_wire(key: &SourceWitnessKeyV1) -> &str {
    match key {
        SourceWitnessKeyV1::Syntax(key) => key.wire_literal(),
        SourceWitnessKeyV1::BasisEndpoint(key) => key.wire_literal(),
    }
}
fn is_eligible_node(payload: &TypeScriptPayload) -> bool {
    matches!(
        payload.outcome,
        TypeScriptOutcomeV1::Callable(CallableOutcomeV1::EligiblePublic)
    )
}
fn deferred_obligation(
    binding: &TypeScriptRegistryBinding,
    rule_id: &str,
    property_id: &str,
    origin_rule_id: Option<&str>,
    target_kind: &str,
    target_key: &str,
) -> Value {
    let id = ContentHash::sha256(&canonical_json(&json!({"binding": registry_binding_value(binding), "domain": "typescript.enumerate_and_defer.obligation.v5", "origin_rule_id": origin_rule_id, "property_id": property_id, "rule_id": rule_id, "target_key": target_key, "target_kind": target_kind})).expect("fixed obligation preimage is canonical")).to_string();
    json!({"id": id, "registry_binding": registry_binding_value(binding), "rule_id": rule_id, "property_id": property_id, "origin_rule_id": origin_rule_id, "target_kind": target_kind, "status": "deferred"})
}
