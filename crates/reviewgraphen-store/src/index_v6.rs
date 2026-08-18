//! Minimal content-addressed projection of a fresh V5 target predecessor.
//!
//! This projection is deliberately not an editable planning index.  It binds
//! the exact confirmed V5 genesis prefix to the accepted ProgramSpace and
//! obligation universe that M6 will use as its target predecessor.  The
//! canonical projection bytes are themselves a CAS object and are re-read
//! before Store can mint an incremental session proof.

use super::{IndexError, IndexLimits};
use crate::{CasHash, CasReader, CasStore, EventJournal, StoreRoot, StoreRootIdentity};
use reviewgraphen_core::{
    ContentHash, EventLogV5, ProgramSpace, StableId, TerminalProofV5, V5InheritedReportProjection,
    V5StructuralPrefixCoordinates, V5TerminalReviewClosure, V5TypedProjectionRecord,
    canonical_json,
};
use rusqlite::{Connection, OpenFlags, Transaction, types::ValueRef};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::cell::Cell;
use std::io::Cursor;

pub const INDEX_SCHEMA_VERSION_V6: u64 = 6;
pub const PROJECTION_CONTRACT_VERSION_V6: &str =
    "reviewgraphen.pre_incremental_index_projection.v6";
/// Contract for the distinct current/terminal snapshot envelope.  It is not
/// the narrower pre-incremental marker contract.
pub const INDEX_SNAPSHOT_CONTRACT_VERSION_V6: &str = "reviewgraphen.index_snapshot.v6";

#[cfg(test)]
thread_local! {
    /// Counts the one point at which V6 allocates and writes its full
    /// canonical snapshot.  Limit tests use this to prove that a rejected
    /// preflight never starts serializer/materialization work.
    static V6_CANONICAL_SNAPSHOT_MATERIALIZATION_COUNT: Cell<u64> = const { Cell::new(0) };
}

#[cfg(test)]
fn reset_v6_canonical_snapshot_materialization_count_for_test() {
    V6_CANONICAL_SNAPSHOT_MATERIALIZATION_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
fn v6_canonical_snapshot_materialization_count_for_test() -> u64 {
    V6_CANONICAL_SNAPSHOT_MATERIALIZATION_COUNT.with(Cell::get)
}

#[cfg(test)]
fn record_v6_canonical_snapshot_materialization() {
    V6_CANONICAL_SNAPSHOT_MATERIALIZATION_COUNT
        .with(|count| count.set(count.get().saturating_add(1)));
}

#[cfg(not(test))]
fn record_v6_canonical_snapshot_materialization() {}

// This is intentionally a stand-alone literal.  V6 does not apply a patch to
// a V5 image: an old image is a different derived representation and must be
// rebuilt from the V5 journal. The inherited and M6 families retain their
// distinct ADR-defined tables; no generic row table coalesces claim, evidence,
// verification, decision, or gluing state.
pub(crate) const SCHEMA_V6: &str = r#"
CREATE TABLE index_meta (
 singleton INTEGER PRIMARY KEY CHECK (singleton=1),
 index_schema_version INTEGER NOT NULL CHECK (index_schema_version=6),
 projection_contract_version TEXT NOT NULL CHECK (projection_contract_version='reviewgraphen.index_projection.v6'),
 event_contract_version TEXT NOT NULL CHECK (event_contract_version='reviewgraphen.review_event.v5'),
 projection_mode TEXT NOT NULL CHECK (projection_mode IN ('v5_incremental','v5_current_structural','v5_terminal_proof')),
 run_id TEXT NOT NULL, genesis_hash TEXT NOT NULL,
 confirmed_offset INTEGER NOT NULL CHECK (confirmed_offset>=0), tail_hash TEXT NOT NULL,
 event_count INTEGER NOT NULL CHECK (event_count>=0),
 policy_revision_hash TEXT NOT NULL,
 authority_replay_basis_digest TEXT NOT NULL,
 incremental_source_closure_id TEXT, source_prefix_tail_hash TEXT,
 CHECK((incremental_source_closure_id IS NULL)=(source_prefix_tail_hash IS NULL))
) STRICT;
CREATE TABLE events (
 sequence INTEGER PRIMARY KEY CHECK (sequence>0), event_id TEXT NOT NULL UNIQUE,
 schema TEXT NOT NULL CHECK (schema='reviewgraphen.review_event.v5'),
 event_hash TEXT NOT NULL, payload_hash TEXT NOT NULL,
 payload_kind TEXT NOT NULL CHECK (payload_kind IN (
  'obligation_transition','run_genesis_manifest','artifact_registered',
  'snapshot_sources_recorded','review_plan_recorded','context_envelope_projected',
  'review_execution_recorded','evidence_recorded_v3','evidence_bound_v3',
  'verification_recorded_v3','decision_recorded_v3','finding_recorded_v3',
  'artifact_registered_v4','gluing_bundle_recorded_v4',
  'incremental_source_bound_v5','program_mapping_recorded_v5','change_morphism_sealed_v5',
  'obligation_correspondence_entry_recorded_v5','obligation_correspondence_sealed_v5',
  'historical_record_assessed_v5','gluing_freshness_recorded_v5','staleness_assessment_sealed_v5',
  'artifact_registered_v5','preservation_verified_v5','partial_rerun_action_recorded_v5',
  'partial_rerun_plan_sealed_v5','gluing_rerun_action_recorded_v5','gluing_rerun_plan_sealed_v5','terminal_completed_v5'
 )),
 actor TEXT NOT NULL, logical_time INTEGER NOT NULL CHECK (logical_time>=0),
 UNIQUE(sequence,event_id)
) STRICT;
CREATE TABLE program_objects (object_id TEXT PRIMARY KEY, object_kind TEXT NOT NULL, body_hash TEXT NOT NULL) STRICT;
CREATE TABLE program_relations (relation_id TEXT PRIMARY KEY, relation_kind TEXT NOT NULL, source_id TEXT NOT NULL, target_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL) STRICT;
CREATE TABLE universe (singleton INTEGER PRIMARY KEY CHECK (singleton = 1), universe_id TEXT NOT NULL UNIQUE, snapshot_id TEXT NOT NULL, profile_id TEXT NOT NULL, rule_set_hash TEXT NOT NULL, extractor_set_hash TEXT NOT NULL, policy_version TEXT NOT NULL, rule_pack_version TEXT NOT NULL, body_hash TEXT NOT NULL) STRICT;
CREATE TABLE obligations (obligation_id TEXT PRIMARY KEY, target_kind TEXT NOT NULL CHECK(target_kind IN ('node','relation','path','invariant','subgraph')), target_ids_canonical_json TEXT NOT NULL, property_id TEXT NOT NULL, lifecycle TEXT NOT NULL CHECK(lifecycle IN ('generated','planned','in_progress','completed','stale','superseded','cancelled')), body_hash TEXT NOT NULL) STRICT;
CREATE TABLE obligation_lifecycle (event_sequence INTEGER NOT NULL, event_id TEXT NOT NULL, obligation_id TEXT NOT NULL, next_lifecycle TEXT NOT NULL CHECK(next_lifecycle IN ('generated','planned','in_progress','completed','stale','superseded','cancelled')), PRIMARY KEY(event_sequence, obligation_id), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)) STRICT;
CREATE TABLE executions (
 event_sequence INTEGER NOT NULL CHECK (event_sequence > 0), event_id TEXT NOT NULL,
 execution_id TEXT NOT NULL UNIQUE, plan_id TEXT NOT NULL, wave_id TEXT NOT NULL,
 snapshot_id TEXT NOT NULL, envelope_id TEXT NOT NULL, obligation_ids_canonical_json TEXT NOT NULL,
 reviewer_kind TEXT NOT NULL CHECK (reviewer_kind = 'fake'), reviewer_id TEXT NOT NULL CHECK (reviewer_id = 'reviewgraphen.fake_reviewer@1'),
 provider TEXT, model TEXT, model_revision TEXT,
 system_prompt_version TEXT NOT NULL CHECK (system_prompt_version = 'reviewgraphen.system.no_tools@1'),
 prompt_template_version TEXT NOT NULL CHECK (prompt_template_version = 'fixture@1'),
 inference_settings_canonical_json TEXT NOT NULL CHECK (inference_settings_canonical_json = '{}'),
 tool_policy_version TEXT NOT NULL CHECK (tool_policy_version = 'reviewgraphen.tool_policy.none@1'),
 tool_calls_canonical_json TEXT NOT NULL CHECK (tool_calls_canonical_json = '[]'),
 attempt INTEGER NOT NULL CHECK (attempt > 0), raw_registration_id TEXT NOT NULL,
 raw_hash TEXT NOT NULL, parsed_claim_ids_canonical_json TEXT NOT NULL,
 outcome_kind TEXT NOT NULL CHECK (outcome_kind IN ('structured','abstained','malformed','provider_failure')),
 outcome_canonical_json TEXT NOT NULL, identity_body_hash TEXT NOT NULL, body_hash TEXT NOT NULL,
 PRIMARY KEY (event_sequence, execution_id),
 FOREIGN KEY (event_sequence, event_id) REFERENCES events (sequence, event_id),
 FOREIGN KEY (plan_id) REFERENCES review_plans (plan_id), FOREIGN KEY (envelope_id) REFERENCES context_envelopes (envelope_id),
 FOREIGN KEY (raw_registration_id) REFERENCES artifact_registrations (registration_id),
 CHECK (provider IS NULL AND model IS NULL AND model_revision IS NULL), CHECK (attempt <= 4294967295)
) STRICT;
CREATE TABLE claims (
 event_sequence INTEGER NOT NULL CHECK (event_sequence > 0), event_id TEXT NOT NULL,
 claim_id TEXT NOT NULL UNIQUE, execution_id TEXT NOT NULL,
 obligation_ids_canonical_json TEXT NOT NULL, property_id TEXT NOT NULL,
 target_refs_canonical_json TEXT NOT NULL,
 polarity TEXT NOT NULL CHECK (polarity IN ('issue_present','issue_absent','inconclusive','not_applicable','conflict')),
 disposition TEXT NOT NULL CHECK (disposition = 'proposed'), summary TEXT NOT NULL,
 source_ids_canonical_json TEXT NOT NULL, assumptions_canonical_json TEXT NOT NULL,
 requested_evidence_canonical_json TEXT NOT NULL, candidate_confidence_canonical_json TEXT NOT NULL,
 author_kind TEXT NOT NULL CHECK (author_kind = 'ai'), review_status TEXT NOT NULL CHECK (review_status = 'unreviewed'),
 identity_body_hash TEXT NOT NULL, body_hash TEXT NOT NULL,
 PRIMARY KEY (event_sequence, claim_id), FOREIGN KEY (event_sequence,event_id) REFERENCES events(sequence,event_id),
 FOREIGN KEY (execution_id) REFERENCES executions(execution_id)
) STRICT;
CREATE TABLE artifact_registrations (
 event_sequence INTEGER NOT NULL CHECK (event_sequence>0), event_id TEXT NOT NULL,
 registration_id TEXT NOT NULL UNIQUE, run_id TEXT NOT NULL, cas_hash TEXT NOT NULL,
 media_type TEXT NOT NULL, size INTEGER NOT NULL CHECK(size>=0),
 sensitivity TEXT NOT NULL CHECK(sensitivity IN ('canonical_state','workspace_source','sensitive')),
 source_kind TEXT NOT NULL CHECK(source_kind IN ('run_genesis','snapshot_ingest','reviewer_execution','verifier_artifact','external_harness_witness')),
 source_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
 PRIMARY KEY(event_sequence,registration_id), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;
CREATE TABLE snapshot_source_index (event_sequence INTEGER NOT NULL, event_id TEXT NOT NULL, snapshot_id TEXT NOT NULL, artifact_id TEXT NOT NULL, registration_id TEXT NOT NULL, path TEXT NOT NULL, content_hash TEXT NOT NULL, cas_hash TEXT NOT NULL, line_count INTEGER NOT NULL CHECK(line_count >= 0), PRIMARY KEY(snapshot_id,path,artifact_id), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)) STRICT;
CREATE TABLE review_plans (
 event_sequence INTEGER NOT NULL CHECK(event_sequence > 0), event_id TEXT NOT NULL,
 plan_id TEXT NOT NULL UNIQUE, universe_id TEXT NOT NULL, snapshot_id TEXT NOT NULL,
 planner_input_hash TEXT NOT NULL, planner_policy_version TEXT NOT NULL CHECK(planner_policy_version = 'scheduler.baseline@1'),
 planner_policy_hash TEXT NOT NULL, budget_canonical_json TEXT NOT NULL, budget_hash TEXT NOT NULL,
 risk_breakdown_canonical_json TEXT NOT NULL, waves_canonical_json TEXT NOT NULL, deferred_canonical_json TEXT NOT NULL,
 identity_body_hash TEXT NOT NULL, body_hash TEXT NOT NULL,
 PRIMARY KEY(event_sequence,plan_id), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;
CREATE TABLE context_envelopes (
 event_sequence INTEGER NOT NULL CHECK(event_sequence > 0), event_id TEXT NOT NULL,
 envelope_id TEXT NOT NULL UNIQUE, snapshot_id TEXT NOT NULL,
 context_policy_version TEXT NOT NULL CHECK(context_policy_version = 'context.baseline@1'),
 context_policy_hash TEXT NOT NULL, candidate_ids_canonical_json TEXT NOT NULL,
 obligation_ids_canonical_json TEXT NOT NULL, context_policy_canonical_json TEXT NOT NULL,
 included_sources_canonical_json TEXT NOT NULL, excluded_sources_canonical_json TEXT NOT NULL,
 unknowns_canonical_json TEXT NOT NULL, assumptions_canonical_json TEXT NOT NULL,
 losses_canonical_json TEXT NOT NULL, projection_hash TEXT NOT NULL, body_hash TEXT NOT NULL,
 PRIMARY KEY(event_sequence,envelope_id), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;
CREATE TABLE unreconciled_authority_records (event_sequence INTEGER NOT NULL CHECK(event_sequence > 0), event_id TEXT NOT NULL, record_id TEXT PRIMARY KEY, kind TEXT NOT NULL CHECK(kind IN ('evidence_recorded','evidence_bound','verification_recorded','decision_recorded')), body_hash TEXT NOT NULL, authority_reconciled INTEGER NOT NULL DEFAULT 0 CHECK(authority_reconciled = 0), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)) STRICT;
CREATE TABLE projected_findings (event_sequence INTEGER NOT NULL CHECK(event_sequence > 0), event_id TEXT NOT NULL, finding_id TEXT PRIMARY KEY, body_hash TEXT NOT NULL, projection_status TEXT NOT NULL DEFAULT 'shadow_only' CHECK(projection_status = 'shadow_only'), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)) STRICT;
CREATE TABLE evidence_v3 (
 event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL, evidence_id TEXT NOT NULL UNIQUE,
 schema TEXT NOT NULL CHECK(schema='reviewgraphen.evidence.v3'), kind TEXT NOT NULL CHECK(kind IN ('static_fact','test_witness')),
 snapshot_id TEXT NOT NULL, subject_ids_canonical_json TEXT NOT NULL, descriptor_id TEXT NOT NULL,
 procedure_version TEXT NOT NULL, input_registration_id TEXT NOT NULL, output_registration_id TEXT NOT NULL,
 observation TEXT NOT NULL CHECK(observation IN ('fact_present','witnessed')), body_hash TEXT NOT NULL,
 PRIMARY KEY(event_sequence,evidence_id), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id),
 FOREIGN KEY(input_registration_id) REFERENCES artifact_registrations(registration_id), FOREIGN KEY(output_registration_id) REFERENCES artifact_registrations(registration_id),
 CHECK ((kind='static_fact' AND observation='fact_present' AND descriptor_id='reviewgraphen.static_fact_verifier@1' AND procedure_version='reviewgraphen.static_fact.projection@1') OR (kind='test_witness' AND observation='witnessed' AND descriptor_id='reviewgraphen.fixture_test_verifier@1' AND procedure_version='reviewgraphen.fixture_test.duplicate_submit@1'))
) STRICT;
CREATE TABLE evidence_bindings_v3 (
 event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL, binding_id TEXT NOT NULL UNIQUE,
 schema TEXT NOT NULL CHECK(schema='reviewgraphen.evidence_binding.v3'), claim_id TEXT NOT NULL, evidence_id TEXT NOT NULL,
 relation TEXT NOT NULL CHECK(relation IN ('qualifies','reproduces')), property_id TEXT NOT NULL CHECK(property_id='payment.at_most_once'), body_hash TEXT NOT NULL,
 PRIMARY KEY(event_sequence,binding_id), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id),
 FOREIGN KEY(claim_id) REFERENCES claims(claim_id), FOREIGN KEY(evidence_id) REFERENCES evidence_v3(evidence_id)
) STRICT;
CREATE TABLE verifications_v3 (
 event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL, verification_id TEXT NOT NULL UNIQUE,
 schema TEXT NOT NULL CHECK(schema='reviewgraphen.verification.v3'), claim_id TEXT NOT NULL,
 descriptor_id TEXT NOT NULL, procedure_version TEXT NOT NULL, input_registration_id TEXT NOT NULL,
 output_registration_id TEXT NOT NULL, evidence_ids_canonical_json TEXT NOT NULL,
 outcome TEXT NOT NULL CHECK(outcome IN ('passed','inconclusive','unsupported')), limitations_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
 PRIMARY KEY(event_sequence,verification_id), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id),
 FOREIGN KEY(claim_id) REFERENCES claims(claim_id), FOREIGN KEY(input_registration_id) REFERENCES artifact_registrations(registration_id),
 FOREIGN KEY(output_registration_id) REFERENCES artifact_registrations(registration_id),
 CHECK ((descriptor_id='reviewgraphen.static_fact_verifier@1' AND procedure_version='reviewgraphen.static_fact.projection@1' AND outcome IN ('inconclusive','unsupported')) OR (descriptor_id='reviewgraphen.fixture_test_verifier@1' AND procedure_version='reviewgraphen.fixture_test.duplicate_submit@1' AND outcome='passed'))
) STRICT;
CREATE TABLE decisions_v3 (
 event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL, decision_id TEXT NOT NULL UNIQUE,
 schema TEXT NOT NULL CHECK(schema='reviewgraphen.human_decision.v3'), policy_revision_hash TEXT NOT NULL,
 run_id TEXT NOT NULL, universe_id TEXT NOT NULL, claim_id TEXT NOT NULL,
 property_id TEXT NOT NULL CHECK(property_id='payment.at_most_once'), outcome TEXT NOT NULL CHECK(outcome IN ('accept','reject','defer','exception')),
 actor TEXT NOT NULL CHECK(actor GLOB 'human:?*'), authority_id TEXT NOT NULL, snapshot_id TEXT NOT NULL,
 source_ids_canonical_json TEXT NOT NULL, rationale TEXT NOT NULL CHECK(length(CAST(rationale AS BLOB)) BETWEEN 1 AND 8192),
 issued_at TEXT NOT NULL, expires_at TEXT, body_hash TEXT NOT NULL,
 PRIMARY KEY(event_sequence,decision_id), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id),
 FOREIGN KEY(claim_id) REFERENCES claims(claim_id), CHECK((outcome='exception' AND expires_at IS NOT NULL) OR (outcome!='exception' AND expires_at IS NULL))
) STRICT;
CREATE TABLE findings_v3 (
 event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL, finding_id TEXT NOT NULL UNIQUE,
 schema TEXT NOT NULL CHECK(schema='reviewgraphen.finding.v3'), projection_descriptor_id TEXT NOT NULL CHECK(projection_descriptor_id='reviewgraphen.finding_projection@1'),
 claim_id TEXT NOT NULL, status TEXT NOT NULL CHECK(status IN ('unverified_candidate','verified_candidate','accepted','rejected')),
 evidence_ids_canonical_json TEXT NOT NULL, verification_ids_canonical_json TEXT NOT NULL,
 decision_id TEXT, supersedes_finding_id TEXT, body_hash TEXT NOT NULL,
 PRIMARY KEY(event_sequence,finding_id), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id),
 FOREIGN KEY(claim_id) REFERENCES claims(claim_id), FOREIGN KEY(decision_id) REFERENCES decisions_v3(decision_id),
 FOREIGN KEY(supersedes_finding_id) REFERENCES findings_v3(finding_id),
 CHECK((status IN ('accepted','rejected') AND decision_id IS NOT NULL) OR (status IN ('unverified_candidate','verified_candidate') AND decision_id IS NULL))
) STRICT;
CREATE TABLE claim_assessments_v3 (
 claim_id TEXT PRIMARY KEY, disposition TEXT NOT NULL CHECK(disposition IN ('proposed','supported','accepted','rejected')),
 review_status TEXT NOT NULL CHECK(review_status IN ('unreviewed','human_reviewed','accepted','rejected')),
 binding_ids_canonical_json TEXT NOT NULL, evidence_ids_canonical_json TEXT NOT NULL,
 verification_ids_canonical_json TEXT NOT NULL, decision_ids_canonical_json TEXT NOT NULL,
 finding_ids_canonical_json TEXT NOT NULL, active_decision_id TEXT, current_finding_id TEXT,
 decision_conflict INTEGER NOT NULL CHECK(decision_conflict IN (0,1)),
 confirmed_event_sequence INTEGER NOT NULL CHECK(confirmed_event_sequence>0),
 FOREIGN KEY(claim_id) REFERENCES claims(claim_id), FOREIGN KEY(active_decision_id) REFERENCES decisions_v3(decision_id),
 FOREIGN KEY(current_finding_id) REFERENCES findings_v3(finding_id)
) STRICT;
CREATE TABLE artifact_registrations_v4 (
 event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
 registration_id TEXT NOT NULL UNIQUE,
 schema TEXT NOT NULL CHECK(schema='reviewgraphen.artifact_registration.v4'),
 run_id TEXT NOT NULL, cas_hash TEXT NOT NULL, media_type TEXT NOT NULL,
 size INTEGER NOT NULL CHECK(size>=0),
 sensitivity TEXT NOT NULL CHECK(sensitivity IN ('canonical_state','workspace_source','sensitive')),
 source_kind TEXT NOT NULL CHECK(source_kind IN ('run_genesis','snapshot_ingest','reviewer_execution','verifier_artifact','external_harness_witness','gluing_input')),
 source_canonical_json TEXT NOT NULL, descriptor_id TEXT NOT NULL UNIQUE, body_hash TEXT NOT NULL,
 PRIMARY KEY(event_sequence,registration_id), UNIQUE(registration_id,descriptor_id),
 FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id),
 FOREIGN KEY(descriptor_id) REFERENCES gluing_input_descriptors_v4(descriptor_id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE gluing_input_descriptors_v4 (
 event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
 descriptor_id TEXT NOT NULL UNIQUE, registration_id TEXT NOT NULL UNIQUE,
 schema TEXT NOT NULL CHECK(schema='reviewgraphen.gluing_input_descriptor.v4'),
 run_id TEXT NOT NULL, snapshot_id TEXT NOT NULL, universe_id TEXT NOT NULL, plan_id TEXT NOT NULL,
 profile_descriptor_id TEXT NOT NULL CHECK(profile_descriptor_id='reviewgraphen.double_submit_gluing@1'),
 context_id TEXT NOT NULL CHECK(context_id IN ('context:payment','context:ui-event')),
 assignment_key TEXT NOT NULL CHECK(assignment_key='caller_duplicate_protection'),
 assignment_value TEXT NOT NULL CHECK(assignment_value IN ('satisfied','required','unknown')),
 qualification_source_ids_canonical_json TEXT NOT NULL,
 descriptor_hash TEXT NOT NULL, descriptor_size INTEGER NOT NULL CHECK(descriptor_size>=0), body_hash TEXT NOT NULL,
 PRIMARY KEY(event_sequence,descriptor_id), UNIQUE(run_id,context_id),
 FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id),
 FOREIGN KEY(registration_id,descriptor_id) REFERENCES artifact_registrations_v4(registration_id,descriptor_id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE context_covers_v4 (
 event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
 cover_id TEXT NOT NULL UNIQUE, schema TEXT NOT NULL CHECK(schema='reviewgraphen.context_cover.v4'),
 run_id TEXT NOT NULL, snapshot_id TEXT NOT NULL, universe_id TEXT NOT NULL, plan_id TEXT NOT NULL,
 profile_descriptor_id TEXT NOT NULL CHECK(profile_descriptor_id='reviewgraphen.double_submit_gluing@1'),
 selected_obligation_ids_canonical_json TEXT NOT NULL, required_context_ids_canonical_json TEXT NOT NULL,
 cover_domain_ids_canonical_json TEXT NOT NULL, covered_domain_ids_canonical_json TEXT NOT NULL,
 uncovered_domain_ids_canonical_json TEXT NOT NULL, source_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
 PRIMARY KEY(event_sequence,cover_id), UNIQUE(event_sequence,event_id,cover_id),
 FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;
CREATE TABLE sections_v4 (
 event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
 section_id TEXT NOT NULL UNIQUE, schema TEXT NOT NULL CHECK(schema='reviewgraphen.section.v4'),
 cover_id TEXT NOT NULL REFERENCES context_covers_v4(cover_id),
 context_id TEXT NOT NULL CHECK(context_id IN ('context:payment','context:ui-event')),
 snapshot_id TEXT NOT NULL, property_id TEXT NOT NULL CHECK(property_id='payment.at_most_once'),
 invariant_id TEXT NOT NULL CHECK(invariant_id='invariant:payment-at-most-once'),
 obligation_id TEXT NOT NULL, claim_id TEXT NOT NULL REFERENCES claims(claim_id),
 claim_assessment_id TEXT NOT NULL REFERENCES claim_assessments_v3(claim_id),
 input_descriptor_id TEXT NOT NULL REFERENCES gluing_input_descriptors_v4(descriptor_id),
 input_registration_id TEXT NOT NULL REFERENCES artifact_registrations_v4(registration_id),
 assignment_key TEXT NOT NULL CHECK(assignment_key='caller_duplicate_protection'),
 assignment_value TEXT NOT NULL CHECK(assignment_value IN ('satisfied','required','unknown')),
 passed_current_verification INTEGER NOT NULL CHECK(passed_current_verification IN (0,1)),
 source_ids_canonical_json TEXT NOT NULL, qualification_source_ids_canonical_json TEXT NOT NULL,
 binding_ids_canonical_json TEXT NOT NULL, evidence_ids_canonical_json TEXT NOT NULL,
 verification_ids_canonical_json TEXT NOT NULL, decision_ids_canonical_json TEXT NOT NULL,
 finding_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
 PRIMARY KEY(event_sequence,section_id), UNIQUE(cover_id,context_id), UNIQUE(event_sequence,event_id,section_id),
 FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;
CREATE TABLE gluing_attempts_v4 (
 event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
 attempt_id TEXT NOT NULL UNIQUE, schema TEXT NOT NULL CHECK(schema='reviewgraphen.gluing_attempt.v4'),
 cover_id TEXT NOT NULL REFERENCES context_covers_v4(cover_id), snapshot_id TEXT NOT NULL,
 property_id TEXT NOT NULL CHECK(property_id='payment.at_most_once'),
 invariant_id TEXT NOT NULL CHECK(invariant_id='invariant:payment-at-most-once'),
 input_descriptor_ids_canonical_json TEXT NOT NULL, section_ids_canonical_json TEXT NOT NULL,
 restriction_ids_canonical_json TEXT NOT NULL,
 result TEXT NOT NULL CHECK(result IN ('failed','unknown','candidate','glued_with_qualification','glued')),
 global_candidate_id TEXT UNIQUE, obstruction_id TEXT UNIQUE,
 source_ids_canonical_json TEXT NOT NULL, claim_ids_canonical_json TEXT NOT NULL,
 evidence_ids_canonical_json TEXT NOT NULL, verification_ids_canonical_json TEXT NOT NULL,
 decision_ids_canonical_json TEXT NOT NULL, finding_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
 PRIMARY KEY(event_sequence,attempt_id), UNIQUE(event_sequence,event_id,attempt_id),
 FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;
CREATE TABLE restrictions_v4 (
 event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
 attempt_id TEXT NOT NULL REFERENCES gluing_attempts_v4(attempt_id), restriction_id TEXT NOT NULL UNIQUE,
 schema TEXT NOT NULL CHECK(schema='reviewgraphen.restriction.v4'), section_id TEXT NOT NULL REFERENCES sections_v4(section_id),
 context_pair_canonical_json TEXT NOT NULL, overlap_member_ids_canonical_json TEXT NOT NULL,
 assignment_key TEXT NOT NULL CHECK(assignment_key='caller_duplicate_protection'),
 assignment_value TEXT NOT NULL CHECK(assignment_value IN ('satisfied','required','unknown')),
 source_ids_canonical_json TEXT NOT NULL, qualification_source_ids_canonical_json TEXT NOT NULL,
 claim_ids_canonical_json TEXT NOT NULL, evidence_ids_canonical_json TEXT NOT NULL,
 verification_ids_canonical_json TEXT NOT NULL, decision_ids_canonical_json TEXT NOT NULL,
 finding_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
 PRIMARY KEY(event_sequence,restriction_id), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id),
 FOREIGN KEY(event_sequence,event_id,attempt_id) REFERENCES gluing_attempts_v4(event_sequence,event_id,attempt_id),
 FOREIGN KEY(event_sequence,event_id,section_id) REFERENCES sections_v4(event_sequence,event_id,section_id)
) STRICT;
CREATE TABLE global_candidates_v4 (
 event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
 attempt_id TEXT NOT NULL REFERENCES gluing_attempts_v4(attempt_id),
 global_candidate_id TEXT NOT NULL UNIQUE REFERENCES gluing_attempts_v4(global_candidate_id),
 schema TEXT NOT NULL CHECK(schema='reviewgraphen.global_candidate.v4'),
 cover_id TEXT NOT NULL REFERENCES context_covers_v4(cover_id),
 invariant_id TEXT NOT NULL CHECK(invariant_id='invariant:payment-at-most-once'),
 property_id TEXT NOT NULL CHECK(property_id='payment.at_most_once'),
 required_section_ids_canonical_json TEXT NOT NULL, restriction_ids_canonical_json TEXT NOT NULL,
 qualification_source_ids_canonical_json TEXT NOT NULL, source_ids_canonical_json TEXT NOT NULL,
 claim_ids_canonical_json TEXT NOT NULL, evidence_ids_canonical_json TEXT NOT NULL,
 verification_ids_canonical_json TEXT NOT NULL, decision_ids_canonical_json TEXT NOT NULL,
 finding_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
 PRIMARY KEY(event_sequence,global_candidate_id), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id),
 FOREIGN KEY(event_sequence,event_id,attempt_id) REFERENCES gluing_attempts_v4(event_sequence,event_id,attempt_id),
 FOREIGN KEY(event_sequence,event_id,cover_id) REFERENCES context_covers_v4(event_sequence,event_id,cover_id)
) STRICT;
CREATE TABLE gluing_obstructions_v4 (
 event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
 attempt_id TEXT NOT NULL REFERENCES gluing_attempts_v4(attempt_id), obstruction_id TEXT NOT NULL UNIQUE,
 schema TEXT NOT NULL CHECK(schema='reviewgraphen.gluing_obstruction.v4'),
 kind TEXT NOT NULL CHECK(kind IN ('required_section_missing','section_unknown','required_overlap_missing','assignment_conflict')),
 conflicting_context_ids_canonical_json TEXT NOT NULL, section_ids_canonical_json TEXT NOT NULL,
 overlap_member_ids_canonical_json TEXT NOT NULL,
 assignment_key TEXT NOT NULL CHECK(assignment_key='caller_duplicate_protection'),
 left_assignment_value TEXT CHECK(left_assignment_value IN ('satisfied','required','unknown')),
 right_assignment_value TEXT CHECK(right_assignment_value IN ('satisfied','required','unknown')),
 source_ids_canonical_json TEXT NOT NULL, claim_ids_canonical_json TEXT NOT NULL,
 evidence_ids_canonical_json TEXT NOT NULL, verification_ids_canonical_json TEXT NOT NULL,
 decision_ids_canonical_json TEXT NOT NULL, finding_ids_canonical_json TEXT NOT NULL,
 affected_invariant_id TEXT NOT NULL CHECK(affected_invariant_id='invariant:payment-at-most-once'),
 severity TEXT NOT NULL CHECK(severity IN ('high','critical')),
 required_resolution TEXT NOT NULL CHECK(required_resolution IN ('record_required_section','resolve_context_overlap','resolve_unknown_duplicate_protection_assignment','resolve_duplicate_protection_responsibility')),
 human_decision_required INTEGER NOT NULL CHECK(human_decision_required IN (0,1)),
 blocks_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
 PRIMARY KEY(event_sequence,obstruction_id), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;
CREATE TABLE artifact_registrations_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  registration_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.artifact_registration.v5'),
  run_id TEXT NOT NULL, cas_hash TEXT NOT NULL, media_type TEXT NOT NULL,
  size INTEGER NOT NULL CHECK(size>=0),
  sensitivity TEXT NOT NULL CHECK(sensitivity='canonical_state'),
  source_kind TEXT NOT NULL CHECK(source_kind='preservation_artifact'),
  source_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,registration_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE incremental_source_closures_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  closure_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.incremental_source_closure.v5'),
  repository_id TEXT NOT NULL, repository_identity_hash TEXT NOT NULL,
  source_run_id TEXT NOT NULL, source_genesis_hash TEXT NOT NULL,
  source_confirmed_offset INTEGER NOT NULL CHECK(source_confirmed_offset>=0),
  source_tail_hash TEXT NOT NULL, source_event_count INTEGER NOT NULL CHECK(source_event_count>0),
  source_snapshot_id TEXT NOT NULL, source_universe_id TEXT NOT NULL,
  source_index_snapshot_hash TEXT NOT NULL,
  source_authority_policy_revision_hash TEXT NOT NULL,
  source_authority_replay_basis_digest TEXT NOT NULL,
  source_resolved_target_commit_oid TEXT NOT NULL, source_target_tree_hash TEXT NOT NULL,
  source_gluing_bundle_id TEXT NOT NULL,
  target_run_id TEXT NOT NULL, target_genesis_hash TEXT NOT NULL,
  target_predecessor_offset INTEGER NOT NULL CHECK(target_predecessor_offset>=0),
  target_predecessor_tail_hash TEXT NOT NULL,
  target_predecessor_event_count INTEGER NOT NULL CHECK(target_predecessor_event_count>0),
  target_snapshot_id TEXT NOT NULL, target_universe_id TEXT NOT NULL,
  target_predecessor_index_snapshot_hash TEXT NOT NULL,
  target_authority_policy_revision_hash TEXT NOT NULL,
  target_pre_incremental_authority_replay_basis_digest TEXT NOT NULL,
  target_resolved_base_commit_oid TEXT NOT NULL, target_base_tree_hash TEXT NOT NULL,
  target_resolved_target_commit_oid TEXT NOT NULL, target_target_tree_hash TEXT NOT NULL,
  body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,closure_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE program_mappings_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  mapping_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.program_mapping.v5'),
  source_closure_id TEXT NOT NULL REFERENCES incremental_source_closures_v5(closure_id),
  source_snapshot_id TEXT NOT NULL, target_snapshot_id TEXT NOT NULL,
  object_kind TEXT NOT NULL CHECK(object_kind IN ('repository','snapshot','artifact','relation','context','invariant','limitation')),
  from_ids_canonical_json TEXT NOT NULL, to_ids_canonical_json TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('preserved','modified','added','removed','split','merged','unresolved')),
  candidate_key_kind TEXT NOT NULL CHECK(candidate_key_kind IN ('repository_identity','snapshot_pair','same_path','git_rename_same_content','rust_symbol_anchor_v1','same_kind_label_language_location','mapped_directed_endpoints','mapped_members','mapped_scope','mapped_limitation_sources','no_candidate')),
  source_body_hashes_canonical_json TEXT NOT NULL,
  target_body_hashes_canonical_json TEXT NOT NULL,
  change_fact_ids_canonical_json TEXT NOT NULL,
  predecessor_mapping_ids_canonical_json TEXT NOT NULL,
  successor_ids_canonical_json TEXT NOT NULL, source_ids_canonical_json TEXT NOT NULL,
  body_hash TEXT NOT NULL, PRIMARY KEY(event_sequence,mapping_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE change_morphisms_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  morphism_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.change_morphism.v5'),
  source_closure_id TEXT NOT NULL REFERENCES incremental_source_closures_v5(closure_id),
  repository_id TEXT NOT NULL, source_snapshot_id TEXT NOT NULL, target_snapshot_id TEXT NOT NULL,
  mapping_policy_descriptor_id TEXT NOT NULL CHECK(mapping_policy_descriptor_id='reviewgraphen.program_mapping@1'),
  semantic_anchor_descriptor_id TEXT NOT NULL CHECK(semantic_anchor_descriptor_id='reviewgraphen.rust_symbol_anchor@1'),
  mapping_count INTEGER NOT NULL CHECK(mapping_count>=0), mapping_set_digest TEXT NOT NULL,
  source_domain_count INTEGER NOT NULL CHECK(source_domain_count>=0), source_domain_digest TEXT NOT NULL,
  target_domain_count INTEGER NOT NULL CHECK(target_domain_count>=0), target_domain_digest TEXT NOT NULL,
  status_counts_canonical_json TEXT NOT NULL, source_ids_canonical_json TEXT NOT NULL,
  body_hash TEXT NOT NULL, PRIMARY KEY(event_sequence,morphism_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE obligation_correspondence_entries_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  entry_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.obligation_correspondence_entry.v5'),
  morphism_id TEXT NOT NULL REFERENCES change_morphisms_v5(morphism_id),
  from_obligation_ids_canonical_json TEXT NOT NULL, to_obligation_ids_canonical_json TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('preserved','modified','added','removed','split','merged','unresolved')),
  source_mapping_ids_canonical_json TEXT NOT NULL,
  predecessor_entry_ids_canonical_json TEXT NOT NULL,
  successor_obligation_ids_canonical_json TEXT NOT NULL,
  source_body_hashes_canonical_json TEXT NOT NULL,
  target_body_hashes_canonical_json TEXT NOT NULL,
  source_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,entry_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE obligation_correspondences_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  correspondence_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.obligation_correspondence.v5'),
  morphism_id TEXT NOT NULL REFERENCES change_morphisms_v5(morphism_id),
  source_universe_id TEXT NOT NULL, target_universe_id TEXT NOT NULL,
  policy_descriptor_id TEXT NOT NULL CHECK(policy_descriptor_id='reviewgraphen.obligation_correspondence@1'),
  entry_count INTEGER NOT NULL CHECK(entry_count>=0), entry_set_digest TEXT NOT NULL,
  source_domain_count INTEGER NOT NULL CHECK(source_domain_count>=0), source_domain_digest TEXT NOT NULL,
  target_domain_count INTEGER NOT NULL CHECK(target_domain_count>=0), target_domain_digest TEXT NOT NULL,
  status_counts_canonical_json TEXT NOT NULL, source_ids_canonical_json TEXT NOT NULL,
  body_hash TEXT NOT NULL, PRIMARY KEY(event_sequence,correspondence_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE historical_record_assessments_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  record_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.historical_record_assessment.v5'),
  assessment_id TEXT NOT NULL,
  source_record_kind TEXT NOT NULL CHECK(source_record_kind IN ('obligation','review_plan','context_envelope','artifact_registration_v3','artifact_registration_v4','execution','claim','claim_assessment','evidence','evidence_binding','verification','decision','finding','gluing_input_descriptor','context_cover','section','restriction','gluing_attempt','global_candidate','gluing_obstruction','coverage')),
  source_record_id TEXT NOT NULL, source_record_body_hash TEXT NOT NULL,
  successor_record_ids_canonical_json TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('structurally_preserved','stale','superseded')),
  directness TEXT NOT NULL CHECK(directness IN ('not_applicable','direct','indirect','direct_and_indirect')),
  reasons_canonical_json TEXT NOT NULL, dependency_source_ids_canonical_json TEXT NOT NULL,
  mapping_ids_canonical_json TEXT NOT NULL,
  correspondence_entry_ids_canonical_json TEXT NOT NULL,
  source_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,record_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE gluing_freshness_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  freshness_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.gluing_freshness.v5'),
  assessment_id TEXT NOT NULL, source_attempt_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('structurally_preserved','stale','superseded')),
  reasons_canonical_json TEXT NOT NULL,
  dependency_mapping_ids_canonical_json TEXT NOT NULL,
  successor_target_attempt_ids_canonical_json TEXT NOT NULL,
  source_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,freshness_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE staleness_assessments_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  assessment_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.staleness_assessment.v5'),
  source_closure_id TEXT NOT NULL REFERENCES incremental_source_closures_v5(closure_id),
  morphism_id TEXT NOT NULL REFERENCES change_morphisms_v5(morphism_id),
  correspondence_id TEXT NOT NULL REFERENCES obligation_correspondences_v5(correspondence_id),
  impact_policy_descriptor_id TEXT NOT NULL CHECK(impact_policy_descriptor_id='reviewgraphen.mvp_property_impact@1'),
  assessment_time TEXT NOT NULL,
  record_count INTEGER NOT NULL CHECK(record_count>=0), record_set_digest TEXT NOT NULL,
  gluing_freshness_count INTEGER NOT NULL CHECK(gluing_freshness_count>=0),
  gluing_freshness_set_digest TEXT NOT NULL,
  stale_source_count INTEGER NOT NULL CHECK(stale_source_count>=0), stale_source_digest TEXT NOT NULL,
  superseded_source_count INTEGER NOT NULL CHECK(superseded_source_count>=0), superseded_source_digest TEXT NOT NULL,
  preservation_candidate_count INTEGER NOT NULL CHECK(preservation_candidate_count>=0),
  preservation_candidate_digest TEXT NOT NULL,
  m5_dependent_successor_count INTEGER NOT NULL CHECK(m5_dependent_successor_count>=0),
  m5_dependent_successor_digest TEXT NOT NULL,
  source_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,assessment_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE preservation_evidence_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  evidence_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.preservation_evidence.v5'),
  target_snapshot_id TEXT NOT NULL, target_obligation_id TEXT NOT NULL,
  source_closure_id TEXT NOT NULL REFERENCES incremental_source_closures_v5(closure_id),
  morphism_id TEXT NOT NULL REFERENCES change_morphisms_v5(morphism_id),
  correspondence_entry_id TEXT NOT NULL REFERENCES obligation_correspondence_entries_v5(entry_id),
  source_claim_id TEXT NOT NULL, source_evidence_ids_canonical_json TEXT NOT NULL,
  source_verification_id TEXT NOT NULL,
  dependency_mapping_ids_canonical_json TEXT NOT NULL,
  input_registration_id TEXT NOT NULL REFERENCES artifact_registrations_v5(registration_id),
  output_registration_id TEXT NOT NULL REFERENCES artifact_registrations_v5(registration_id),
  descriptor_id TEXT NOT NULL CHECK(descriptor_id='reviewgraphen.structural_preservation@1'),
  procedure_version TEXT NOT NULL CHECK(procedure_version='reviewgraphen.structural_preservation.payment_v1'),
  observation TEXT NOT NULL CHECK(observation='structure_preserved'),
  source_ids_canonical_json TEXT NOT NULL,
  body_hash TEXT NOT NULL, PRIMARY KEY(event_sequence,evidence_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE preservation_verifications_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  verification_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.preservation_verification.v5'),
  target_snapshot_id TEXT NOT NULL, target_obligation_id TEXT NOT NULL,
  evidence_id TEXT NOT NULL REFERENCES preservation_evidence_v5(evidence_id),
  source_verification_id TEXT NOT NULL,
  descriptor_id TEXT NOT NULL CHECK(descriptor_id='reviewgraphen.structural_preservation@1'),
  procedure_version TEXT NOT NULL CHECK(procedure_version='reviewgraphen.structural_preservation.payment_v1'),
  outcome TEXT NOT NULL CHECK(outcome='passed'),
  source_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,verification_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE partial_rerun_actions_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  action_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.partial_rerun_action.v5'),
  staleness_assessment_id TEXT NOT NULL REFERENCES staleness_assessments_v5(assessment_id),
  subject_kind TEXT NOT NULL CHECK(subject_kind='obligation'),
  subject_ids_canonical_json TEXT NOT NULL,
  action TEXT NOT NULL CHECK(action IN ('reproject_context','rerun_reviewer','rerun_verifier','rerun_human_decision')),
  prerequisites_canonical_json TEXT NOT NULL,
  stale_source_record_ids_canonical_json TEXT NOT NULL,
  reasons_canonical_json TEXT NOT NULL,
  source_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,action_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE partial_rerun_plans_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  plan_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.partial_rerun_plan.v5'),
  source_closure_id TEXT NOT NULL REFERENCES incremental_source_closures_v5(closure_id),
  morphism_id TEXT NOT NULL REFERENCES change_morphisms_v5(morphism_id),
  correspondence_id TEXT NOT NULL REFERENCES obligation_correspondences_v5(correspondence_id),
  staleness_assessment_id TEXT NOT NULL REFERENCES staleness_assessments_v5(assessment_id),
  planner_descriptor_id TEXT NOT NULL CHECK(planner_descriptor_id='reviewgraphen.partial_rerun@1'),
  target_plan_id TEXT NOT NULL,
  selected_target_count INTEGER NOT NULL CHECK(selected_target_count>=0), selected_target_digest TEXT NOT NULL,
  action_count INTEGER NOT NULL CHECK(action_count>=0), action_set_digest TEXT NOT NULL,
  preservation_verification_count INTEGER NOT NULL CHECK(preservation_verification_count>=0),
  preservation_verification_digest TEXT NOT NULL,
  required_human_resolution_count INTEGER NOT NULL CHECK(required_human_resolution_count>=0),
  required_human_resolution_digest TEXT NOT NULL,
  target_gluing_required INTEGER NOT NULL CHECK(target_gluing_required IN (0,1)),
  source_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,plan_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE gluing_rerun_actions_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  action_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.gluing_rerun_action.v5'),
  planning_scope_id TEXT NOT NULL,
  subject_kind TEXT NOT NULL CHECK(subject_kind IN ('gluing_context','gluing_attempt')),
  subject_ids_canonical_json TEXT NOT NULL,
  action TEXT NOT NULL CHECK(action IN ('register_gluing_input','rebuild_section','reglue')),
  prerequisites_canonical_json TEXT NOT NULL,
  reasons_canonical_json TEXT NOT NULL,
  source_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,action_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE gluing_rerun_plans_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  plan_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.gluing_rerun_plan.v5'),
  planning_scope_id TEXT NOT NULL,
  source_closure_id TEXT NOT NULL REFERENCES incremental_source_closures_v5(closure_id),
  partial_rerun_plan_id TEXT NOT NULL REFERENCES partial_rerun_plans_v5(plan_id),
  target_plan_id TEXT NOT NULL,
  selection_descriptor_id TEXT NOT NULL CHECK(selection_descriptor_id='reviewgraphen.m5_claim_selection@1'),
  claim_bindings_canonical_json TEXT NOT NULL,
  action_count INTEGER NOT NULL CHECK(action_count>=0 AND action_count<=5),
  action_set_digest TEXT NOT NULL,
  existing_target_bundle_witness_canonical_json TEXT,
  source_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,plan_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;
"#;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreIncrementalIndexMarkerV6 {
    pub index_schema_version: u64,
    pub projection_contract_version: String,
    pub event_contract_version: String,
    pub projection_mode: String,
    pub run_id: StableId,
    pub genesis_hash: ContentHash,
    pub confirmed_offset: u64,
    pub tail_hash: ContentHash,
    pub event_count: u64,
    pub policy_revision_hash: ContentHash,
    pub authority_replay_basis_digest: ContentHash,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreIncrementalIndexSnapshotV6 {
    pub marker: PreIncrementalIndexMarkerV6,
    pub repository_id: StableId,
    pub repository_identity_hash: ContentHash,
    pub snapshot_id: StableId,
    pub universe_id: StableId,
    pub plan_id: StableId,
    pub plan_body_hash: ContentHash,
    pub program_space: ProgramSpace,
    pub obligation_ids: Vec<StableId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexRebuildReceiptV6 {
    pub run_id: StableId,
    pub confirmed_offset: u64,
    pub tail_hash: ContentHash,
    pub event_count: u64,
    pub snapshot_hash: ContentHash,
    pub cas_hash: CasHash,
}

/// Durable current-V5 structural projection. Rows retain their event witness and
/// source trace while the state arrays remain intentionally disjoint: an
/// evidence or verification row never becomes a human-accepted row merely by
/// appearing in the same journal.
#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IndexSnapshotV6 {
    pub schema: String,
    pub projection_information_loss: Vec<String>,
    pub source_event_ids: Vec<StableId>,
    pub marker: PreIncrementalIndexMarkerV6,
    pub repository_id: StableId,
    pub snapshot_id: StableId,
    pub profile_id: String,
    pub profile_version: String,
    pub policy_version: String,
    pub rule_set_hash: ContentHash,
    pub extractor_set_hash: ContentHash,
    pub coverage_denominator_ids: Vec<StableId>,
    pub terminal_proof_id: Option<StableId>,
    pub terminal_proof_hash: Option<ContentHash>,
    /// Exact accepted terminal facts from the proof-bound genesis. These are
    /// absent in structural mode when Core has not exposed obligation bodies.
    pub program_objects: Vec<super::IndexProgramObject>,
    pub program_relations: Vec<super::IndexProgramRelation>,
    pub universe: Option<super::IndexUniverse>,
    pub obligations: Vec<super::IndexObligation>,
    /// Frozen V4/M4/D2 rows in their original event order, with complete
    /// closed DTO bodies. They remain separate from M6 so facts, claims,
    /// evidence, verification and decisions cannot be relabelled by moving a
    /// row across a generic state bucket.
    pub inherited_v5_rows: Vec<V5InheritedReportProjection>,
    /// Aggregate claim state reconstructed by the proof-bound terminal
    /// replay. It remains separate from claim/evidence/decision event rows:
    /// no inferred review result is promoted to an accepted claim.
    pub terminal_claim_assessments: Vec<super::v5::IndexClaimAssessmentV3AtV5>,
    /// Historical events whose complete typed body is authority-gated outside
    /// structural replay (currently the V4 gluing bundle). The closed Core
    /// row still retains its exact event witness so the event table cannot
    /// silently omit a durable member while claiming the full prefix.
    pub inherited_event_rows: Vec<V5TypedProjectionRecord>,
    /// Every durable M6 row, with its concrete Core DTO and exact V5 event
    /// witness. This is deliberately distinct from inherited projection data.
    pub m6_rows: Vec<V5TypedProjectionRecord>,
}

/// Receipt for the current V5/v6 rebuild. The image is content addressed;
/// callers compare this hash rather than treating SQLite bytes as canonical.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexRebuildReceiptV6Current {
    pub run_id: StableId,
    pub confirmed_offset: u64,
    pub tail_hash: ContentHash,
    pub event_count: u64,
    pub snapshot_hash: ContentHash,
    pub cas_hash: CasHash,
    pub accounting: IndexAccountingV6,
}

/// Exact resource accounting for the public V6 snapshot materialization.
///
/// `rows`, `integer_cells` and `text_bytes` model the V6 projection tables;
/// the remaining values are measured from the actual typed snapshot, not from
/// an estimate supplied by a caller.  In particular, `query_bytes` is counted
/// before the canonical output vector is allocated and `owned_bytes` is the
/// ADR-0023 recursive ownership charge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexAccountingV6 {
    pub rows: u64,
    pub integer_cells: u64,
    pub text_bytes: u64,
    pub sql_bytes: u64,
    pub query_bytes: u64,
    pub owned_bytes: u64,
    pub observed_object_bytes: u64,
    pub observed_event_line_bytes: u64,
    pub working_bytes: u64,
}

/// Receipt for a roots/CAS-emitted terminal proof that Store has re-read,
/// bound to the locked journal tail, and used for a v6 projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexRebuildReceiptV6Terminal {
    pub rebuild: IndexRebuildReceiptV6Current,
    pub terminal_proof_id: StableId,
    pub terminal_proof_hash: ContentHash,
    pub terminal_proof_cas_hash: CasHash,
    pub terminal_proof_size: u64,
}

pub struct DerivedIndexV6<'root> {
    root: &'root StoreRoot,
}

/// Opaque current V6 target projection.  The retained replay session keeps
/// the journal prefix locked for the lifetime of this value.
pub struct ValidatedIndexSnapshotV6 {
    _session: crate::journal::ReplayedV5RunSession,
    snapshot: PreIncrementalIndexSnapshotV6,
    canonical_bytes: Vec<u8>,
    snapshot_hash: ContentHash,
    cas_hash: CasHash,
    store_root_identity: StoreRootIdentity,
    observed: TargetProjectionObservedV6,
}

struct TargetProjectionObservedV6 {
    largest_cas_bytes: Vec<u8>,
    largest_cas_hash: Option<CasHash>,
    max_event_line_bytes: u64,
    decoded_owned_bytes: u64,
}

#[derive(Serialize)]
struct TargetPolicyIdentityV6<'a> {
    schema: &'static str,
    profile_id: &'a str,
    profile_version: &'a str,
    policy_version: &'a str,
    rule_set_hash: &'a ContentHash,
    extractor_set_hash: &'a ContentHash,
}

#[derive(Serialize)]
struct TargetReplayBasisV6<'a> {
    schema: &'static str,
    run_id: &'a StableId,
    genesis_hash: &'a ContentHash,
    confirmed_offset: u64,
    tail_hash: &'a ContentHash,
    event_count: u64,
    snapshot_id: &'a StableId,
    universe_id: &'a StableId,
    plan_id: &'a StableId,
    plan_body_hash: &'a ContentHash,
    policy_revision_hash: &'a ContentHash,
}

#[derive(Serialize)]
struct CanonicalRepositoryIdentityV6<'a> {
    repository_id: &'a StableId,
    repository_identity: &'a str,
}

pub(crate) fn repository_identity_hash_v6(
    program: &ProgramSpace,
) -> Result<ContentHash, IndexError> {
    Ok(ContentHash::sha256(
        &canonical_json(&CanonicalRepositoryIdentityV6 {
            repository_id: program.repository_id(),
            repository_identity: program.repository_identity(),
        })
        .map_err(|_| IndexError::ProjectionContractViolation)?,
    ))
}

impl<'root> DerivedIndexV6<'root> {
    pub fn open(root: &'root StoreRoot) -> Result<Self, IndexError> {
        // Admission creates and verifies the CAS hierarchy once; neither this
        // handle nor its callers receive a filesystem path.
        let _ = CasStore::open(root)?;
        Ok(Self { root })
    }

    pub(crate) fn matches_store_root(&self, root: &StoreRoot) -> bool {
        self.root.identity() == root.identity()
    }

    pub fn rebuild_pre_incremental_v6(
        &self,
        journal: &EventJournal<'_>,
    ) -> Result<IndexRebuildReceiptV6, IndexError> {
        let session = journal.replayed_v5_target_session()?;
        if session.store_root_identity() != self.root.identity() {
            return Err(IndexError::ProjectionContractViolation);
        }
        let (snapshot, _) = project_complete_target(&session, self.root)?;
        let bytes =
            canonical_json(&snapshot).map_err(|_| IndexError::ProjectionContractViolation)?;
        ensure_snapshot_bytes_v6(self.root, &bytes)?;
        let snapshot_hash = ContentHash::sha256(&bytes);
        let cas_hash = CasHash::parse(snapshot_hash.to_string())?;
        let size = u64::try_from(bytes.len()).map_err(|_| IndexError::IntegerOutOfRange)?;
        CasStore::open(self.root)?.put(&cas_hash, Some(size), Cursor::new(&bytes))?;
        // Publication is not authority until the exact object has been
        // reopened and compared with the canonical typed projection.
        CasStore::open(self.root)?.verify_exact_bytes_streaming(&cas_hash, &bytes)?;
        Ok(IndexRebuildReceiptV6 {
            run_id: snapshot.marker.run_id,
            confirmed_offset: snapshot.marker.confirmed_offset,
            tail_hash: snapshot.marker.tail_hash,
            event_count: snapshot.marker.event_count,
            snapshot_hash,
            cas_hash,
        })
    }

    pub fn validated_snapshot_current_v6(
        &self,
        journal: &EventJournal<'_>,
    ) -> Result<ValidatedIndexSnapshotV6, IndexError> {
        let session = journal.replayed_v5_target_session()?;
        self.validated_snapshot_from_session_v6(session)
    }

    pub(crate) fn validated_snapshot_from_session_v6(
        &self,
        session: crate::journal::ReplayedV5RunSession,
    ) -> Result<ValidatedIndexSnapshotV6, IndexError> {
        if session.store_root_identity() != self.root.identity() {
            return Err(IndexError::ProjectionContractViolation);
        }
        let (snapshot, observed) = project_complete_target(&session, self.root)?;
        let canonical_bytes =
            canonical_json(&snapshot).map_err(|_| IndexError::ProjectionContractViolation)?;
        ensure_snapshot_bytes_v6(self.root, &canonical_bytes)?;
        let snapshot_hash = ContentHash::sha256(&canonical_bytes);
        let cas_hash = CasHash::parse(snapshot_hash.to_string())?;
        CasStore::open(self.root)?.verify_exact_bytes_streaming(&cas_hash, &canonical_bytes)?;
        Ok(ValidatedIndexSnapshotV6 {
            _session: session,
            snapshot,
            canonical_bytes,
            snapshot_hash,
            cas_hash,
            store_root_identity: self.root.identity().clone(),
            observed,
        })
    }

    /// Rebuilds the public, raw-prose-free current structural projection from one exact
    /// durable V5 prefix.  Core performs the closed payload and chain
    /// validation; Store only classifies those validated records and atomically
    /// publishes the canonical snapshot through CAS.
    pub fn rebuild_current_v5_v6(
        &self,
        journal: &EventJournal<'_>,
    ) -> Result<IndexRebuildReceiptV6Current, IndexError> {
        let session = journal.replayed_v5_target_session()?;
        if session.store_root_identity() != self.root.identity() {
            return Err(IndexError::ProjectionContractViolation);
        }
        let (snapshot, _) = project_current_target(&session, self.root)?;
        let connection = build_connection_v6(&snapshot, self.root)?;
        let accounting =
            account_sqlite_snapshot_v6(&connection, &snapshot, session.log(), self.root)?;
        let (bytes, accounting) = canonical_current_snapshot_v6(self.root, &snapshot, accounting)?;
        let snapshot_hash = ContentHash::sha256(&bytes);
        let cas_hash = CasHash::parse(snapshot_hash.to_string())?;
        let size = u64::try_from(bytes.len()).map_err(|_| IndexError::IntegerOutOfRange)?;
        CasStore::open(self.root)?.put(&cas_hash, Some(size), Cursor::new(&bytes))?;
        CasStore::open(self.root)?.verify_exact_bytes_streaming(&cas_hash, &bytes)?;
        Ok(IndexRebuildReceiptV6Current {
            run_id: snapshot.marker.run_id,
            confirmed_offset: snapshot.marker.confirmed_offset,
            tail_hash: snapshot.marker.tail_hash,
            event_count: snapshot.marker.event_count,
            snapshot_hash,
            cas_hash,
            accounting,
        })
    }

    /// Rebuilds a terminal projection only after the caller supplies a proof
    /// that Core binds to this lock-held complete target chain.  The proof is
    /// descriptive; source/M4 report authority is deliberately not inferred
    /// by this target-only index operation.
    pub fn rebuild_terminal_v5_v6(
        &self,
        journal: &EventJournal<'_>,
        proof: &TerminalProofV5,
    ) -> Result<IndexRebuildReceiptV6Terminal, IndexError> {
        let session = journal.replayed_v5_target_session()?;
        if session.store_root_identity() != self.root.identity() {
            return Err(IndexError::ProjectionContractViolation);
        }
        self.rebuild_terminal_from_verified_session_v5_v6(&session, proof)
            .map(|(receipt, _)| receipt)
    }

    #[allow(dead_code)] // Reserved for the dual-lock roots/CAS terminal authority.
    fn rebuild_terminal_from_verified_session_v5_v6(
        &self,
        session: &crate::journal::ReplayedV5RunSession,
        proof: &TerminalProofV5,
    ) -> Result<(IndexRebuildReceiptV6Terminal, IndexSnapshotV6), IndexError> {
        self.rebuild_terminal_from_verified_session_with_inherited_v5_v6(session, proof, None)
    }

    pub fn rebuild_terminal_from_persisted_v5_v6(
        &self,
        journal: &EventJournal<'_>,
        persisted: &crate::journal::PersistedTerminalProofV5,
    ) -> Result<IndexRebuildReceiptV6Terminal, IndexError> {
        let session = journal.replayed_v5_target_session()?;
        if session.store_root_identity() != self.root.identity() {
            return Err(IndexError::ProjectionContractViolation);
        }
        // A persisted marker/proof proves the event chain but cannot recreate
        // the roots/CAS-derived assessment closure. The target-only route may
        // therefore succeed only for a terminal shape with no assessment-
        // dependent rows; SQLite FK validation refuses every richer shape.
        self.rebuild_terminal_from_verified_session_with_inherited_v5_v6(
            &session,
            persisted.proof(),
            None,
        )
        .map(|(receipt, _)| receipt)
    }

    pub(crate) fn rebuild_terminal_from_verified_session_with_inherited_v5_v6(
        &self,
        session: &crate::journal::ReplayedV5RunSession,
        proof: &TerminalProofV5,
        terminal_closure: Option<&V5TerminalReviewClosure>,
    ) -> Result<(IndexRebuildReceiptV6Terminal, IndexSnapshotV6), IndexError> {
        let proof_bytes = proof
            .canonical_bytes()
            .map_err(|_| IndexError::ProjectionContractViolation)?;
        let proof_hash = ContentHash::sha256(&proof_bytes);
        let proof_cas_hash = CasHash::parse(proof_hash.to_string())?;
        let proof_size =
            u64::try_from(proof_bytes.len()).map_err(|_| IndexError::IntegerOutOfRange)?;
        CasStore::open(self.root)?.put(
            &proof_cas_hash,
            Some(proof_size),
            Cursor::new(&proof_bytes),
        )?;
        CasStore::open(self.root)?.verify_exact_bytes_streaming(&proof_cas_hash, &proof_bytes)?;

        session
            .log()
            .verify_terminal_proof_v5(proof)
            .map_err(|_| IndexError::ProjectionContractViolation)?;
        // A gluing terminal is source-bound: its completed bundle and the
        // claim-assessment rows used by the terminal SQLite projection are
        // only available from the roots/CAS-derived Core closure. A durable
        // marker/proof alone must never take this legacy target-only route.
        if terminal_closure.is_none() && proof.completion_kind() == "gluing_bundle_recorded" {
            return Err(IndexError::ProjectionContractViolation);
        }
        let (snapshot, _) = project_current_target_with_proof(
            session,
            self.root,
            Some((proof, proof_hash.clone())),
            terminal_closure,
        )?;
        let connection = build_connection_v6(&snapshot, self.root)?;
        let mut accounting =
            account_sqlite_snapshot_v6(&connection, &snapshot, session.log(), self.root)?;
        let previous_observed_object_bytes = accounting.observed_object_bytes;
        accounting.observed_object_bytes = accounting.observed_object_bytes.max(proof_size);
        accounting.working_bytes = accounting
            .working_bytes
            .checked_add(
                accounting
                    .observed_object_bytes
                    .saturating_sub(previous_observed_object_bytes),
            )
            .ok_or(IndexError::IntegerOutOfRange)?;
        let (bytes, accounting) = canonical_current_snapshot_v6(self.root, &snapshot, accounting)?;
        let snapshot_hash = ContentHash::sha256(&bytes);
        let cas_hash = CasHash::parse(snapshot_hash.to_string())?;
        let size = u64::try_from(bytes.len()).map_err(|_| IndexError::IntegerOutOfRange)?;
        CasStore::open(self.root)?.put(&cas_hash, Some(size), Cursor::new(&bytes))?;
        CasStore::open(self.root)?.verify_exact_bytes_streaming(&cas_hash, &bytes)?;
        Ok((
            IndexRebuildReceiptV6Terminal {
                rebuild: IndexRebuildReceiptV6Current {
                    run_id: snapshot.marker.run_id.clone(),
                    confirmed_offset: snapshot.marker.confirmed_offset,
                    tail_hash: snapshot.marker.tail_hash.clone(),
                    event_count: snapshot.marker.event_count,
                    snapshot_hash,
                    cas_hash,
                    accounting,
                },
                terminal_proof_id: proof.id().clone(),
                terminal_proof_hash: proof_hash,
                terminal_proof_cas_hash: proof_cas_hash,
                terminal_proof_size: proof_size,
            },
            snapshot,
        ))
    }

    /// Reopens the canonical proof derived from the final terminal marker.
    /// This covers the crash window after the marker becomes durable and
    /// before its proof object is published to CAS.
    pub fn rebuild_verified_terminal_v5_v6(
        &self,
        journal: &EventJournal<'_>,
    ) -> Result<IndexRebuildReceiptV6Terminal, IndexError> {
        let session = journal.replayed_v5_target_session()?;
        if session.store_root_identity() != self.root.identity() {
            return Err(IndexError::ProjectionContractViolation);
        }
        let proof = TerminalProofV5::recover_from_terminal_log_for_store(session.log())
            .map_err(|_| IndexError::ProjectionContractViolation)?;
        self.rebuild_terminal_from_verified_session_v5_v6(&session, &proof)
            .map(|(receipt, _)| receipt)
    }

    /// Reopens an already published canonical terminal proof from CAS.  CAS
    /// bytes are decoded canonically and rebound to the lock-held journal;
    /// neither a filename nor a caller-provided JSON proof is accepted.
    pub fn rebuild_terminal_from_proof_cas_v5_v6(
        &self,
        journal: &EventJournal<'_>,
        proof_cas_hash: &CasHash,
        proof_size: u64,
    ) -> Result<IndexRebuildReceiptV6Terminal, IndexError> {
        let session = journal.replayed_v5_target_session()?;
        if session.store_root_identity() != self.root.identity() {
            return Err(IndexError::ProjectionContractViolation);
        }
        let size = usize::try_from(proof_size).map_err(|_| IndexError::IntegerOutOfRange)?;
        let mut bytes = vec![0_u8; size];
        CasReader::open_existing(self.root)?.read_into(
            proof_cas_hash,
            Some(proof_size),
            &mut bytes,
        )?;
        let proof = TerminalProofV5::from_canonical_bytes(&bytes)
            .map_err(|_| IndexError::ProjectionContractViolation)?;
        if ContentHash::sha256(&bytes).to_string() != proof_cas_hash.to_string() {
            return Err(IndexError::ProjectionContractViolation);
        }
        self.rebuild_terminal_from_verified_session_v5_v6(&session, &proof)
            .map(|(receipt, _)| receipt)
    }
}

fn project_current_target(
    session: &crate::journal::ReplayedV5RunSession,
    root: &StoreRoot,
) -> Result<(IndexSnapshotV6, IndexAccountingV6), IndexError> {
    project_current_target_with_proof(session, root, None, None)
}

fn project_terminal_claim_assessments_v6(
    closure: &V5TerminalReviewClosure,
    confirmed_event_sequence: u64,
) -> Result<Vec<super::v5::IndexClaimAssessmentV3AtV5>, IndexError> {
    let mut rows = Vec::new();
    let mut projection_error = None;
    closure
        .visit_claim_assessments_for_store(&mut |value| {
            if projection_error.is_none() {
                match super::v5::project_claim_assessment_v4(value, confirmed_event_sequence) {
                    Ok(row) => rows.push(row),
                    Err(error) => projection_error = Some(error),
                }
            }
            Ok(())
        })
        .map_err(|_| IndexError::ProjectionContractViolation)?;
    if let Some(error) = projection_error {
        return Err(error);
    }
    Ok(rows)
}

fn project_current_target_with_proof(
    session: &crate::journal::ReplayedV5RunSession,
    root: &StoreRoot,
    proof: Option<(&TerminalProofV5, ContentHash)>,
    terminal_closure: Option<&V5TerminalReviewClosure>,
) -> Result<(IndexSnapshotV6, IndexAccountingV6), IndexError> {
    let log = session.log();
    let event_count =
        u64::try_from(log.envelopes().len()).map_err(|_| IndexError::IntegerOutOfRange)?;
    let structural = if proof.is_none() {
        Some(
            log.replay_pre_incremental_structural_prefix_for_store(
                V5StructuralPrefixCoordinates::new(
                    log.run_id(),
                    log.genesis_hash(),
                    session.confirmed_offset(),
                    event_count,
                    log.tail_hash(),
                ),
            )
            .map_err(|_| IndexError::ProjectionContractViolation)?,
        )
    } else {
        None
    };
    let structural_facts = structural.as_ref().map(|value| value.store_facts());
    let terminal_facts = proof
        .as_ref()
        .map(|(proof, _)| log.terminal_index_facts_for_store(proof))
        .transpose()
        .map_err(|_| IndexError::ProjectionContractViolation)?;
    let program = terminal_facts.as_ref().map_or_else(
        || {
            structural_facts
                .as_ref()
                .expect("structural facts")
                .program_space()
        },
        |facts| &facts.program_space,
    );
    let universe = terminal_facts.as_ref().map_or_else(
        || {
            structural_facts
                .as_ref()
                .expect("structural facts")
                .universe()
        },
        |facts| &facts.universe,
    );
    let policy_revision_hash = ContentHash::sha256(
        &canonical_json(&TargetPolicyIdentityV6 {
            schema: "reviewgraphen.target_policy_identity.v6",
            profile_id: program.profile_id(),
            profile_version: program.profile_version(),
            policy_version: program.policy_version(),
            rule_set_hash: program.rule_set_hash(),
            extractor_set_hash: program.extractor_set_hash(),
        })
        .map_err(|_| IndexError::ProjectionContractViolation)?,
    );
    let authority_replay_basis_digest = match proof.as_ref() {
        Some((proof, _)) => proof.authority_replay_basis_digest().clone(),
        None => {
            let plan = structural_facts.as_ref().expect("structural facts").plan();
            ContentHash::sha256(
                &canonical_json(&TargetReplayBasisV6 {
                    schema: "reviewgraphen.v5_current_structural_basis.v1",
                    run_id: log.run_id(),
                    genesis_hash: log.genesis_hash(),
                    confirmed_offset: session.confirmed_offset(),
                    tail_hash: log.tail_hash(),
                    event_count,
                    snapshot_id: program.snapshot_id(),
                    universe_id: universe.id(),
                    plan_id: plan.id(),
                    plan_body_hash: &ContentHash::sha256(
                        &plan
                            .canonical_bytes()
                            .map_err(|_| IndexError::ProjectionContractViolation)?,
                    ),
                    policy_revision_hash: &policy_revision_hash,
                })
                .map_err(|_| IndexError::ProjectionContractViolation)?,
            )
        }
    };
    let marker = PreIncrementalIndexMarkerV6 {
        index_schema_version: INDEX_SCHEMA_VERSION_V6,
        projection_contract_version: "reviewgraphen.index_projection.v6".to_owned(),
        event_contract_version: "reviewgraphen.review_event.v5".to_owned(),
        projection_mode: if proof.is_some() {
            "v5_terminal_proof".to_owned()
        } else {
            "v5_current_structural".to_owned()
        },
        run_id: log.run_id().clone(),
        genesis_hash: log.genesis_hash().clone(),
        confirmed_offset: session.confirmed_offset(),
        tail_hash: log.tail_hash().clone(),
        event_count,
        policy_revision_hash: proof.as_ref().map_or_else(
            || policy_revision_hash.clone(),
            |(proof, _)| proof.policy_revision_hash().clone(),
        ),
        authority_replay_basis_digest,
    };
    // The Core visitor has already revalidated the homogeneous chain and
    // exposes only its closed, typed projection variants.
    const LOSS: [&str; 3] = [
        "payload_bodies_and_raw_reviewer_model_prose",
        "tool_call_traces_and_runtime_capabilities",
        "authority_roots_and_cas_resolver_capabilities",
    ];
    let source_ids = reserve_v6(event_count)?;
    let denominator_ids = reserve_v6(v6_len(universe.obligation_ids().len())?)?;
    let inherited_v5_rows = terminal_closure.map_or_else(
        || {
            log.inherited_report_projection_v5_for_store()
                .map_err(|_| IndexError::ProjectionContractViolation)
        },
        |closure| Ok(closure.inherited_rows().to_vec()),
    )?;
    let terminal_claim_assessments = terminal_closure
        .map(|closure| project_terminal_claim_assessments_v6(closure, event_count))
        .transpose()?
        .unwrap_or_default();
    let inherited_event_ids = inherited_v5_rows
        .iter()
        .map(|row| row.witness().event_id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let mut inherited_event_rows = reserve_v6(event_count)?;
    let mut m6_rows = reserve_v6(event_count)?;
    log.visit_typed_index_projection_v5_for_store(&mut |row| {
        if row.is_m6() {
            m6_rows.push(row);
        } else if !inherited_event_ids.contains(&row.witness().event_id) {
            inherited_event_rows.push(row);
        }
        Ok(())
    })
    .map_err(|_| IndexError::ProjectionContractViolation)?;
    // A roots/CAS-derived gluing descriptor deliberately shares the durable
    // V4 registration's event witness. Count distinct witness events rather
    // than projection rows, so the descriptor enriches that row without
    // claiming a fabricated extra journal member.
    let projected_count = inherited_event_ids
        .len()
        .checked_add(inherited_event_rows.len())
        .ok_or(IndexError::IntegerOutOfRange)?
        .checked_add(m6_rows.len())
        .ok_or(IndexError::IntegerOutOfRange)?;
    if projected_count > usize::try_from(event_count).map_err(|_| IndexError::IntegerOutOfRange)? {
        return Err(IndexError::ProjectionContractViolation);
    }
    let mut program_objects = Vec::new();
    let mut program_relations = Vec::new();
    let mut index_universe = None;
    let mut obligations = Vec::new();
    if let Some(facts) = &terminal_facts {
        for artifact in facts.program_space.artifacts() {
            program_objects.push(super::IndexProgramObject {
                object_id: artifact.id.clone(),
                object_kind: artifact.kind.clone(),
                body_hash: super::body_hash(artifact)?,
            });
        }
        for relation in facts.program_space.relations() {
            program_relations.push(super::IndexProgramRelation {
                relation_id: relation.id.clone(),
                relation_kind: relation.kind.clone(),
                source_id: relation.source_id.clone(),
                target_ids_canonical_json: super::canonical_ids(
                    relation.target_ids.iter().cloned(),
                )?,
                body_hash: super::body_hash(relation)?,
            });
        }
        index_universe = Some(super::IndexUniverse {
            universe_id: facts.universe.id().clone(),
            snapshot_id: facts.universe.snapshot_id().clone(),
            profile_id: facts.universe.profile_id().to_owned(),
            rule_set_hash: facts.universe.rule_set_hash().clone(),
            extractor_set_hash: facts.universe.extractor_set_hash().clone(),
            policy_version: facts.universe.policy_version().to_owned(),
            rule_pack_version: facts.universe.rule_pack_version().to_owned(),
            body_hash: super::body_hash(&facts.universe)?,
        });
        for obligation in &facts.obligations {
            obligations.push(super::IndexObligation {
                obligation_id: obligation.id().clone(),
                target_kind: obligation.target_kind().to_owned(),
                target_ids_canonical_json: super::canonical_ids(
                    obligation.normalized_target_refs().iter().cloned(),
                )?,
                property_id: obligation.property_id().to_owned(),
                lifecycle: super::serialized_enum(&obligation.lifecycle())?,
                body_hash: super::body_hash(obligation)?,
            });
        }
    }
    let mut snapshot = IndexSnapshotV6 {
        marker,
        schema: INDEX_SNAPSHOT_CONTRACT_VERSION_V6.to_owned(),
        projection_information_loss: LOSS.iter().map(|value| (*value).to_owned()).collect(),
        source_event_ids: source_ids,
        repository_id: program.repository_id().clone(),
        snapshot_id: program.snapshot_id().clone(),
        profile_id: program.profile_id().to_owned(),
        profile_version: program.profile_version().to_owned(),
        policy_version: program.policy_version().to_owned(),
        rule_set_hash: program.rule_set_hash().clone(),
        extractor_set_hash: program.extractor_set_hash().clone(),
        coverage_denominator_ids: denominator_ids,
        terminal_proof_id: proof.as_ref().map(|(proof, _)| proof.id().clone()),
        terminal_proof_hash: proof.as_ref().map(|(_, hash)| hash.clone()),
        program_objects,
        program_relations,
        universe: index_universe,
        obligations,
        inherited_v5_rows,
        terminal_claim_assessments,
        inherited_event_rows,
        m6_rows,
    };
    for event in log.envelopes() {
        snapshot.source_event_ids.push(event.id().clone());
    }
    for obligation_id in universe.obligation_ids() {
        snapshot
            .coverage_denominator_ids
            .push(obligation_id.clone());
    }
    // Materialize once before enforcing limits.  The V6 accounting boundary is
    // the actual typed SQLite cells, never a parallel collection estimate.
    let connection = build_connection_v6(&snapshot, root)?;
    let accounting = account_sqlite_snapshot_v6(&connection, &snapshot, log, root)?;
    enforce_current_snapshot_accounting_v6(root, &accounting)?;
    Ok((snapshot, accounting))
}

/// Builds the disposable SQLite representation from the already validated
/// typed projection.  There is deliberately no V5 image reader here: V5 is a
/// journal contract and an existing V5 SQLite file is `RebuildRequired` for
/// this V6 builder.
fn build_connection_v6(
    snapshot: &IndexSnapshotV6,
    root: &StoreRoot,
) -> Result<Connection, IndexError> {
    let limits = IndexLimits::try_from(root.limits())?;
    let connection = Connection::open_in_memory_with_flags(
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.execute_batch("PRAGMA foreign_keys=ON")?;
    // `SCHEMA_V6` is deliberately a single literal; do not derive it from
    // SCHEMA_V5 because that would make an old derived image look admissible.
    connection.execute_batch(SCHEMA_V6)?;
    connection.pragma_update(None, "user_version", 6_i64)?;
    let transaction = connection.unchecked_transaction()?;
    transaction.pragma_update(None, "defer_foreign_keys", true)?;
    insert_snapshot_v6(&transaction, snapshot)?;
    transaction.commit()?;
    assert_m6_table_materialization_v6(&connection, snapshot)?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    let foreign_keys = connection
        .prepare("PRAGMA foreign_key_check")?
        .query([])?
        .next()?
        .is_some();
    if version != 6 || integrity != "ok" || foreign_keys || limits.max_rows == 0 {
        return Err(IndexError::ProjectionContractViolation);
    }
    Ok(connection)
}

/// Ensures every durable M6 family has exactly its declared table image.
/// This is checked against the closed typed enum rather than a payload kind,
/// so an omitted insert cannot be hidden by an event-tuple row.
fn assert_m6_table_materialization_v6(
    connection: &Connection,
    snapshot: &IndexSnapshotV6,
) -> Result<(), IndexError> {
    let expected = |predicate: fn(&V5TypedProjectionRecord) -> bool| -> usize {
        snapshot.m6_rows.iter().filter(|row| predicate(row)).count()
    };
    let families: [(&str, usize); 15] = [
        (
            "incremental_source_closures_v5",
            expected(|row| {
                matches!(
                    row,
                    V5TypedProjectionRecord::IncrementalSourceBoundV5 { .. }
                )
            }),
        ),
        (
            "program_mappings_v5",
            expected(|row| {
                matches!(
                    row,
                    V5TypedProjectionRecord::ProgramMappingRecordedV5 { .. }
                )
            }),
        ),
        (
            "change_morphisms_v5",
            expected(|row| matches!(row, V5TypedProjectionRecord::ChangeMorphismSealedV5 { .. })),
        ),
        (
            "obligation_correspondence_entries_v5",
            expected(|row| {
                matches!(
                    row,
                    V5TypedProjectionRecord::ObligationCorrespondenceEntryRecordedV5 { .. }
                )
            }),
        ),
        (
            "obligation_correspondences_v5",
            expected(|row| {
                matches!(
                    row,
                    V5TypedProjectionRecord::ObligationCorrespondenceSealedV5 { .. }
                )
            }),
        ),
        (
            "historical_record_assessments_v5",
            expected(|row| {
                matches!(
                    row,
                    V5TypedProjectionRecord::HistoricalRecordAssessedV5 { .. }
                )
            }),
        ),
        (
            "gluing_freshness_v5",
            expected(|row| {
                matches!(
                    row,
                    V5TypedProjectionRecord::GluingFreshnessRecordedV5 { .. }
                )
            }),
        ),
        (
            "staleness_assessments_v5",
            expected(|row| {
                matches!(
                    row,
                    V5TypedProjectionRecord::StalenessAssessmentSealedV5 { .. }
                )
            }),
        ),
        (
            "artifact_registrations_v5",
            expected(|row| matches!(row, V5TypedProjectionRecord::ArtifactRegisteredV5 { .. })),
        ),
        (
            "preservation_evidence_v5",
            expected(|row| matches!(row, V5TypedProjectionRecord::PreservationVerifiedV5 { .. })),
        ),
        (
            "preservation_verifications_v5",
            expected(|row| matches!(row, V5TypedProjectionRecord::PreservationVerifiedV5 { .. })),
        ),
        (
            "partial_rerun_actions_v5",
            expected(|row| {
                matches!(
                    row,
                    V5TypedProjectionRecord::PartialRerunActionRecordedV5 { .. }
                )
            }),
        ),
        (
            "partial_rerun_plans_v5",
            expected(|row| {
                matches!(
                    row,
                    V5TypedProjectionRecord::PartialRerunPlanSealedV5 { .. }
                )
            }),
        ),
        (
            "gluing_rerun_actions_v5",
            expected(|row| {
                matches!(
                    row,
                    V5TypedProjectionRecord::GluingRerunActionRecordedV5 { .. }
                )
            }),
        ),
        (
            "gluing_rerun_plans_v5",
            expected(|row| matches!(row, V5TypedProjectionRecord::GluingRerunPlanSealedV5 { .. })),
        ),
    ];
    for (table, expected) in families {
        let observed: i64 =
            connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })?;
        if usize::try_from(observed).map_err(|_| IndexError::IntegerOutOfRange)? != expected {
            return Err(IndexError::ProjectionContractViolation);
        }
    }
    // Count equality is insufficient: verify that each concrete typed row is
    // retrievable at its exact event tuple and that SQLite retained its DTO
    // identity rather than an event-family surrogate. `foreign_key_check` in
    // the caller then verifies every declared target edge of these rows.
    let check = |table: &str,
                 id_column: &str,
                 witness: &reviewgraphen_core::V5ProjectionEventWitness,
                 expected_id: &StableId|
     -> Result<(), IndexError> {
        let sql =
            format!("SELECT {id_column} FROM {table} WHERE event_sequence=?1 AND event_id=?2");
        let observed: String = connection.query_row(
            &sql,
            rusqlite::params![
                super::to_i64(witness.sequence)?,
                witness.event_id.to_string()
            ],
            |row| row.get(0),
        )?;
        if observed != expected_id.as_str() {
            return Err(IndexError::ProjectionContractViolation);
        }
        Ok(())
    };
    for row in &snapshot.m6_rows {
        let witness = row.witness();
        match row {
            V5TypedProjectionRecord::IncrementalSourceBoundV5 { value, .. } => check(
                "incremental_source_closures_v5",
                "closure_id",
                witness,
                value.id(),
            )?,
            V5TypedProjectionRecord::ProgramMappingRecordedV5 { value, .. } => {
                check("program_mappings_v5", "mapping_id", witness, value.id())?
            }
            V5TypedProjectionRecord::ChangeMorphismSealedV5 { value, .. } => {
                check("change_morphisms_v5", "morphism_id", witness, value.id())?
            }
            V5TypedProjectionRecord::ObligationCorrespondenceEntryRecordedV5 { value, .. } => {
                check(
                    "obligation_correspondence_entries_v5",
                    "entry_id",
                    witness,
                    value.id(),
                )?
            }
            V5TypedProjectionRecord::ObligationCorrespondenceSealedV5 { value, .. } => check(
                "obligation_correspondences_v5",
                "correspondence_id",
                witness,
                value.id(),
            )?,
            V5TypedProjectionRecord::HistoricalRecordAssessedV5 { value, .. } => check(
                "historical_record_assessments_v5",
                "record_id",
                witness,
                value.id(),
            )?,
            V5TypedProjectionRecord::GluingFreshnessRecordedV5 { value, .. } => {
                check("gluing_freshness_v5", "freshness_id", witness, value.id())?
            }
            V5TypedProjectionRecord::StalenessAssessmentSealedV5 { value, .. } => check(
                "staleness_assessments_v5",
                "assessment_id",
                witness,
                value.id(),
            )?,
            V5TypedProjectionRecord::ArtifactRegisteredV5 { value, .. } => check(
                "artifact_registrations_v5",
                "registration_id",
                witness,
                value.id(),
            )?,
            V5TypedProjectionRecord::PreservationVerifiedV5 {
                evidence,
                verification,
                ..
            } => {
                check(
                    "preservation_evidence_v5",
                    "evidence_id",
                    witness,
                    evidence.id(),
                )?;
                check(
                    "preservation_verifications_v5",
                    "verification_id",
                    witness,
                    verification.id(),
                )?;
            }
            V5TypedProjectionRecord::PartialRerunActionRecordedV5 { value, .. } => {
                check("partial_rerun_actions_v5", "action_id", witness, value.id())?
            }
            V5TypedProjectionRecord::PartialRerunPlanSealedV5 { value, .. } => {
                check("partial_rerun_plans_v5", "plan_id", witness, value.id())?
            }
            V5TypedProjectionRecord::GluingRerunActionRecordedV5 { value, .. } => {
                check("gluing_rerun_actions_v5", "action_id", witness, value.id())?
            }
            V5TypedProjectionRecord::GluingRerunPlanSealedV5 { value, .. } => {
                check("gluing_rerun_plans_v5", "plan_id", witness, value.id())?
            }
            V5TypedProjectionRecord::TerminalCompletedV5 { .. } => {}
            _ => return Err(IndexError::ProjectionContractViolation),
        }
    }
    Ok(())
}

/// Checks the inherited V3/V4 adapter by reading the actual SQL image back.
/// Counts alone are insufficient: representative identity/body cells and all
/// authority-gated historical event witnesses are compared exactly.
fn assert_inherited_table_materialization_v6(
    connection: &Connection,
    snapshot: &IndexSnapshotV6,
    inherited: &super::v5::IndexSnapshotV5,
) -> Result<(), IndexError> {
    for (table, expected) in [
        ("program_objects", snapshot.program_objects.len()),
        ("program_relations", snapshot.program_relations.len()),
        ("universe", usize::from(snapshot.universe.is_some())),
        ("obligations", snapshot.obligations.len()),
    ] {
        let observed: i64 =
            connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })?;
        if usize::try_from(observed).map_err(|_| IndexError::IntegerOutOfRange)? != expected {
            return Err(IndexError::ProjectionContractViolation);
        }
    }
    for row in &snapshot.program_objects {
        let body: String = connection.query_row(
            "SELECT body_hash FROM program_objects WHERE object_id=?1 AND object_kind=?2",
            rusqlite::params![row.object_id.to_string(), row.object_kind],
            |record| record.get(0),
        )?;
        if body != row.body_hash.as_str() {
            return Err(IndexError::ProjectionContractViolation);
        }
    }
    for row in &snapshot.program_relations {
        let (source, body): (String, String) = connection.query_row(
            "SELECT source_id,body_hash FROM program_relations WHERE relation_id=?1",
            [row.relation_id.to_string()],
            |record| Ok((record.get(0)?, record.get(1)?)),
        )?;
        if source != row.source_id.as_str() || body != row.body_hash.as_str() {
            return Err(IndexError::ProjectionContractViolation);
        }
    }
    if let Some(row) = &snapshot.universe {
        let body: String = connection.query_row(
            "SELECT body_hash FROM universe WHERE universe_id=?1",
            [row.universe_id.to_string()],
            |record| record.get(0),
        )?;
        if body != row.body_hash.as_str() {
            return Err(IndexError::ProjectionContractViolation);
        }
    }
    for row in &snapshot.obligations {
        let (lifecycle, body): (String, String) = connection.query_row(
            "SELECT lifecycle,body_hash FROM obligations WHERE obligation_id=?1",
            [row.obligation_id.to_string()],
            |record| Ok((record.get(0)?, record.get(1)?)),
        )?;
        if lifecycle != row.lifecycle || body != row.body_hash.as_str() {
            return Err(IndexError::ProjectionContractViolation);
        }
    }
    let families = [
        ("obligation_lifecycle", inherited.obligation_lifecycle.len()),
        ("executions", inherited.executions.len()),
        ("claims", inherited.claims.len()),
        (
            "artifact_registrations",
            inherited.artifact_registrations.len(),
        ),
        ("snapshot_source_index", inherited.snapshot_sources.len()),
        ("review_plans", inherited.review_plans.len()),
        ("context_envelopes", inherited.context_envelopes.len()),
        ("evidence_v3", inherited.evidence.len()),
        ("evidence_bindings_v3", inherited.evidence_bindings.len()),
        ("verifications_v3", inherited.verifications.len()),
        ("decisions_v3", inherited.decisions.len()),
        ("findings_v3", inherited.findings.len()),
        ("claim_assessments_v3", inherited.claim_assessments.len()),
        (
            "artifact_registrations_v4",
            inherited.artifact_registrations_v4.len(),
        ),
        (
            "gluing_input_descriptors_v4",
            inherited.gluing_input_descriptors.len(),
        ),
        ("context_covers_v4", inherited.context_covers.len()),
        ("sections_v4", inherited.sections.len()),
        ("restrictions_v4", inherited.restrictions.len()),
        ("gluing_attempts_v4", inherited.gluing_attempts.len()),
        ("global_candidates_v4", inherited.global_candidates.len()),
        (
            "gluing_obstructions_v4",
            inherited.gluing_obstructions.len(),
        ),
    ];
    for (table, expected) in families {
        let observed: i64 =
            connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })?;
        if usize::try_from(observed).map_err(|_| IndexError::IntegerOutOfRange)? != expected {
            return Err(IndexError::ProjectionContractViolation);
        }
    }

    let check_id_body = |table: &str,
                         id_column: &str,
                         sequence: u64,
                         event_id: &StableId,
                         expected_id: &StableId,
                         expected_body: &ContentHash|
     -> Result<(), IndexError> {
        let sql = format!(
            "SELECT {id_column},body_hash FROM {table} WHERE event_sequence=?1 AND event_id=?2 AND {id_column}=?3"
        );
        let (id, body): (String, String) = connection.query_row(
            &sql,
            rusqlite::params![
                super::to_i64(sequence)?,
                event_id.to_string(),
                expected_id.to_string()
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if id != expected_id.as_str() || body != expected_body.as_str() {
            return Err(IndexError::ProjectionContractViolation);
        }
        Ok(())
    };
    for row in &inherited.executions {
        check_id_body(
            "executions",
            "execution_id",
            row.event_sequence,
            &row.event_id,
            &row.execution_id,
            &row.body_hash,
        )?;
    }
    for row in &inherited.claims {
        check_id_body(
            "claims",
            "claim_id",
            row.event_sequence,
            &row.event_id,
            &row.claim_id,
            &row.body_hash,
        )?;
    }
    for row in &inherited.artifact_registrations {
        check_id_body(
            "artifact_registrations",
            "registration_id",
            row.event_sequence,
            &row.event_id,
            &row.registration_id,
            &row.body_hash,
        )?;
    }
    for row in &inherited.evidence {
        check_id_body(
            "evidence_v3",
            "evidence_id",
            row.event_sequence,
            &row.event_id,
            &row.evidence_id,
            &row.body_hash,
        )?;
    }
    for row in &inherited.evidence_bindings {
        check_id_body(
            "evidence_bindings_v3",
            "binding_id",
            row.event_sequence,
            &row.event_id,
            &row.binding_id,
            &row.body_hash,
        )?;
    }
    for row in &inherited.verifications {
        check_id_body(
            "verifications_v3",
            "verification_id",
            row.event_sequence,
            &row.event_id,
            &row.verification_id,
            &row.body_hash,
        )?;
    }
    for row in &inherited.decisions {
        check_id_body(
            "decisions_v3",
            "decision_id",
            row.event_sequence,
            &row.event_id,
            &row.decision_id,
            &row.body_hash,
        )?;
    }
    for row in &inherited.findings {
        check_id_body(
            "findings_v3",
            "finding_id",
            row.event_sequence,
            &row.event_id,
            &row.finding_id,
            &row.body_hash,
        )?;
    }
    for row in &inherited.artifact_registrations_v4 {
        check_id_body(
            "artifact_registrations_v4",
            "registration_id",
            row.event_sequence,
            &row.event_id,
            &row.registration_id,
            &row.body_hash,
        )?;
    }

    for expected in &snapshot.inherited_event_rows {
        let witness = expected.witness();
        let observed: (String, String, String, String, i64) = connection.query_row(
            "SELECT event_hash,payload_hash,payload_kind,actor,logical_time FROM events WHERE sequence=?1 AND event_id=?2",
            rusqlite::params![super::to_i64(witness.sequence)?, witness.event_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )?;
        if observed.0 != witness.event_hash.as_str()
            || observed.1 != witness.payload_hash.as_str()
            || observed.2 != payload_kind_v6(expected)
            || observed.3 != witness.actor
            || observed.4 != super::to_i64(witness.logical_time)?
        {
            return Err(IndexError::ProjectionContractViolation);
        }
    }
    Ok(())
}

fn payload_kind_v6(row: &V5TypedProjectionRecord) -> &'static str {
    use V5TypedProjectionRecord as Row;
    match row {
        Row::RunGenesisManifest { .. } => "run_genesis_manifest",
        Row::ArtifactRegistered { .. } => "artifact_registered",
        Row::SnapshotSourcesRecorded { .. } => "snapshot_sources_recorded",
        Row::ReviewPlanRecorded { .. } => "review_plan_recorded",
        Row::ContextEnvelopeProjected { .. } => "context_envelope_projected",
        Row::ObligationTransition { .. } => "obligation_transition",
        Row::ReviewExecutionRecorded { .. } => "review_execution_recorded",
        Row::FindingRecordedV3 { .. } => "finding_recorded_v3",
        Row::EvidenceRecordedV3 { .. } => "evidence_recorded_v3",
        Row::EvidenceBoundV3 { .. } => "evidence_bound_v3",
        Row::VerificationRecordedV3 { .. } => "verification_recorded_v3",
        Row::DecisionRecordedV3 { .. } => "decision_recorded_v3",
        Row::ArtifactRegisteredV4 { .. } => "artifact_registered_v4",
        Row::GluingBundleRecordedV4 { .. } => "gluing_bundle_recorded_v4",
        Row::IncrementalSourceBoundV5 { .. } => "incremental_source_bound_v5",
        Row::ProgramMappingRecordedV5 { .. } => "program_mapping_recorded_v5",
        Row::ChangeMorphismSealedV5 { .. } => "change_morphism_sealed_v5",
        Row::ObligationCorrespondenceEntryRecordedV5 { .. } => {
            "obligation_correspondence_entry_recorded_v5"
        }
        Row::ObligationCorrespondenceSealedV5 { .. } => "obligation_correspondence_sealed_v5",
        Row::HistoricalRecordAssessedV5 { .. } => "historical_record_assessed_v5",
        Row::GluingFreshnessRecordedV5 { .. } => "gluing_freshness_recorded_v5",
        Row::StalenessAssessmentSealedV5 { .. } => "staleness_assessment_sealed_v5",
        Row::ArtifactRegisteredV5 { .. } => "artifact_registered_v5",
        Row::PreservationVerifiedV5 { .. } => "preservation_verified_v5",
        Row::PartialRerunActionRecordedV5 { .. } => "partial_rerun_action_recorded_v5",
        Row::PartialRerunPlanSealedV5 { .. } => "partial_rerun_plan_sealed_v5",
        Row::GluingRerunActionRecordedV5 { .. } => "gluing_rerun_action_recorded_v5",
        Row::GluingRerunPlanSealedV5 { .. } => "gluing_rerun_plan_sealed_v5",
        Row::TerminalCompletedV5 { .. } => "terminal_completed_v5",
    }
}

fn inherited_payload_kind_v6(row: &V5InheritedReportProjection) -> &'static str {
    use V5InheritedReportProjection as Row;
    match row {
        Row::RunGenesisManifest { .. } => "run_genesis_manifest",
        Row::ArtifactRegistered { .. } => "artifact_registered",
        Row::SnapshotSourcesRecorded { .. } => "snapshot_sources_recorded",
        Row::ReviewPlanRecorded { .. } => "review_plan_recorded",
        Row::ContextEnvelopeProjected { .. } => "context_envelope_projected",
        Row::ObligationTransition { .. } => "obligation_transition",
        Row::ReviewExecutionRecorded { .. } => "review_execution_recorded",
        Row::EvidenceRecordedV3 { .. } => "evidence_recorded_v3",
        Row::EvidenceBoundV3 { .. } => "evidence_bound_v3",
        Row::VerificationRecordedV3 { .. } => "verification_recorded_v3",
        Row::DecisionRecordedV3 { .. } => "decision_recorded_v3",
        Row::FindingRecordedV3 { .. } => "finding_recorded_v3",
        Row::ArtifactRegisteredV4 { .. } => "artifact_registered_v4",
        Row::GluingInputDescriptorV4 { .. } => "gluing_input_descriptor_v4",
        Row::GluingBundleRecordedV4 { .. } => "gluing_bundle_recorded_v4",
    }
}

fn canonical_cell_v6<T: Serialize + ?Sized>(value: &T) -> Result<String, IndexError> {
    let value = serde_json::to_value(value).map_err(|_| IndexError::ProjectionContractViolation)?;
    String::from_utf8(canonical_json(&value).map_err(|_| IndexError::ProjectionContractViolation)?)
        .map_err(|_| IndexError::ProjectionContractViolation)
}

fn enum_cell_v6<T: Serialize>(value: &T) -> Result<String, IndexError> {
    let cell = canonical_cell_v6(value)?;
    serde_json::from_str(&cell).map_err(|_| IndexError::ProjectionContractViolation)
}

fn insert_snapshot_v6(tx: &Transaction<'_>, snapshot: &IndexSnapshotV6) -> Result<(), IndexError> {
    use rusqlite::params;
    let marker = &snapshot.marker;
    tx.execute(
        "INSERT INTO index_meta VALUES(1,6,?1,'reviewgraphen.review_event.v5',?2,?3,?4,?5,?6,?7,?8,?9,NULL,NULL)",
        params![marker.projection_contract_version, marker.projection_mode, marker.run_id.to_string(), marker.genesis_hash.to_string(), super::to_i64(marker.confirmed_offset)?, marker.tail_hash.to_string(), super::to_i64(marker.event_count)?, marker.policy_revision_hash.to_string(), marker.authority_replay_basis_digest.to_string()],
    )?;
    for row in &snapshot.program_objects {
        tx.execute(
            "INSERT INTO program_objects VALUES(?1,?2,?3)",
            rusqlite::params![
                row.object_id.to_string(),
                row.object_kind,
                row.body_hash.to_string()
            ],
        )?;
    }
    for row in &snapshot.program_relations {
        tx.execute(
            "INSERT INTO program_relations VALUES(?1,?2,?3,?4,?5)",
            rusqlite::params![
                row.relation_id.to_string(),
                row.relation_kind,
                row.source_id.to_string(),
                row.target_ids_canonical_json,
                row.body_hash.to_string()
            ],
        )?;
    }
    if let Some(row) = &snapshot.universe {
        tx.execute(
            "INSERT INTO universe VALUES(1,?1,?2,?3,?4,?5,?6,?7,?8)",
            rusqlite::params![
                row.universe_id.to_string(),
                row.snapshot_id.to_string(),
                row.profile_id,
                row.rule_set_hash.to_string(),
                row.extractor_set_hash.to_string(),
                row.policy_version,
                row.rule_pack_version,
                row.body_hash.to_string()
            ],
        )?;
    }
    for row in &snapshot.obligations {
        tx.execute(
            "INSERT INTO obligations VALUES(?1,?2,?3,?4,?5,?6)",
            rusqlite::params![
                row.obligation_id.to_string(),
                row.target_kind,
                row.target_ids_canonical_json,
                row.property_id,
                row.lifecycle,
                row.body_hash.to_string()
            ],
        )?;
    }
    for row in &snapshot.inherited_v5_rows {
        if matches!(
            row,
            V5InheritedReportProjection::GluingInputDescriptorV4 { .. }
        ) {
            continue;
        }
        let witness = row.witness();
        tx.execute(
            "INSERT INTO events VALUES(?1,?2,'reviewgraphen.review_event.v5',?3,?4,?5,?6,?7)",
            params![
                super::to_i64(witness.sequence)?,
                witness.event_id.to_string(),
                witness.event_hash.to_string(),
                witness.payload_hash.to_string(),
                inherited_payload_kind_v6(row),
                witness.actor,
                super::to_i64(witness.logical_time)?
            ],
        )?;
    }
    let mut inherited = super::v5::ReplayProjectionRowsV5::default();
    for row in &snapshot.inherited_v5_rows {
        match row {
            V5InheritedReportProjection::ArtifactRegisteredV4 { witness, .. } => {
                let descriptor =
                    snapshot
                        .inherited_v5_rows
                        .iter()
                        .find_map(|candidate| match candidate {
                            V5InheritedReportProjection::GluingInputDescriptorV4 {
                                witness: descriptor_witness,
                                value: descriptor,
                                ..
                            } if descriptor_witness.event_id == witness.event_id => {
                                Some(descriptor)
                            }
                            _ => None,
                        });
                // A structural target replay does not possess the roots/CAS
                // descriptor. Keep the event witness, but do not fabricate
                // the mutually-bound V4 registration/descriptor SQL pair.
                if let Some(descriptor) = descriptor {
                    inherited.observe_inherited_v6(
                        witness,
                        row.registration_report_view_v4_with_descriptor(descriptor)
                            .map_err(|_| IndexError::ProjectionContractViolation)?,
                    )?;
                }
            }
            V5InheritedReportProjection::GluingInputDescriptorV4 { .. } => {}
            _ => inherited.observe_inherited_v6(row.witness(), row.report_view_v4())?,
        }
    }
    // Reuse V5's closed typed-row SQL adapter with only its inherited family
    // vectors populated. Metadata/events and M6 rows remain owned by V6.
    let inherited_snapshot = super::v5::IndexSnapshotV5 {
        marker: super::v5::IndexMarkerV5 {
            index_schema_version: 5,
            sqlite_user_version: 5,
            projection_contract_version: super::v5::PROJECTION_CONTRACT_VERSION_V5.to_owned(),
            event_contract_version: "reviewgraphen.review_event.v4".to_owned(),
            projection_mode: "v4_gluing".to_owned(),
            run_id: snapshot.marker.run_id.clone(),
            genesis_hash: snapshot.marker.genesis_hash.clone(),
            confirmed_offset: snapshot.marker.confirmed_offset,
            tail_hash: snapshot.marker.tail_hash.clone(),
            event_count: snapshot.marker.event_count,
            policy_revision_hash: snapshot.marker.policy_revision_hash.clone(),
            authority_replay_basis_digest: snapshot.marker.authority_replay_basis_digest.clone(),
        },
        events: Vec::new(),
        shadows: Vec::new(),
        projected_findings: Vec::new(),
        program_objects: Vec::new(),
        program_relations: Vec::new(),
        universe: None,
        obligations: Vec::new(),
        obligation_lifecycle: inherited.obligation_lifecycle,
        executions: inherited.executions,
        claims: inherited.claims,
        artifact_registrations: inherited.artifact_registrations,
        snapshot_sources: inherited.snapshot_sources,
        context_envelopes: inherited.context_envelopes,
        review_plans: inherited.review_plans,
        evidence: inherited.evidence,
        evidence_bindings: inherited.evidence_bindings,
        verifications: inherited.verifications,
        decisions: inherited.decisions,
        findings: inherited.findings,
        // Claim assessments are replay-derived aggregate state. Terminal
        // mode receives them only from the roots/CAS-validated Core closure;
        // structural mode deliberately remains empty rather than fabricating
        // an assessment from event metadata.
        claim_assessments: snapshot.terminal_claim_assessments.clone(),
        artifact_registrations_v4: inherited.artifact_registrations_v4,
        gluing_input_descriptors: inherited.gluing_input_descriptors,
        context_covers: inherited.context_covers,
        sections: inherited.sections,
        restrictions: inherited.restrictions,
        gluing_attempts: inherited.gluing_attempts,
        global_candidates: inherited.global_candidates,
        gluing_obstructions: inherited.gluing_obstructions,
        policy_revision_hash: snapshot.marker.policy_revision_hash.clone(),
        authority_replay_basis_digest: snapshot.marker.authority_replay_basis_digest.clone(),
    };
    insert_inherited_snapshot_tables_v6(tx, &inherited_snapshot)?;
    for row in &snapshot.inherited_event_rows {
        if snapshot
            .inherited_v5_rows
            .iter()
            .any(|inherited| inherited.witness().event_id == row.witness().event_id)
        {
            continue;
        }
        let witness = row.witness();
        tx.execute(
            "INSERT INTO events VALUES(?1,?2,'reviewgraphen.review_event.v5',?3,?4,?5,?6,?7)",
            params![
                super::to_i64(witness.sequence)?,
                witness.event_id.to_string(),
                witness.event_hash.to_string(),
                witness.payload_hash.to_string(),
                payload_kind_v6(row),
                witness.actor,
                super::to_i64(witness.logical_time)?
            ],
        )?;
    }
    for row in &snapshot.m6_rows {
        let witness = row.witness();
        tx.execute(
            "INSERT INTO events VALUES(?1,?2,'reviewgraphen.review_event.v5',?3,?4,?5,?6,?7)",
            params![
                super::to_i64(witness.sequence)?,
                witness.event_id.to_string(),
                witness.event_hash.to_string(),
                witness.payload_hash.to_string(),
                payload_kind_v6(row),
                witness.actor,
                super::to_i64(witness.logical_time)?
            ],
        )?;
        if let V5TypedProjectionRecord::ArtifactRegisteredV5 { value, .. } = row {
            let value = value
                .index_projection_v6()
                .map_err(|_| IndexError::ProjectionContractViolation)?;
            tx.execute(
                "INSERT INTO artifact_registrations_v5 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                params![
                    super::to_i64(witness.sequence)?, witness.event_id.to_string(),
                    value.registration_id.to_string(), value.schema, value.run_id.to_string(),
                    value.cas_hash.to_string(), value.media_type, super::to_i64(value.size)?,
                    value.sensitivity, value.source_kind, value.source_canonical_json,
                    value.body_hash.to_string(),
                ],
            )?;
        }
        if let V5TypedProjectionRecord::IncrementalSourceBoundV5 { value, .. } = row {
            let value = value.report_projection();
            let body_hash = row.witness().body_hash.to_string();
            tx.execute(
                "INSERT INTO incremental_source_closures_v5 VALUES(?1,?2,?3,'reviewgraphen.incremental_source_closure.v5',?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31,?32,?33)",
                params![
                    super::to_i64(witness.sequence)?, witness.event_id.to_string(), row.record_ids()[0].to_string(),
                    value.repository_id.to_string(), value.repository_identity_hash.to_string(), value.source_run_id.to_string(),
                    value.source_genesis_hash.to_string(), super::to_i64(value.source_confirmed_offset)?, value.source_tail_hash.to_string(),
                    super::to_i64(value.source_event_count)?, value.source_snapshot_id.to_string(), value.source_universe_id.to_string(),
                    value.source_index_snapshot_hash.to_string(), value.source_authority_policy_revision_hash.to_string(),
                    value.source_authority_replay_basis_digest.to_string(), value.source_resolved_target_commit_oid,
                    value.source_target_tree_hash.to_string(), value.source_gluing_bundle_id.to_string(), value.target_run_id.to_string(),
                    value.target_genesis_hash.to_string(), super::to_i64(value.target_predecessor_offset)?, value.target_predecessor_tail_hash.to_string(),
                    super::to_i64(value.target_predecessor_event_count)?, value.target_snapshot_id.to_string(), value.target_universe_id.to_string(),
                    value.target_predecessor_index_snapshot_hash.to_string(), value.target_authority_policy_revision_hash.to_string(),
                    value.target_pre_incremental_authority_replay_basis_digest.to_string(), value.target_resolved_base_commit_oid,
                    value.target_base_tree_hash.to_string(), value.target_resolved_target_commit_oid, value.target_target_tree_hash.to_string(), body_hash,
                ],
            )?;
        }
        if let V5TypedProjectionRecord::ProgramMappingRecordedV5 { value, .. } = row {
            tx.execute(
                "INSERT INTO program_mappings_v5 VALUES(?1,?2,?3,'reviewgraphen.program_mapping.v5',?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
                params![
                    super::to_i64(witness.sequence)?, witness.event_id.to_string(), value.id().to_string(),
                    value.source_closure_id().to_string(), value.source_snapshot_id().to_string(), value.target_snapshot_id().to_string(),
                    enum_cell_v6(&value.object_kind())?, canonical_cell_v6(value.from_ids())?, canonical_cell_v6(value.to_ids())?,
                    enum_cell_v6(&value.status())?, enum_cell_v6(&value.candidate_key_kind())?, canonical_cell_v6(value.source_body_hashes())?,
                    canonical_cell_v6(value.target_body_hashes())?, canonical_cell_v6(value.change_fact_ids())?,
                    canonical_cell_v6(value.predecessor_mapping_ids())?, canonical_cell_v6(value.successor_ids())?, canonical_cell_v6(value.source_ids())?,
                    value.body_hash().map_err(|_| IndexError::ProjectionContractViolation)?.to_string(),
                ],
            )?;
        }
        if let V5TypedProjectionRecord::ChangeMorphismSealedV5 { value, .. } = row {
            tx.execute(
                "INSERT INTO change_morphisms_v5 VALUES(?1,?2,?3,'reviewgraphen.change_morphism.v5',?4,?5,?6,?7,'reviewgraphen.program_mapping@1','reviewgraphen.rust_symbol_anchor@1',?8,?9,?10,?11,?12,?13,?14,?15,?16)",
                params![
                    super::to_i64(witness.sequence)?, witness.event_id.to_string(), value.id().to_string(),
                    value.source_closure_id().to_string(), value.repository_id().to_string(), value.source_snapshot_id().to_string(), value.target_snapshot_id().to_string(),
                    super::to_i64(value.mapping_count())?, value.mapping_set_digest().to_string(), super::to_i64(value.source_domain_count())?,
                    value.source_domain_digest().to_string(), super::to_i64(value.target_domain_count())?, value.target_domain_digest().to_string(),
                    canonical_cell_v6(value.status_counts())?, canonical_cell_v6(value.source_ids())?, value.body_hash().map_err(|_| IndexError::ProjectionContractViolation)?.to_string(),
                ],
            )?;
        }
        if let V5TypedProjectionRecord::ObligationCorrespondenceEntryRecordedV5 { value, .. } = row
        {
            tx.execute(
                "INSERT INTO obligation_correspondence_entries_v5 VALUES(?1,?2,?3,'reviewgraphen.obligation_correspondence_entry.v5',?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                params![
                    super::to_i64(witness.sequence)?, witness.event_id.to_string(), value.id().to_string(), value.morphism_id().to_string(),
                    canonical_cell_v6(value.from_obligation_ids())?, canonical_cell_v6(value.to_obligation_ids())?, enum_cell_v6(&value.status())?,
                    canonical_cell_v6(value.source_mapping_ids())?, canonical_cell_v6(value.predecessor_entry_ids())?, canonical_cell_v6(value.successor_obligation_ids())?,
                    canonical_cell_v6(value.source_body_hashes())?, canonical_cell_v6(value.target_body_hashes())?, canonical_cell_v6(value.source_ids())?,
                    value.body_hash().map_err(|_| IndexError::ProjectionContractViolation)?.to_string(),
                ],
            )?;
        }
        if let V5TypedProjectionRecord::ObligationCorrespondenceSealedV5 { value, .. } = row {
            tx.execute(
                "INSERT INTO obligation_correspondences_v5 VALUES(?1,?2,?3,'reviewgraphen.obligation_correspondence.v5',?4,?5,?6,'reviewgraphen.obligation_correspondence@1',?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                params![
                    super::to_i64(witness.sequence)?, witness.event_id.to_string(), value.id().to_string(), value.morphism_id().to_string(),
                    value.source_universe_id().to_string(), value.target_universe_id().to_string(), super::to_i64(value.entry_count())?, value.entry_set_digest().to_string(),
                    super::to_i64(value.source_domain_count())?, value.source_domain_digest().to_string(), super::to_i64(value.target_domain_count())?,
                    value.target_domain_digest().to_string(), canonical_cell_v6(value.status_counts())?, canonical_cell_v6(value.source_ids())?,
                    value.body_hash().map_err(|_| IndexError::ProjectionContractViolation)?.to_string(),
                ],
            )?;
        }
        if let V5TypedProjectionRecord::HistoricalRecordAssessedV5 { value, .. } = row {
            tx.execute(
                "INSERT INTO historical_record_assessments_v5 VALUES(?1,?2,?3,'reviewgraphen.historical_record_assessment.v5',?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
                params![
                    super::to_i64(witness.sequence)?, witness.event_id.to_string(), value.id().to_string(), value.assessment_id().to_string(),
                    enum_cell_v6(&value.source_record_kind())?, value.source_record_id().to_string(), value.source_record_body_hash().to_string(),
                    canonical_cell_v6(value.successor_record_ids())?, enum_cell_v6(&value.status())?, enum_cell_v6(&value.directness())?,
                    canonical_cell_v6(value.reasons())?, canonical_cell_v6(value.dependency_source_ids())?, canonical_cell_v6(value.mapping_ids())?,
                    canonical_cell_v6(value.correspondence_entry_ids())?, canonical_cell_v6(value.source_ids())?, value.body_hash().map_err(|_| IndexError::ProjectionContractViolation)?.to_string(),
                ],
            )?;
        }
        if let V5TypedProjectionRecord::GluingFreshnessRecordedV5 { value, .. } = row {
            tx.execute(
                "INSERT INTO gluing_freshness_v5 VALUES(?1,?2,?3,'reviewgraphen.gluing_freshness.v5',?4,?5,?6,?7,?8,?9,?10,?11)",
                params![
                    super::to_i64(witness.sequence)?, witness.event_id.to_string(), value.id().to_string(), value.assessment_id().to_string(),
                    value.source_attempt_id().to_string(), enum_cell_v6(&value.status())?, canonical_cell_v6(value.reasons())?,
                    canonical_cell_v6(value.dependency_mapping_ids())?, canonical_cell_v6(value.successor_target_attempt_ids())?, canonical_cell_v6(value.source_ids())?,
                    value.body_hash().map_err(|_| IndexError::ProjectionContractViolation)?.to_string(),
                ],
            )?;
        }
        if let V5TypedProjectionRecord::StalenessAssessmentSealedV5 { value, .. } = row {
            tx.execute(
                "INSERT INTO staleness_assessments_v5 VALUES(?1,?2,?3,'reviewgraphen.staleness_assessment.v5',?4,?5,?6,'reviewgraphen.mvp_property_impact@1',?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)",
                params![
                    super::to_i64(witness.sequence)?, witness.event_id.to_string(), value.id().to_string(), value.source_closure_id().to_string(),
                    value.morphism_id().to_string(), value.correspondence_id().to_string(), value.assessment_time(), super::to_i64(value.record_count())?,
                    value.record_set_digest().to_string(), super::to_i64(value.gluing_freshness_count())?, value.gluing_freshness_set_digest().to_string(),
                    super::to_i64(value.stale_source_count())?, value.stale_source_digest().to_string(), super::to_i64(value.superseded_source_count())?,
                    value.superseded_source_digest().to_string(), super::to_i64(value.preservation_candidate_count())?, value.preservation_candidate_digest().to_string(),
                    super::to_i64(value.m5_dependent_successor_count())?, value.m5_dependent_successor_digest().to_string(), canonical_cell_v6(value.source_ids())?,
                    value.body_hash().map_err(|_| IndexError::ProjectionContractViolation)?.to_string(),
                ],
            )?;
        }
        if let V5TypedProjectionRecord::PreservationVerifiedV5 {
            evidence,
            verification,
            ..
        } = row
        {
            let value = evidence
                .index_projection_v6()
                .map_err(|_| IndexError::ProjectionContractViolation)?;
            tx.execute(
                "INSERT INTO preservation_evidence_v5 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
                params![
                    super::to_i64(witness.sequence)?, witness.event_id.to_string(), value.evidence_id.to_string(), value.schema,
                    value.target_snapshot_id.to_string(), value.target_obligation_id.to_string(), value.source_closure_id.to_string(),
                    value.morphism_id.to_string(), value.correspondence_entry_id.to_string(), value.source_claim_id.to_string(),
                    value.source_evidence_ids_canonical_json, value.source_verification_id.to_string(), value.dependency_mapping_ids_canonical_json,
                    value.input_registration_id.to_string(), value.output_registration_id.to_string(), value.descriptor_id, value.procedure_version,
                    value.observation, value.source_ids_canonical_json, value.body_hash.to_string(),
                ],
            )?;
            let verification = verification
                .index_projection_v6()
                .map_err(|_| IndexError::ProjectionContractViolation)?;
            tx.execute(
                "INSERT INTO preservation_verifications_v5 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                params![
                    super::to_i64(witness.sequence)?, witness.event_id.to_string(), verification.verification_id.to_string(), verification.schema,
                    verification.target_snapshot_id.to_string(), verification.target_obligation_id.to_string(), verification.evidence_id.to_string(),
                    verification.source_verification_id.to_string(), verification.descriptor_id, verification.procedure_version, verification.outcome,
                    verification.source_ids_canonical_json, verification.body_hash.to_string(),
                ],
            )?;
        }
        if let V5TypedProjectionRecord::PartialRerunActionRecordedV5 { value, .. } = row {
            tx.execute(
                "INSERT INTO partial_rerun_actions_v5 VALUES(?1,?2,?3,'reviewgraphen.partial_rerun_action.v5',?4,'obligation',?5,?6,?7,?8,?9,?10,?11)",
                params![
                    super::to_i64(witness.sequence)?, witness.event_id.to_string(), value.id().to_string(),
                    value.staleness_assessment_id().to_string(), canonical_cell_v6(value.subject_ids())?, enum_cell_v6(&value.action())?,
                    canonical_cell_v6(value.prerequisites())?, canonical_cell_v6(value.stale_source_record_ids())?, canonical_cell_v6(value.reasons())?,
                    canonical_cell_v6(value.source_ids())?, value.body_hash().map_err(|_| IndexError::ProjectionContractViolation)?.to_string(),
                ],
            )?;
        }
        if let V5TypedProjectionRecord::PartialRerunPlanSealedV5 { value, .. } = row {
            tx.execute(
                "INSERT INTO partial_rerun_plans_v5 VALUES(?1,?2,?3,'reviewgraphen.partial_rerun_plan.v5',?4,?5,?6,?7,'reviewgraphen.partial_rerun@1',?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
                params![
                    super::to_i64(witness.sequence)?, witness.event_id.to_string(), value.id().to_string(),
                    value.source_closure_id().to_string(), value.morphism_id().to_string(), value.correspondence_id().to_string(),
                    value.staleness_assessment_id().to_string(), value.target_plan_id().to_string(), super::to_i64(value.selected_target_count())?,
                    value.selected_target_digest().to_string(), super::to_i64(value.action_count())?, value.action_set_digest().to_string(),
                    super::to_i64(value.preservation_verification_count())?, value.preservation_verification_digest().to_string(),
                    super::to_i64(value.required_human_resolution_count())?, value.required_human_resolution_digest().to_string(),
                    i64::from(value.target_gluing_required()), canonical_cell_v6(value.source_ids())?,
                    value.body_hash().map_err(|_| IndexError::ProjectionContractViolation)?.to_string(),
                ],
            )?;
        }
        if let V5TypedProjectionRecord::GluingRerunActionRecordedV5 { value, .. } = row {
            tx.execute(
                "INSERT INTO gluing_rerun_actions_v5 VALUES(?1,?2,?3,'reviewgraphen.gluing_rerun_action.v5',?4,?5,?6,?7,?8,?9,?10,?11)",
                params![
                    super::to_i64(witness.sequence)?, witness.event_id.to_string(), value.id().to_string(), value.planning_scope_id().to_string(),
                    enum_cell_v6(&value.subject_kind())?, canonical_cell_v6(value.subject_ids())?, enum_cell_v6(&value.action())?,
                    canonical_cell_v6(value.prerequisites())?, canonical_cell_v6(value.reasons())?, canonical_cell_v6(value.source_ids())?,
                    value.body_hash().map_err(|_| IndexError::ProjectionContractViolation)?.to_string(),
                ],
            )?;
        }
        if let V5TypedProjectionRecord::GluingRerunPlanSealedV5 { value, .. } = row {
            tx.execute(
                "INSERT INTO gluing_rerun_plans_v5 VALUES(?1,?2,?3,'reviewgraphen.gluing_rerun_plan.v5',?4,?5,?6,?7,'reviewgraphen.m5_claim_selection@1',?8,?9,?10,?11,?12,?13)",
                params![
                    super::to_i64(witness.sequence)?, witness.event_id.to_string(), value.id().to_string(), value.planning_scope_id().to_string(),
                    value.source_closure_id().to_string(), value.partial_rerun_plan_id().to_string(), value.target_plan_id().to_string(),
                    canonical_cell_v6(value.claim_bindings())?, super::to_i64(value.action_count())?, value.action_set_digest().to_string(),
                    value.existing_target_bundle_witness().map(canonical_cell_v6).transpose()?, canonical_cell_v6(value.source_ids())?,
                    value.body_hash().map_err(|_| IndexError::ProjectionContractViolation)?.to_string(),
                ],
            )?;
        }
    }
    assert_inherited_table_materialization_v6(tx, snapshot, &inherited_snapshot)?;
    Ok(())
}

/// Inserts only V3/V4 typed family tables. The V6 metadata/event/program/M6
/// families are already populated by their own contract and must not be
/// overwritten by V5's marker or event adapter.
fn insert_inherited_snapshot_tables_v6(
    tx: &Transaction<'_>,
    inherited: &super::v5::IndexSnapshotV5,
) -> Result<(), IndexError> {
    super::v5::insert_inherited_tables_v6(tx, inherited)
}

/// Measures Rows6/Icells6/Tbytes6 by reading the actual SQLite values that
/// were inserted.  Accounting never synthesizes table rows from collection
/// lengths, which keeps future schema changes visible to the resource budget.
fn account_sqlite_snapshot_v6(
    connection: &Connection,
    snapshot: &IndexSnapshotV6,
    log: &EventLogV5,
    root: &StoreRoot,
) -> Result<IndexAccountingV6, IndexError> {
    let names = [
        "index_meta",
        "events",
        "program_objects",
        "program_relations",
        "universe",
        "obligations",
        "obligation_lifecycle",
        "executions",
        "claims",
        "artifact_registrations",
        "snapshot_source_index",
        "review_plans",
        "context_envelopes",
        "unreconciled_authority_records",
        "projected_findings",
        "evidence_v3",
        "evidence_bindings_v3",
        "verifications_v3",
        "decisions_v3",
        "findings_v3",
        "claim_assessments_v3",
        "artifact_registrations_v4",
        "gluing_input_descriptors_v4",
        "context_covers_v4",
        "sections_v4",
        "gluing_attempts_v4",
        "restrictions_v4",
        "global_candidates_v4",
        "gluing_obstructions_v4",
        "artifact_registrations_v5",
        "incremental_source_closures_v5",
        "program_mappings_v5",
        "change_morphisms_v5",
        "obligation_correspondence_entries_v5",
        "obligation_correspondences_v5",
        "historical_record_assessments_v5",
        "gluing_freshness_v5",
        "staleness_assessments_v5",
        "preservation_evidence_v5",
        "preservation_verifications_v5",
        "partial_rerun_actions_v5",
        "partial_rerun_plans_v5",
        "gluing_rerun_actions_v5",
        "gluing_rerun_plans_v5",
    ];
    let mut rows = 0_u64;
    let mut integer_cells = 0_u64;
    let mut text_bytes = 0_u64;
    for name in names {
        let mut statement = connection.prepare(&format!("SELECT * FROM {name}"))?;
        let columns = statement.column_count();
        let mut query = statement.query([])?;
        while let Some(row) = query.next()? {
            add_v6(&mut rows, 1)?;
            for column in 0..columns {
                match row.get_ref(column)? {
                    ValueRef::Null => {}
                    ValueRef::Integer(_) | ValueRef::Real(_) => add_v6(&mut integer_cells, 1)?,
                    ValueRef::Text(value) => add_v6(&mut text_bytes, v6_len(value.len())?)?,
                    ValueRef::Blob(_) => return Err(IndexError::ProjectionContractViolation),
                }
            }
        }
    }
    let sql_bytes = text_bytes
        .checked_add(
            integer_cells
                .checked_mul(8)
                .ok_or(IndexError::IntegerOutOfRange)?,
        )
        .and_then(|value| value.checked_add(rows))
        .ok_or(IndexError::IntegerOutOfRange)?;
    let query_bytes = v6_json_length(snapshot)?;
    let owned_bytes = super::v5::recursive_ownership_charge(snapshot)?;
    let observed_event_line_bytes = log
        .maximum_canonical_event_line_bytes_for_store()
        .map_err(|_| IndexError::ProjectionContractViolation)?;
    let observed_object_bytes = largest_registered_cas_object_bytes_v6(snapshot, root)?;
    let working_bytes = sql_bytes
        .checked_add(query_bytes)
        .and_then(|value| value.checked_add(owned_bytes))
        .and_then(|value| value.checked_add(observed_object_bytes))
        .and_then(|value| value.checked_add(observed_event_line_bytes))
        .ok_or(IndexError::IntegerOutOfRange)?;
    Ok(IndexAccountingV6 {
        rows,
        integer_cells,
        text_bytes,
        sql_bytes,
        query_bytes,
        owned_bytes,
        observed_object_bytes,
        observed_event_line_bytes,
        working_bytes,
    })
}

fn add_v6(total: &mut u64, value: u64) -> Result<(), IndexError> {
    *total = total
        .checked_add(value)
        .ok_or(IndexError::IntegerOutOfRange)?;
    Ok(())
}

/// Reads each CAS object named by the closed typed projection and returns the
/// largest actual object size. Declared registration sizes are coordinates,
/// not observations; the V6 working-set oracle therefore charges bytes that
/// were successfully read and hash-checked by Store.
fn largest_registered_cas_object_bytes_v6(
    snapshot: &IndexSnapshotV6,
    root: &StoreRoot,
) -> Result<u64, IndexError> {
    let mut coordinates = Vec::new();
    for row in &snapshot.inherited_v5_rows {
        match row {
            V5InheritedReportProjection::RunGenesisManifest { value, .. } => {
                let registration = value.genesis_artifact();
                coordinates.push((registration.cas_hash().clone(), registration.size()));
            }
            V5InheritedReportProjection::ArtifactRegistered { value, .. } => {
                coordinates.push((value.cas_hash().clone(), value.size()));
            }
            V5InheritedReportProjection::ArtifactRegisteredV4 { value, .. } => {
                coordinates.push((value.cas_hash().clone(), value.size()));
            }
            _ => {}
        }
    }
    for row in &snapshot.m6_rows {
        if let V5TypedProjectionRecord::ArtifactRegisteredV5 { value, .. } = row {
            let value = value
                .index_projection_v6()
                .map_err(|_| IndexError::ProjectionContractViolation)?;
            coordinates.push((value.cas_hash, value.size));
        }
    }
    coordinates.sort();
    coordinates.dedup();
    let reader = crate::CasReader::open_existing(root)?;
    let mut largest = 0_u64;
    for (hash, size) in coordinates {
        let cas_hash = CasHash::parse(hash.to_string())?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(usize::try_from(size).map_err(|_| IndexError::IntegerOutOfRange)?)
            .map_err(|_| IndexError::Incomplete {
                limit: root.limits().max_index_working_bytes,
                observed: size,
            })?;
        reader.read_into(&cas_hash, Some(size), &mut bytes)?;
        if ContentHash::sha256(&bytes) != hash {
            return Err(IndexError::ProjectionContractViolation);
        }
        largest = largest.max(v6_len(bytes.len())?);
    }
    Ok(largest)
}

fn v6_len(value: usize) -> Result<u64, IndexError> {
    u64::try_from(value).map_err(|_| IndexError::IntegerOutOfRange)
}

/// Counts the compact canonical JSON wire size without retaining the output.
/// Core canonicalization only permutes object keys, which cannot alter the
/// compact byte count.  This is deliberately separate from `canonical_json`:
/// the admission check has to run before a snapshot-sized `Vec<u8>` exists.
#[derive(Default)]
struct V6CountingWriter {
    bytes: u64,
}

impl std::io::Write for V6CountingWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(v6_len(bytes.len()).map_err(|_| std::io::Error::other("length"))?)
            .ok_or_else(|| std::io::Error::other("overflow"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn v6_json_length<T: ?Sized + Serialize>(value: &T) -> Result<u64, IndexError> {
    let mut writer = V6CountingWriter::default();
    serde_json::to_writer(&mut writer, value)
        .map_err(|_| IndexError::ProjectionContractViolation)?;
    Ok(writer.bytes)
}

fn reserve_v6<T>(count: u64) -> Result<Vec<T>, IndexError> {
    let count = usize::try_from(count).map_err(|_| IndexError::IntegerOutOfRange)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| IndexError::Incomplete {
            limit: u64::try_from(count).unwrap_or(u64::MAX),
            observed: u64::try_from(count).unwrap_or(u64::MAX),
        })?;
    Ok(values)
}

fn enforce_current_snapshot_accounting_v6(
    root: &StoreRoot,
    accounting: &IndexAccountingV6,
) -> Result<(), IndexError> {
    if accounting.rows > root.limits().max_index_rows {
        return Err(IndexError::Incomplete {
            limit: root.limits().max_index_rows,
            observed: accounting.rows,
        });
    }
    if accounting.query_bytes > root.limits().max_index_serialized_bytes
        || accounting.query_bytes > root.limits().max_index_query_bytes
    {
        return Err(IndexError::Incomplete {
            limit: root
                .limits()
                .max_index_serialized_bytes
                .min(root.limits().max_index_query_bytes),
            observed: accounting.query_bytes,
        });
    }
    if accounting.working_bytes > root.limits().max_index_working_bytes {
        return Err(IndexError::Incomplete {
            limit: root.limits().max_index_working_bytes,
            observed: accounting.working_bytes,
        });
    }
    Ok(())
}

fn canonical_current_snapshot_v6(
    root: &StoreRoot,
    snapshot: &IndexSnapshotV6,
    accounting: IndexAccountingV6,
) -> Result<(Vec<u8>, IndexAccountingV6), IndexError> {
    // `query_bytes` was measured by the streaming counting writer during the
    // accounting pass.  Admit that exact measurement before `canonical_json`
    // can allocate or encode the snapshot-sized output vector.
    enforce_current_snapshot_accounting_v6(root, &accounting)?;
    record_v6_canonical_snapshot_materialization();
    let bytes = canonical_json(snapshot).map_err(|_| IndexError::ProjectionContractViolation)?;
    if v6_len(bytes.len())? != accounting.query_bytes {
        return Err(IndexError::ProjectionContractViolation);
    }
    Ok((bytes, accounting))
}

fn ensure_snapshot_bytes_v6(root: &StoreRoot, bytes: &[u8]) -> Result<(), IndexError> {
    let observed = u64::try_from(bytes.len()).map_err(|_| IndexError::IntegerOutOfRange)?;
    let limit = root
        .limits()
        .max_index_serialized_bytes
        .min(root.limits().max_index_query_bytes)
        .min(root.limits().max_index_working_bytes);
    if observed > limit {
        return Err(IndexError::Incomplete { limit, observed });
    }
    Ok(())
}

impl ValidatedIndexSnapshotV6 {
    /// Executes one Store-internal operation while the V5 journal lock and
    /// replay session retained by this validated snapshot remain live.  The
    /// session itself never escapes the index boundary.
    pub(crate) fn with_session_mut<T>(
        &mut self,
        operation: impl FnOnce(&mut crate::journal::ReplayedV5RunSession) -> Result<T, IndexError>,
    ) -> Result<T, IndexError> {
        operation(&mut self._session)
    }

    #[must_use]
    pub const fn snapshot(&self) -> &PreIncrementalIndexSnapshotV6 {
        &self.snapshot
    }

    #[must_use]
    pub fn snapshot_hash(&self) -> &ContentHash {
        &self.snapshot_hash
    }

    pub(crate) fn store_root_identity(&self) -> &StoreRootIdentity {
        &self.store_root_identity
    }

    pub(crate) fn canonical_snapshot_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }

    pub(crate) const fn decoded_owned_bytes(&self) -> u64 {
        self.observed.decoded_owned_bytes
    }

    pub(crate) const fn max_cas_bytes(&self) -> u64 {
        self.observed.largest_cas_bytes.len() as u64
    }

    pub(crate) fn largest_verified_cas_bytes(&self) -> &[u8] {
        &self.observed.largest_cas_bytes
    }

    pub(crate) const fn max_event_line_bytes(&self) -> u64 {
        self.observed.max_event_line_bytes
    }

    pub(crate) fn confirmed_journal_bytes(&self) -> u64 {
        self._session.confirmed_offset()
    }

    pub(crate) fn retain_confirmed_journal_prefix(&mut self) -> Result<Vec<u8>, IndexError> {
        self._session
            .retain_confirmed_prefix_bytes()
            .map_err(Into::into)
    }

    pub(crate) fn event_log(&self) -> &reviewgraphen_core::EventLogV5 {
        self._session.log()
    }

    pub(crate) fn revalidate_cas(&self, root: &StoreRoot) -> Result<(), IndexError> {
        if root.identity() != &self.store_root_identity
            || ContentHash::sha256(&self.canonical_bytes) != self.snapshot_hash
            || CasHash::parse(self.snapshot_hash.to_string())? != self.cas_hash
        {
            return Err(IndexError::ProjectionContractViolation);
        }
        CasStore::open(root)?
            .verify_exact_bytes_streaming(&self.cas_hash, &self.canonical_bytes)
            .map_err(IndexError::from)?;
        if let Some(hash) = &self.observed.largest_cas_hash {
            CasStore::open(root)?
                .verify_exact_bytes_streaming(hash, &self.observed.largest_cas_bytes)
                .map_err(IndexError::from)?;
        }
        Ok(())
    }
}

fn project_complete_target(
    session: &crate::journal::ReplayedV5RunSession,
    root: &StoreRoot,
) -> Result<(PreIncrementalIndexSnapshotV6, TargetProjectionObservedV6), IndexError> {
    let log = session.log();
    let event_count =
        u64::try_from(log.envelopes().len()).map_err(|_| IndexError::IntegerOutOfRange)?;
    // Keep the opaque Core-owned backing alive for this projection.  It
    // checks the exact source-bound offset/count/tail tuple against the live
    // EventLog allocation; Store reads only the narrow accepted facts below.
    let structural_prefix = log
        .replay_pre_incremental_structural_prefix_for_store(V5StructuralPrefixCoordinates::new(
            log.run_id(),
            log.genesis_hash(),
            session.confirmed_offset(),
            event_count,
            log.tail_hash(),
        ))
        .map_err(|_| IndexError::ProjectionContractViolation)?;
    let projection = structural_prefix.store_facts();
    let program = projection.program_space();
    let universe = projection.universe();
    let plan = projection.plan();
    if universe.snapshot_id() != program.snapshot_id()
        || program.accepted_git_revision_closure().is_none()
    {
        return Err(IndexError::ProjectionContractViolation);
    }
    let reader = crate::CasReader::open_existing(root)?;
    let mut largest_cas_bytes = Vec::new();
    let mut largest_cas_hash = None;
    for registration in projection.source_registrations() {
        let hash = CasHash::parse(registration.cas_hash().to_string())?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(
                usize::try_from(registration.size()).map_err(|_| IndexError::IntegerOutOfRange)?,
            )
            .map_err(|_| IndexError::Incomplete {
                limit: root.limits().max_index_working_bytes,
                observed: registration.size(),
            })?;
        reader.read_into(&hash, Some(registration.size()), &mut bytes)?;
        if bytes.len() > largest_cas_bytes.len() {
            largest_cas_bytes = bytes;
            largest_cas_hash = Some(hash);
        }
    }
    let policy_revision_hash = ContentHash::sha256(
        &canonical_json(&TargetPolicyIdentityV6 {
            schema: "reviewgraphen.target_policy_identity.v6",
            profile_id: program.profile_id(),
            profile_version: program.profile_version(),
            policy_version: program.policy_version(),
            rule_set_hash: program.rule_set_hash(),
            extractor_set_hash: program.extractor_set_hash(),
        })
        .map_err(|_| IndexError::ProjectionContractViolation)?,
    );
    let plan_body_hash = ContentHash::sha256(
        &plan
            .canonical_bytes()
            .map_err(|_| IndexError::ProjectionContractViolation)?,
    );
    let authority_replay_basis_digest = ContentHash::sha256(
        &canonical_json(&TargetReplayBasisV6 {
            schema: "reviewgraphen.target_pre_incremental_replay_basis.v6",
            run_id: log.run_id(),
            genesis_hash: log.genesis_hash(),
            confirmed_offset: session.confirmed_offset(),
            tail_hash: log.tail_hash(),
            event_count,
            snapshot_id: program.snapshot_id(),
            universe_id: universe.id(),
            plan_id: plan.id(),
            plan_body_hash: &plan_body_hash,
            policy_revision_hash: &policy_revision_hash,
        })
        .map_err(|_| IndexError::ProjectionContractViolation)?,
    );
    let snapshot = PreIncrementalIndexSnapshotV6 {
        marker: PreIncrementalIndexMarkerV6 {
            index_schema_version: INDEX_SCHEMA_VERSION_V6,
            projection_contract_version: PROJECTION_CONTRACT_VERSION_V6.to_owned(),
            event_contract_version: "reviewgraphen.review_event.v5".to_owned(),
            projection_mode: "fresh_target_pre_incremental".to_owned(),
            run_id: log.run_id().clone(),
            genesis_hash: log.genesis_hash().clone(),
            confirmed_offset: session.confirmed_offset(),
            tail_hash: log.tail_hash().clone(),
            event_count,
            policy_revision_hash,
            authority_replay_basis_digest,
        },
        repository_id: program.repository_id().clone(),
        repository_identity_hash: repository_identity_hash_v6(program)?,
        snapshot_id: program.snapshot_id().clone(),
        universe_id: universe.id().clone(),
        plan_id: plan.id().clone(),
        plan_body_hash,
        program_space: program.clone(),
        obligation_ids: universe.obligation_ids().iter().cloned().collect(),
    };
    let decoded_owned_bytes = super::v5::recursive_ownership_charge(&snapshot)?;
    let max_event_line_bytes = log
        .maximum_canonical_event_line_bytes_for_store()
        .map_err(|_| IndexError::ProjectionContractViolation)?;
    Ok((
        snapshot,
        TargetProjectionObservedV6 {
            largest_cas_bytes,
            largest_cas_hash,
            max_event_line_bytes,
            decoded_owned_bytes,
        },
    ))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{JournalError, JournalGenesis, JournalIdentity, StoreLimits};
    use reviewgraphen_core::{
        ArtifactRegisteredV3, ArtifactSensitivity, ArtifactSourceV3, EventCommand, EventLog,
        EventLogV5, MvpRulePack, PlanBudget, ProgramSpace, ReviewAggregate,
        RunGenesisBootstrapRequestV4, SnapshotSourceRecordEntry, SnapshotSourcesRecorded, plan,
    };
    use serde_json::Value;

    const FIXTURE: &[u8] =
        include_bytes!("../../../examples/double-submit-payment/program-space.json");

    const BASE_COMMIT_OID: &str = "1111111111111111111111111111111111111111";
    const TARGET_COMMIT_OID: &str = "2222222222222222222222222222222222222222";
    const BASE_TREE_HASH: &str = "git:3333333333333333333333333333333333333333";
    const TARGET_TREE_HASH: &str = "git:4444444444444444444444444444444444444444";

    fn upgrade_fixture_to_incremental_v3(
        input: &mut Value,
        base_commit_oid: &str,
        base_tree_hash: &str,
    ) {
        input["schema"] = Value::String("reviewgraphen.program_space.input.v3".to_owned());
        input["source"]["kind"] = Value::String("git".to_owned());
        input["source"]["revision"] = Value::String(TARGET_COMMIT_OID.to_owned());
        input["source"]["content_hash"] = Value::String(TARGET_TREE_HASH.to_owned());
        input["snapshot"]["base_revision"] = Value::String(base_commit_oid.to_owned());
        input["snapshot"]["target_revision"] = Value::String(TARGET_COMMIT_OID.to_owned());
        input["snapshot"]["tree_hash"] = Value::String(TARGET_TREE_HASH.to_owned());
        input["snapshot"]["dirty"] = Value::Bool(false);
        input["snapshot"]["id"] = Value::String("snapshot:v6-target".to_owned());
        let test = input["artifacts"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|artifact| artifact["id"] == reviewgraphen_core::FIXTURE_TEST_ARTIFACT_ID)
            .unwrap();
        test["location"]["start_line"] = Value::Null;
        test["location"]["end_line"] = Value::Null;
        for limitation in input["extraction"]["limitations"].as_array_mut().unwrap() {
            for source_id in limitation["source_ids"].as_array_mut().unwrap() {
                if source_id == "snapshot:double-submit-v1" {
                    *source_id = Value::String("snapshot:v6-target".to_owned());
                }
            }
        }

        // The base fixture predates the range-anchor invariant.  Its target
        // projection must nevertheless carry the accepted containment fact
        // that assigns the reached payment function to its source file.
        let mut contains = input["relations"][0].clone();
        contains["id"] = Value::String("relation:file-contains-payment-charge".to_owned());
        contains["kind"] = Value::String("contains".to_owned());
        contains["source_id"] = Value::String("file:payment-repository".to_owned());
        contains["target_ids"] = serde_json::json!(["function:payment-charge"]);
        contains["directed"] = Value::Bool(true);
        input["relations"].as_array_mut().unwrap().push(contains);

        for relation in input["relations"].as_array_mut().unwrap() {
            relation["ordered_target_ids"] = relation["target_ids"].clone();
        }

        let mut anchors = serde_json::Map::new();
        for artifact in input["artifacts"].as_array_mut().unwrap() {
            let kind = artifact["kind"].as_str().unwrap().to_owned();
            if artifact["language"] == "rust"
                && matches!(kind.as_str(), "function" | "method" | "type")
            {
                let id = artifact["id"].as_str().unwrap().to_owned();
                artifact["provenance"]["extraction_method"] =
                    Value::String("reviewgraphen.ingest.rust_syn.v1".to_owned());
                anchors.insert(
                    id.clone(),
                    serde_json::json!({
                        "descriptor": "reviewgraphen.rust_symbol_anchor@1",
                        "language": "rust",
                        "symbol_kind": kind,
                        "signature_shape_hash": ContentHash::sha256(
                            format!("signature:{id}").as_bytes()
                        ),
                        "normalized_body_hash": ContentHash::sha256(
                            format!("body:{id}").as_bytes()
                        )
                    }),
                );
            }
        }
        input["incremental_facts"] = serde_json::json!({
            "git_revision_closure": {
                "base_commit_oid": base_commit_oid,
                "base_tree_hash": base_tree_hash,
                "target_commit_oid": TARGET_COMMIT_OID,
                "target_tree_hash": TARGET_TREE_HASH
            },
            "rust_anchor_extractor_id": "reviewgraphen.ingest.rust_syn.anchor.v1",
            "rust_anchor_syn_version": "2.0.119",
            "rust_symbol_anchors": anchors
        });
    }

    pub(crate) fn planned_target(root: &StoreRoot) -> EventLogV5 {
        planned_target_with_schema(root, true, BASE_COMMIT_OID, BASE_TREE_HASH, "run:v6-target")
    }

    pub(crate) fn planned_target_with_run(root: &StoreRoot, run_id: &str) -> EventLogV5 {
        planned_target_with_schema(root, true, BASE_COMMIT_OID, BASE_TREE_HASH, run_id)
    }

    pub(crate) fn planned_double_submit_target_with_run(
        root: &StoreRoot,
        run_id: &str,
    ) -> EventLogV5 {
        let mut input: Value = serde_json::from_slice(FIXTURE).unwrap();
        upgrade_fixture_to_incremental_v3(&mut input, BASE_COMMIT_OID, BASE_TREE_HASH);
        input["profile"]["id"] = Value::String("double-submit-payment".to_owned());
        input["profile"]["version"] = Value::String("1".to_owned());
        input["profile"]["policy_version"] = Value::String("default@1".to_owned());
        planned_target_from_input(root, input, run_id)
    }

    pub(crate) fn planned_target_with_base(
        root: &StoreRoot,
        base_commit_oid: &str,
        base_tree_hash: &str,
    ) -> EventLogV5 {
        planned_target_with_schema(root, true, base_commit_oid, base_tree_hash, "run:v6-target")
    }

    fn planned_target_with_schema(
        root: &StoreRoot,
        incremental_v3: bool,
        base_commit_oid: &str,
        base_tree_hash: &str,
        run_id: &str,
    ) -> EventLogV5 {
        let mut input: Value = serde_json::from_slice(FIXTURE).unwrap();
        if incremental_v3 {
            upgrade_fixture_to_incremental_v3(&mut input, base_commit_oid, base_tree_hash);
        }
        planned_target_from_input(root, input, run_id)
    }

    fn planned_target_from_input(root: &StoreRoot, mut input: Value, run_id: &str) -> EventLogV5 {
        let mut bytes_by_path = std::collections::BTreeMap::new();
        for artifact in input["artifacts"].as_array_mut().unwrap() {
            if artifact["kind"] == "file" {
                let path = artifact["location"]["path"].as_str().unwrap().to_owned();
                // Symbol anchors in the fixture reach lines 9–13.  Keep the
                // recorded source long enough for the deterministic context
                // projection to admit the corresponding excerpt.
                let bytes = format!("accepted target source: {path}\n")
                    .repeat(40)
                    .into_bytes();
                artifact["content_hash"] = Value::String(ContentHash::sha256(&bytes).to_string());
                bytes_by_path.insert(path, bytes);
            }
        }
        let program = ProgramSpace::from_json_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
        let (universe, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        let aggregate = ReviewAggregate::new(program, universe, obligations).unwrap();
        let run_id = StableId::parse(run_id).unwrap();
        let snapshot_id = aggregate.program().snapshot_id().clone();
        let mut local = EventLog::new_v3(run_id.clone(), aggregate).unwrap();
        let genesis_bytes = local
            .run_genesis_snapshot()
            .unwrap()
            .canonical_bytes()
            .unwrap();
        let mut registrations = Vec::new();
        let mut entries = Vec::new();
        let cas = CasStore::open(root).unwrap();
        let files = local
            .aggregate()
            .program()
            .artifacts()
            .iter()
            .filter(|artifact| artifact.kind == "file")
            .cloned()
            .collect::<Vec<_>>();
        for artifact in files {
            let path = artifact.location.as_ref().unwrap().path.clone();
            let bytes = &bytes_by_path[&path];
            let hash = ContentHash::sha256(bytes);
            let registration = ArtifactRegisteredV3::new(
                run_id.clone(),
                hash.clone(),
                "text/plain",
                u64::try_from(bytes.len()).unwrap(),
                ArtifactSensitivity::WorkspaceSource,
                ArtifactSourceV3::SnapshotIngest {
                    adapter_id: "test-git-adapter".to_owned(),
                    run_id: run_id.clone(),
                    snapshot_id: snapshot_id.clone(),
                },
            )
            .unwrap();
            let cas_hash = CasHash::parse(hash.to_string()).unwrap();
            cas.put(
                &cas_hash,
                Some(u64::try_from(bytes.len()).unwrap()),
                Cursor::new(bytes),
            )
            .unwrap();
            entries.push(
                SnapshotSourceRecordEntry::new(
                    artifact.id.clone(),
                    path,
                    hash.clone(),
                    registration.registration_id().clone(),
                    hash,
                    u64::try_from(bytes.iter().filter(|byte| **byte == b'\n').count())
                        .unwrap_or(u64::MAX)
                        .saturating_add(1),
                )
                .unwrap(),
            );
            local
                .append(EventCommand::artifact_registered_v3(registration.clone()))
                .unwrap();
            registrations.push(registration);
        }
        registrations.sort_by(|left, right| left.registration_id().cmp(right.registration_id()));
        entries.sort_by(|left, right| left.path().cmp(right.path()));
        let sources = SnapshotSourcesRecorded::new(snapshot_id, entries).unwrap();
        local
            .append(EventCommand::snapshot_sources_recorded(sources.clone()))
            .unwrap();
        let review_plan = plan(local.aggregate(), PlanBudget::new(16, 16).unwrap()).unwrap();
        let request = RunGenesisBootstrapRequestV4::new(
            run_id,
            genesis_bytes,
            local.aggregate().program().repository_identity(),
            local.aggregate().program().snapshot_id().clone(),
            local.aggregate().program().profile_id(),
            local.aggregate().program().profile_version(),
        )
        .unwrap();
        EventLogV5::from_planned_bootstrap_request(request, registrations, sources, review_plan)
            .unwrap()
    }

    #[test]
    fn v6_projection_refuses_program_space_v2() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let legacy = planned_target_with_schema(
            &root,
            false,
            BASE_COMMIT_OID,
            BASE_TREE_HASH,
            "run:v6-target",
        );
        let (journal, _) = EventJournal::publish_new_v5(&root, legacy).unwrap();
        let index = DerivedIndexV6::open(&root).unwrap();
        assert!(matches!(
            index.rebuild_pre_incremental_v6(&journal),
            Err(IndexError::ProjectionContractViolation)
        ));
    }

    #[test]
    fn v6_projection_requires_complete_plan_and_rechecks_cas() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let genesis_only = EventLogV5::from_bootstrap_request({
            let complete = planned_target(&root);
            let genesis = complete.canonical_genesis_bytes().to_vec();
            let state =
                reviewgraphen_core::RunGenesisSnapshot::from_canonical_v4_bytes_for_store(&genesis)
                    .unwrap();
            RunGenesisBootstrapRequestV4::new(
                StableId::parse("run:v6-incomplete").unwrap(),
                genesis,
                state.program_space().repository_identity(),
                state.program_space().snapshot_id().clone(),
                state.program_space().profile_id(),
                state.program_space().profile_version(),
            )
            .unwrap()
        })
        .unwrap();
        assert!(matches!(
            EventJournal::publish_new_v5(&root, genesis_only),
            Err(JournalError::Domain(_))
        ));

        let target = planned_target(&root);
        let expected_plan = target
            .replay_pre_incremental_state_for_store()
            .unwrap()
            .projection()
            .plan()
            .id()
            .clone();
        for registration in target
            .replay_pre_incremental_state_for_store()
            .unwrap()
            .projection()
            .registrations()
        {
            let hash = CasHash::parse(registration.cas_hash().to_string()).unwrap();
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(usize::try_from(registration.size()).unwrap())
                .unwrap();
            crate::CasReader::open_existing(&root)
                .unwrap()
                .read_into(&hash, Some(registration.size()), &mut bytes)
                .unwrap();
        }
        let (journal, _) = EventJournal::publish_new_v5(&root, target).unwrap();
        let foreign_workspace = tempfile::tempdir().unwrap();
        let foreign_root =
            StoreRoot::open(foreign_workspace.path(), StoreLimits::default()).unwrap();
        let foreign_index = DerivedIndexV6::open(&foreign_root).unwrap();
        assert!(matches!(
            foreign_index.rebuild_pre_incremental_v6(&journal),
            Err(IndexError::ProjectionContractViolation)
        ));
        let index = DerivedIndexV6::open(&root).unwrap();
        let receipt = index.rebuild_pre_incremental_v6(&journal).unwrap();
        let view = index.validated_snapshot_current_v6(&journal).unwrap();
        assert_eq!(view.snapshot().plan_id, expected_plan);
        assert_eq!(view.snapshot_hash(), &receipt.snapshot_hash);
        drop(view);

        let object = root
            .path()
            .join("artifacts/sha256")
            .join(receipt.cas_hash.prefix())
            .join(receipt.cas_hash.hex());
        std::fs::write(object, b"mutated index projection").unwrap();
        assert!(index.validated_snapshot_current_v6(&journal).is_err());
    }

    #[test]
    fn v6_replay_refuses_mutated_genesis_and_registered_source_cas() {
        for mutate_genesis in [true, false] {
            let workspace = tempfile::tempdir().unwrap();
            let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
            let target = planned_target(&root);
            let object_hash = if mutate_genesis {
                CasHash::parse(target.genesis_hash().to_string()).unwrap()
            } else {
                CasHash::parse(
                    target
                        .replay_pre_incremental_state_for_store()
                        .unwrap()
                        .projection()
                        .registrations()[0]
                        .cas_hash()
                        .to_string(),
                )
                .unwrap()
            };
            let (journal, _) = EventJournal::publish_new_v5(&root, target).unwrap();
            let object = root
                .path()
                .join("artifacts/sha256")
                .join(object_hash.prefix())
                .join(object_hash.hex());
            std::fs::write(object, b"mutated accepted target bytes").unwrap();
            let index = DerivedIndexV6::open(&root).unwrap();
            assert!(index.rebuild_pre_incremental_v6(&journal).is_err());
        }
    }

    #[test]
    fn v6_retains_real_largest_cas_bytes_and_revalidates_their_hash() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let target = planned_target(&root);
        let (journal, _) = EventJournal::publish_new_v5(&root, target).unwrap();
        let index = DerivedIndexV6::open(&root).unwrap();
        index.rebuild_pre_incremental_v6(&journal).unwrap();
        let view = index.validated_snapshot_current_v6(&journal).unwrap();
        let hash = view.observed.largest_cas_hash.clone().unwrap();
        let mut oracle = Vec::new();
        oracle
            .try_reserve_exact(view.observed.largest_cas_bytes.len())
            .unwrap();
        crate::CasReader::open_existing(&root)
            .unwrap()
            .read_into(
                &hash,
                Some(view.observed.largest_cas_bytes.len() as u64),
                &mut oracle,
            )
            .unwrap();
        assert_eq!(view.observed.largest_cas_bytes, oracle);

        let object = root
            .path()
            .join("artifacts/sha256")
            .join(hash.prefix())
            .join(hash.hex());
        std::fs::write(object, b"mutated retained target source").unwrap();
        assert!(view.revalidate_cas(&root).is_err());
    }

    #[test]
    fn v6_current_projection_is_deterministic_and_keeps_states_disjoint() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let target = planned_target(&root);
        let (journal, _) = EventJournal::publish_new_v5(&root, target).unwrap();
        let index = DerivedIndexV6::open(&root).unwrap();
        let first = index.rebuild_current_v5_v6(&journal).unwrap();
        let second = index.rebuild_current_v5_v6(&journal).unwrap();
        assert_eq!(first, second);

        let session = journal.replayed_v5_target_session().unwrap();
        let (snapshot, _) = project_current_target(&session, &root).unwrap();
        assert_eq!(snapshot.marker.projection_mode, "v5_current_structural");
        assert!(!snapshot.coverage_denominator_ids.is_empty());
        assert!(!snapshot.inherited_v5_rows.is_empty());
        assert!(
            snapshot
                .inherited_v5_rows
                .iter()
                .all(|row| row.witness().sequence > 0)
        );
        assert!(
            snapshot
                .m6_rows
                .iter()
                .all(|row| row.witness().sequence > 0)
        );
        assert!(snapshot.inherited_v5_rows.iter().any(|row| matches!(
            row,
            V5InheritedReportProjection::SnapshotSourcesRecorded { .. }
        )));
        assert!(
            snapshot
                .inherited_v5_rows
                .iter()
                .any(|row| matches!(row, V5InheritedReportProjection::ReviewPlanRecorded { .. }))
        );
    }

    #[test]
    fn schema_v6_executes_with_adr_m6_tables_and_foreign_keys() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("PRAGMA foreign_keys=ON").unwrap();
        connection.execute_batch(SCHEMA_V6).unwrap();
        connection
            .pragma_update(None, "user_version", 6_i64)
            .unwrap();

        let mut tables = connection
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        let table_names = tables
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<std::collections::BTreeSet<_>, _>>()
            .unwrap();
        for required in [
            "artifact_registrations_v5",
            "incremental_source_closures_v5",
            "program_mappings_v5",
            "change_morphisms_v5",
            "obligation_correspondence_entries_v5",
            "obligation_correspondences_v5",
            "historical_record_assessments_v5",
            "gluing_freshness_v5",
            "staleness_assessments_v5",
            "preservation_evidence_v5",
            "preservation_verifications_v5",
            "partial_rerun_actions_v5",
            "partial_rerun_plans_v5",
            "gluing_rerun_actions_v5",
            "gluing_rerun_plans_v5",
        ] {
            assert!(
                table_names.contains(required),
                "missing ADR V6 table {required}"
            );
        }
        let event_columns = connection
            .prepare("PRAGMA table_info(events)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<std::collections::BTreeSet<_>, _>>()
            .unwrap();
        assert!(event_columns.contains("actor"));
        assert!(event_columns.contains("logical_time"));
        let references = connection
            .prepare("PRAGMA foreign_key_list(program_mappings_v5)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(2))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(references.contains(&"incremental_source_closures_v5".to_owned()));
        assert!(references.contains(&"events".to_owned()));
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 6);
        assert_eq!(
            connection
                .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
    }

    #[test]
    fn v6_current_projection_preflights_exact_and_one_below_limits() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let target = planned_target(&root);
        let (journal, _) = EventJournal::publish_new_v5(&root, target).unwrap();
        let index = DerivedIndexV6::open(&root).unwrap();
        let measured = index.rebuild_current_v5_v6(&journal).unwrap().accounting;
        assert!(measured.rows > 0);

        let exact_limits = StoreLimits {
            max_index_rows: measured.rows,
            ..StoreLimits::default()
        };
        let exact_workspace = tempfile::tempdir().unwrap();
        let exact_root = StoreRoot::open(exact_workspace.path(), exact_limits).unwrap();
        let exact_index = DerivedIndexV6::open(&exact_root).unwrap();
        // Exact lower-bound admission is checked by the actual SQLite row
        // materialization rather than a parallel row formula.
        let exact_target = planned_target(&exact_root);
        let (exact_journal, _) = EventJournal::publish_new_v5(&exact_root, exact_target).unwrap();
        assert!(exact_index.rebuild_current_v5_v6(&exact_journal).is_ok());

        let row_below_workspace = tempfile::tempdir().unwrap();
        let row_below_root = StoreRoot::open(
            row_below_workspace.path(),
            StoreLimits {
                max_index_rows: measured.rows - 1,
                ..exact_limits
            },
        )
        .unwrap();
        assert!(matches!(
            DerivedIndexV6::open(&row_below_root).unwrap().rebuild_current_v5_v6(&EventJournal::publish_new_v5(&row_below_root, planned_target(&row_below_root)).unwrap().0),
            Err(IndexError::Incomplete { observed, .. }) if observed == measured.rows
        ));
    }

    #[test]
    fn v6_current_snapshot_accounting_is_measured_and_exact_at_every_limit() {
        fn rebuild(limits: StoreLimits) -> Result<IndexRebuildReceiptV6Current, IndexError> {
            let workspace = tempfile::tempdir().unwrap();
            let root = StoreRoot::open(workspace.path(), limits).unwrap();
            let target = planned_target(&root);
            let (journal, _) = EventJournal::publish_new_v5(&root, target).unwrap();
            DerivedIndexV6::open(&root)?.rebuild_current_v5_v6(&journal)
        }

        let baseline = rebuild(StoreLimits::default()).unwrap();
        let accounting = &baseline.accounting;
        assert_eq!(
            accounting.sql_bytes,
            accounting.text_bytes + accounting.integer_cells * 8 + accounting.rows
        );
        assert!(accounting.query_bytes > 0);
        assert!(accounting.owned_bytes > 0);
        assert_eq!(
            accounting.working_bytes,
            accounting.sql_bytes
                + accounting.query_bytes
                + accounting.owned_bytes
                + accounting.observed_object_bytes
                + accounting.observed_event_line_bytes
        );

        let exact = StoreLimits {
            max_index_rows: accounting.rows,
            max_index_serialized_bytes: accounting.query_bytes,
            max_index_query_bytes: accounting.query_bytes,
            max_index_working_bytes: accounting.working_bytes,
            ..StoreLimits::default()
        };
        let exact_receipt = rebuild(exact).unwrap();
        assert_eq!(exact_receipt.accounting, *accounting);
        assert_eq!(exact_receipt.snapshot_hash, baseline.snapshot_hash);

        // `query_bytes` is counted with a streaming writer before this
        // phase.  A one-byte-short serialized/query budget must therefore
        // refuse before the full canonical JSON output is allocated or
        // encoded, while the exact inclusive boundary remains admitted.
        let serialized_one_below = StoreLimits {
            max_index_serialized_bytes: accounting.query_bytes - 1,
            ..exact
        };
        reset_v6_canonical_snapshot_materialization_count_for_test();
        assert!(matches!(
            rebuild(serialized_one_below),
            Err(IndexError::Incomplete { limit, observed })
                if limit + 1 == observed && observed == accounting.query_bytes
        ));
        assert_eq!(v6_canonical_snapshot_materialization_count_for_test(), 0);

        for constrained in [
            StoreLimits {
                max_index_rows: accounting.rows - 1,
                ..exact
            },
            StoreLimits {
                max_index_query_bytes: accounting.query_bytes - 1,
                ..exact
            },
            StoreLimits {
                max_index_working_bytes: accounting.working_bytes - 1,
                ..exact
            },
        ] {
            assert!(matches!(
                rebuild(constrained),
                Err(IndexError::Incomplete { .. })
            ));
        }
    }

    #[test]
    fn v6_terminal_recovery_refuses_absent_and_malformed_proof_artifacts() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let target = planned_target(&root);
        let (journal, _) = EventJournal::publish_new_v5(&root, target).unwrap();
        let index = DerivedIndexV6::open(&root).unwrap();

        let missing =
            CasHash::parse(ContentHash::sha256(b"missing terminal proof").to_string()).unwrap();
        assert!(
            index
                .rebuild_terminal_from_proof_cas_v5_v6(&journal, &missing, 0)
                .is_err()
        );

        let malformed_bytes = b"{}";
        let malformed = CasHash::parse(ContentHash::sha256(malformed_bytes).to_string()).unwrap();
        CasStore::open(&root)
            .unwrap()
            .put(
                &malformed,
                Some(u64::try_from(malformed_bytes.len()).unwrap()),
                Cursor::new(malformed_bytes),
            )
            .unwrap();
        assert!(
            index
                .rebuild_terminal_from_proof_cas_v5_v6(
                    &journal,
                    &malformed,
                    u64::try_from(malformed_bytes.len()).unwrap(),
                )
                .is_err()
        );
    }

    /// The two identity constructors differ only in what they do about the
    /// authorship verdict, and only on the V5 wire. Extension points take the
    /// strict one, so a genesis the running pack does not reproduce cannot
    /// reach them -- the refusal is by construction, which matters here
    /// because the sweep of this crate found read-named functions that write
    /// (`recover_terminal_proof_v5` puts into CAS) and write-named ones that
    /// do not, so gating by enumerating names would have been fragile.
    #[test]
    fn read_only_identity_reports_a_v5_verdict_and_the_strict_one_governs_extension() {
        let genesis = include_bytes!("../tests/fixtures/terminal-v5-gluing-genesis.json");
        let run = || StableId::parse("run:m6-gluing-positive-s1").unwrap();

        let (_, verdict) =
            JournalIdentity::new_for_read(run(), JournalGenesis::V5(genesis.to_vec())).unwrap();
        let verdict = verdict.expect("a V5 genesis carries an authorship verdict");
        assert!(
            !verdict.is_reproduced(),
            "this corpus was written by an earlier rule pack, which is exactly \
             the case the read constructor exists for"
        );

        // The same bytes through the strict constructor. Every path that
        // extends a run takes this one, so a version-crossed record cannot
        // reach any of them without a caller having explicitly asked for the
        // read-only form.
        assert!(JournalIdentity::new(run(), JournalGenesis::V5(genesis.to_vec())).is_err());
    }

    #[test]
    fn v6_terminal_legacy_body_hash_fixture_is_rejected_before_target_only_recovery() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let proof_bytes = include_bytes!("../tests/fixtures/terminal-proof-v5-gluing.json");
        // This historical hostile corpus predates the strict terminal DTO and
        // persists a non-contract `body_hash` inside the marker. Publication
        // must reject it before a target-only recovery path is even reached.
        assert!(TerminalProofV5::from_canonical_bytes(proof_bytes).is_ok());
        let genesis = include_bytes!("../tests/fixtures/terminal-v5-gluing-genesis.json");
        // Read-only construction on purpose. This corpus is historical: its
        // genesis was synthesized by whatever rule pack wrote it, so a later
        // pack need not reproduce it, and refusing to *load* it would stop
        // this test reaching the rejection it exists to prove. Publication
        // below is still expected to fail -- on the marker's non-contract
        // `body_hash`, which is the assertion that matters.
        let (identity, _reproduction) = JournalIdentity::new_for_read(
            StableId::parse("run:m6-gluing-positive-s1").unwrap(),
            JournalGenesis::V5(genesis.to_vec()),
        )
        .unwrap();
        let genesis_hash = CasHash::parse(ContentHash::sha256(genesis).to_string()).unwrap();
        CasStore::open(&root)
            .unwrap()
            .put(
                &genesis_hash,
                Some(u64::try_from(genesis.len()).unwrap()),
                Cursor::new(genesis),
            )
            .unwrap();
        // Asserted on the reason, not merely on failure. Once the genesis is
        // loadable again (it is version-crossed, so `new_for_read` above
        // accepts it and reports `NotReproducible`), a bare `is_err()` would
        // pass just as happily if publication had refused for some unrelated
        // reason -- including the verdict itself. The whole point of this
        // fixture is the marker's non-contract `body_hash`, so that is what
        // the assertion names.
        let Err(error) = EventJournal::publish_fixture_v5(
            &root,
            identity,
            include_bytes!("../tests/fixtures/terminal-v5-gluing.jsonl"),
        ) else {
            panic!("the legacy marker must be refused");
        };
        assert!(
            format!("{error:?}").contains("unknown field `body_hash`"),
            "publication must reject the non-contract body_hash, not something \
             incidental: {error:?}"
        );
    }
}
