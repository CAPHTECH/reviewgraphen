//! Authority-bound event-v3 SQLite projection.
//!
//! This module is a child of `index` so it can reuse the descriptor-anchored
//! image transport without making those security-sensitive primitives public.

#![cfg_attr(test, allow(dead_code))]

use super::{
    IndexClaim, IndexContextEnvelope, IndexError, IndexEvent, IndexExecution, IndexFinding,
    IndexLimits, IndexObligation, IndexObligationLifecycle, IndexProgramObject,
    IndexProgramRelation, IndexReviewPlan, IndexShadow, IndexSnapshotSource, IndexUniverse,
};
use crate::{EventJournal, StoreRoot, StoreRootIdentity, journal::IndexV5ReplayedPrefix};
use reviewgraphen_core::{
    ArtifactSensitivity, ArtifactSourceV3, ArtifactSourceV4, AuthorityReplayBasisV4,
    AuthorityTrustRootsV4, BorrowedArtifactRegistrationProjectionV3,
    BorrowedArtifactSourceProjectionV3,
    BorrowedProjectionPayloadRefV4 as BorrowedProjectionPayloadV4,
    BorrowedProjectionPayloadV4 as CoreBorrowedProjectionPayloadV4, BorrowedV4EventMetadata,
    ContentHash, DecodedPayload, GluingBundleV4, RunGenesisSnapshot, Severity, StableId,
    canonical_json,
};
use rusqlite::{Row, types::ValueRef};
use rustix::{
    fd::OwnedFd,
    fs::{self, AtFlags, FileType, Mode, OFlags},
    rand::{GetRandomFlags, getrandom},
};
#[cfg(test)]
use serde::Deserialize;
use serde::Serialize;
use serde::ser::{
    self, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
    SerializeTupleStruct, SerializeTupleVariant, Serializer,
};
use serde_json::{Map, Value};
#[cfg(test)]
use std::cell::Cell;
#[cfg(test)]
use std::collections::BTreeMap;
use std::{cell::RefCell, collections::BTreeSet, fs::File, io::Write};

pub const INDEX_SCHEMA_VERSION_V5: u32 = 5;
pub const PROJECTION_CONTRACT_VERSION_V5: &str = "reviewgraphen.index_projection.v5";

// M5 section/restriction arrays are positional context arrays, not ID sets.
// Keep these readback queries alongside the validator so a future query edit
// cannot silently turn the payment/UI wire order into record-ID order.
const V5_SECTIONS_CONTEXT_ORDER_SQL: &str = "SELECT * FROM sections_v4 \
    ORDER BY event_sequence,CASE context_id \
        WHEN 'context:payment' THEN 0 \
        WHEN 'context:ui-event' THEN 1 \
        ELSE 2 END";
const V5_RESTRICTIONS_CONTEXT_ORDER_SQL: &str = "SELECT restriction.* \
    FROM restrictions_v4 AS restriction \
    JOIN sections_v4 AS section ON section.section_id=restriction.section_id \
    ORDER BY restriction.event_sequence,CASE section.context_id \
        WHEN 'context:payment' THEN 0 \
        WHEN 'context:ui-event' THEN 1 \
        ELSE 2 END";
const EVENT_CONTRACT_VERSION_V4: &str = "reviewgraphen.review_event.v4";
const PROJECTION_MODE_V4: &str = "v4_gluing";
// Separate from ADR 0021 Working4. This is the Store's process-local hard
// ceiling for the non-normative typed-snapshot selector and deliberately
// never scales up from caller input.
const V5_SELECTION_OPERATIONAL_LIMIT: u64 = 256 * 1024 * 1024;
const V5_JSON_STAGING_OPERATIONAL_LIMIT: u64 = 64 * 1024 * 1024;

#[cfg(test)]
thread_local! {
    static V5_PROJECTION_DECODE_COUNT: Cell<u64> = const { Cell::new(0) };
    static V5_PROJECTION_OBLIGATION_PROBE_COUNT: Cell<u64> = const { Cell::new(0) };
    static V5_SELECTION_OPERATIONAL_LIMIT_OVERRIDE: Cell<Option<u64>> = const { Cell::new(None) };
    static V5_JSON_STAGING_LIMIT_OVERRIDE: Cell<Option<u64>> = const { Cell::new(None) };
    static V5_JSON_DECODE_ALLOCATION_COUNT: Cell<u64> = const { Cell::new(0) };
    static V5_MATERIALIZATION_START_COUNT: Cell<u64> = const { Cell::new(0) };
    static V5_FULL_SNAPSHOT_CONSTRUCTION_COUNT: Cell<u64> = const { Cell::new(0) };
    static V5_SQLITE_BUILD_COUNT: Cell<u64> = const { Cell::new(0) };
    static V5_IMAGE_SERIALIZATION_COUNT: Cell<u64> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_v5_projection_decode_count_for_test() {
    V5_PROJECTION_DECODE_COUNT.with(|count| count.set(0));
    V5_PROJECTION_OBLIGATION_PROBE_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn v5_projection_decode_count_for_test() -> u64 {
    V5_PROJECTION_DECODE_COUNT.with(Cell::get)
}

#[cfg(test)]
pub(crate) fn v5_projection_obligation_probe_count_for_test() -> u64 {
    V5_PROJECTION_OBLIGATION_PROBE_COUNT.with(Cell::get)
}

#[cfg(test)]
fn record_projection_obligation_probe() {
    V5_PROJECTION_OBLIGATION_PROBE_COUNT.with(|count| count.set(count.get().saturating_add(1)));
}

#[cfg(not(test))]
fn record_projection_obligation_probe() {}

fn v5_selection_operational_limit() -> u64 {
    #[cfg(test)]
    if let Some(limit) = V5_SELECTION_OPERATIONAL_LIMIT_OVERRIDE.with(Cell::get) {
        return limit;
    }
    V5_SELECTION_OPERATIONAL_LIMIT
}

#[cfg(test)]
pub(crate) fn set_v5_selection_operational_limit_for_test(limit: Option<u64>) {
    V5_SELECTION_OPERATIONAL_LIMIT_OVERRIDE.with(|value| value.set(limit));
}

fn v5_json_staging_limit() -> u64 {
    #[cfg(test)]
    if let Some(limit) = V5_JSON_STAGING_LIMIT_OVERRIDE.with(Cell::get) {
        return limit;
    }
    V5_JSON_STAGING_OPERATIONAL_LIMIT
}

fn admit_v5_json_staging(observed: u64) -> Result<(), IndexError> {
    let limit = v5_json_staging_limit();
    if observed > limit {
        return Err(IndexError::Incomplete { limit, observed });
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn set_v5_json_staging_limit_for_test(limit: Option<u64>) {
    V5_JSON_STAGING_LIMIT_OVERRIDE.with(|value| value.set(limit));
}

#[cfg(test)]
pub(crate) fn reset_v5_json_decode_allocation_count_for_test() {
    V5_JSON_DECODE_ALLOCATION_COUNT.with(|value| value.set(0));
}

#[cfg(test)]
pub(crate) fn v5_json_decode_allocation_count_for_test() -> u64 {
    V5_JSON_DECODE_ALLOCATION_COUNT.with(Cell::get)
}

#[cfg(test)]
pub(crate) fn reset_v5_full_snapshot_construction_count_for_test() {
    V5_PROJECTION_DECODE_COUNT.with(|value| value.set(0));
    V5_MATERIALIZATION_START_COUNT.with(|value| value.set(0));
    V5_FULL_SNAPSHOT_CONSTRUCTION_COUNT.with(|value| value.set(0));
    V5_SQLITE_BUILD_COUNT.with(|value| value.set(0));
    V5_IMAGE_SERIALIZATION_COUNT.with(|value| value.set(0));
}

#[cfg(test)]
pub(crate) fn v5_post_phase0_counts_for_test() -> (u64, u64, u64, u64, u64) {
    (
        V5_PROJECTION_DECODE_COUNT.with(Cell::get),
        V5_MATERIALIZATION_START_COUNT.with(Cell::get),
        V5_FULL_SNAPSHOT_CONSTRUCTION_COUNT.with(Cell::get),
        V5_SQLITE_BUILD_COUNT.with(Cell::get),
        V5_IMAGE_SERIALIZATION_COUNT.with(Cell::get),
    )
}

#[cfg(test)]
fn record_v5_materialization_start() {
    V5_MATERIALIZATION_START_COUNT.with(|value| value.set(value.get().saturating_add(1)));
}

#[cfg(not(test))]
fn record_v5_materialization_start() {}

#[cfg(test)]
fn record_v5_full_snapshot_construction() {
    V5_FULL_SNAPSHOT_CONSTRUCTION_COUNT.with(|value| value.set(value.get().saturating_add(1)));
}

#[cfg(not(test))]
fn record_v5_full_snapshot_construction() {}

#[cfg(test)]
fn record_v5_sqlite_build() {
    V5_SQLITE_BUILD_COUNT.with(|value| value.set(value.get().saturating_add(1)));
}

#[cfg(not(test))]
fn record_v5_sqlite_build() {}

#[cfg(test)]
fn record_v5_image_serialization() {
    V5_IMAGE_SERIALIZATION_COUNT.with(|value| value.set(value.get().saturating_add(1)));
}

#[cfg(not(test))]
fn record_v5_image_serialization() {}

#[cfg(test)]
fn record_v5_json_decode_allocation() {
    V5_JSON_DECODE_ALLOCATION_COUNT.with(|value| value.set(value.get().saturating_add(1)));
}

#[cfg(not(test))]
fn record_v5_json_decode_allocation() {}

/// Complete schema-v5 literal.  In particular, this is deliberately not a
/// schema-v3 migration or a runtime concatenation of older DDL.
pub(crate) const SCHEMA_V5: &str = r#"
CREATE TABLE index_meta (
 singleton INTEGER PRIMARY KEY CHECK (singleton=1),
 index_schema_version INTEGER NOT NULL CHECK (index_schema_version=5),
 projection_contract_version TEXT NOT NULL CHECK (projection_contract_version='reviewgraphen.index_projection.v5'),
 event_contract_version TEXT NOT NULL CHECK (event_contract_version='reviewgraphen.review_event.v4'),
 projection_mode TEXT NOT NULL CHECK (projection_mode='v4_gluing'),
 run_id TEXT NOT NULL, genesis_hash TEXT NOT NULL,
 confirmed_offset INTEGER NOT NULL CHECK (confirmed_offset>=0), tail_hash TEXT NOT NULL,
 event_count INTEGER NOT NULL CHECK (event_count>=0),
 policy_revision_hash TEXT NOT NULL,
 authority_replay_basis_digest TEXT NOT NULL
) STRICT;
CREATE TABLE events (
 sequence INTEGER PRIMARY KEY CHECK (sequence>0), event_id TEXT NOT NULL UNIQUE,
 schema TEXT NOT NULL CHECK (schema='reviewgraphen.review_event.v4'),
 event_hash TEXT NOT NULL, payload_hash TEXT NOT NULL,
 payload_kind TEXT NOT NULL CHECK (payload_kind IN (
  'obligation_transition','run_genesis_manifest','artifact_registered',
  'snapshot_sources_recorded','review_plan_recorded','context_envelope_projected',
  'review_execution_recorded','evidence_recorded_v3','evidence_bound_v3',
  'verification_recorded_v3','decision_recorded_v3','finding_recorded_v3',
  'artifact_registered_v4','gluing_bundle_recorded_v4'
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
"#;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IndexMarkerV5 {
    pub index_schema_version: u64,
    pub sqlite_user_version: u64,
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

/// Borrowed phase-0 representation of [`IndexMarkerV5`].  It deliberately
/// preserves the public field order and wire values, while avoiding the four
/// `String`/ID/hash clones required by the materialized marker.
#[derive(Serialize)]
struct BorrowedIndexMarkerV5<'a> {
    index_schema_version: u64,
    sqlite_user_version: u64,
    projection_contract_version: &'static str,
    event_contract_version: &'static str,
    projection_mode: &'static str,
    run_id: &'a StableId,
    genesis_hash: &'a ContentHash,
    confirmed_offset: u64,
    tail_hash: &'a ContentHash,
    event_count: u64,
    policy_revision_hash: &'a ContentHash,
    authority_replay_basis_digest: &'a ContentHash,
}

fn borrowed_index_marker_v5<'a>(
    basis: &'a AuthorityReplayBasisV4,
    confirmed_offset: u64,
    event_count: u64,
) -> BorrowedIndexMarkerV5<'a> {
    BorrowedIndexMarkerV5 {
        index_schema_version: 5,
        sqlite_user_version: 5,
        projection_contract_version: PROJECTION_CONTRACT_VERSION_V5,
        event_contract_version: EVENT_CONTRACT_VERSION_V4,
        projection_mode: PROJECTION_MODE_V4,
        run_id: basis.run_id(),
        genesis_hash: basis.genesis_hash(),
        confirmed_offset,
        tail_hash: basis.confirmed_tail_hash(),
        event_count,
        policy_revision_hash: basis.policy_revision_hash(),
        authority_replay_basis_digest: basis.basis_digest(),
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IndexArtifactRegistrationV5 {
    pub event_sequence: u64,
    pub event_id: StableId,
    pub registration_id: StableId,
    pub run_id: StableId,
    pub cas_hash: ContentHash,
    pub media_type: String,
    pub size: u64,
    pub sensitivity: String,
    pub source_kind: String,
    pub source_canonical_json: String,
    pub source: Value,
    pub body_hash: ContentHash,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRegistrationV4IndexItem {
    pub event_sequence: u64,
    pub event_id: StableId,
    pub event_actor: String,
    pub registration_id: StableId,
    pub schema: String,
    pub run_id: StableId,
    pub cas_hash: ContentHash,
    pub media_type: String,
    pub size: u64,
    pub sensitivity: String,
    pub source_kind: String,
    pub source: Value,
    pub descriptor_id: StableId,
    pub body_hash: ContentHash,
}

macro_rules! m5_item {
    ($name:ident, $field:ident : $ty:ty $(, $owner:ident)?) => {
        #[derive(Clone, Debug, Eq, PartialEq, Serialize)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            pub event_sequence: u64,
            pub event_id: StableId,
            $(pub $owner: StableId,)?
            pub $field: $ty,
            pub body_hash: ContentHash,
        }
    };
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GluingInputDescriptorV4IndexItem {
    pub event_sequence: u64,
    pub event_id: StableId,
    pub descriptor: Value,
    pub registration_id: StableId,
    pub descriptor_hash: ContentHash,
    pub descriptor_size: u64,
    pub body_hash: ContentHash,
}
m5_item!(ContextCoverV4IndexItem, cover: Value);
m5_item!(SectionV4IndexItem, section: Value);
m5_item!(RestrictionV4IndexItem, restriction: Value, attempt_id);
m5_item!(GluingAttemptV4IndexItem, attempt: Value);
m5_item!(GlobalCandidateV4IndexItem, candidate: Value, attempt_id);
m5_item!(GluingObstructionV4IndexItem, obstruction: Value);

macro_rules! m4_row {
    ($name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Clone, Debug, Eq, PartialEq, Serialize)]
        pub struct $name { $(pub $field: $ty),* }
    };
}

m4_row!(IndexEvidenceV3AtV5 {
    event_sequence: u64,
    event_id: StableId,
    evidence_id: StableId,
    schema: String,
    kind: String,
    snapshot_id: StableId,
    subject_ids_canonical_json: String,
    descriptor_id: String,
    procedure_version: String,
    input_registration_id: StableId,
    output_registration_id: StableId,
    observation: String,
    body_hash: ContentHash,
});
m4_row!(IndexEvidenceBindingV3AtV5 {
    event_sequence: u64,
    event_id: StableId,
    binding_id: StableId,
    schema: String,
    claim_id: StableId,
    evidence_id: StableId,
    relation: String,
    property_id: String,
    body_hash: ContentHash,
});
m4_row!(IndexVerificationV3AtV5 {
    event_sequence: u64,
    event_id: StableId,
    verification_id: StableId,
    schema: String,
    claim_id: StableId,
    descriptor_id: String,
    procedure_version: String,
    input_registration_id: StableId,
    output_registration_id: StableId,
    evidence_ids_canonical_json: String,
    outcome: String,
    limitations_canonical_json: String,
    body_hash: ContentHash,
});
m4_row!(IndexDecisionV3AtV5 {
    event_sequence: u64, event_id: StableId, decision_id: StableId, schema: String,
    policy_revision_hash: ContentHash, run_id: StableId, universe_id: StableId,
    claim_id: StableId, property_id: String, outcome: String, actor: String,
    authority_id: String, snapshot_id: StableId, source_ids_canonical_json: String,
    rationale: String, issued_at: String, expires_at: Option<String>, body_hash: ContentHash,
});
m4_row!(IndexFindingV3AtV5 {
    event_sequence: u64, event_id: StableId, finding_id: StableId, schema: String,
    projection_descriptor_id: String, claim_id: StableId, status: String,
    evidence_ids_canonical_json: String, verification_ids_canonical_json: String,
    decision_id: Option<StableId>, supersedes_finding_id: Option<StableId>, body_hash: ContentHash,
});
m4_row!(IndexClaimAssessmentV3AtV5 {
    claim_id: StableId, disposition: String, review_status: String,
    binding_ids_canonical_json: String, evidence_ids_canonical_json: String,
    verification_ids_canonical_json: String, decision_ids_canonical_json: String,
    finding_ids_canonical_json: String, active_decision_id: Option<StableId>,
    current_finding_id: Option<StableId>, decision_conflict: bool,
    confirmed_event_sequence: u64,
});

/// Complete read-only schema-v5 projection.  The two legacy arrays remain
/// explicit even though an event-v3 authority replay normally leaves them
/// empty.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IndexSnapshotV5 {
    pub marker: IndexMarkerV5,
    pub events: Vec<IndexEvent>,
    pub shadows: Vec<IndexShadow>,
    pub projected_findings: Vec<IndexFinding>,
    pub program_objects: Vec<IndexProgramObject>,
    pub program_relations: Vec<IndexProgramRelation>,
    pub universe: Option<IndexUniverse>,
    pub obligations: Vec<IndexObligation>,
    pub obligation_lifecycle: Vec<IndexObligationLifecycle>,
    pub executions: Vec<IndexExecution>,
    pub claims: Vec<IndexClaim>,
    pub artifact_registrations: Vec<IndexArtifactRegistrationV5>,
    pub snapshot_sources: Vec<IndexSnapshotSource>,
    pub context_envelopes: Vec<IndexContextEnvelope>,
    pub review_plans: Vec<IndexReviewPlan>,
    pub evidence: Vec<IndexEvidenceV3AtV5>,
    pub evidence_bindings: Vec<IndexEvidenceBindingV3AtV5>,
    pub verifications: Vec<IndexVerificationV3AtV5>,
    pub decisions: Vec<IndexDecisionV3AtV5>,
    pub findings: Vec<IndexFindingV3AtV5>,
    pub claim_assessments: Vec<IndexClaimAssessmentV3AtV5>,
    pub artifact_registrations_v4: Vec<ArtifactRegistrationV4IndexItem>,
    pub gluing_input_descriptors: Vec<GluingInputDescriptorV4IndexItem>,
    pub context_covers: Vec<ContextCoverV4IndexItem>,
    pub sections: Vec<SectionV4IndexItem>,
    pub restrictions: Vec<RestrictionV4IndexItem>,
    pub gluing_attempts: Vec<GluingAttemptV4IndexItem>,
    pub global_candidates: Vec<GlobalCandidateV4IndexItem>,
    pub gluing_obstructions: Vec<GluingObstructionV4IndexItem>,
    pub policy_revision_hash: ContentHash,
    pub authority_replay_basis_digest: ContentHash,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexAccountingV5 {
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexRebuildReceiptV5 {
    pub run_id: StableId,
    pub genesis_hash: ContentHash,
    pub confirmed_offset: u64,
    pub tail_hash: ContentHash,
    pub event_count: u64,
    pub policy_revision_hash: ContentHash,
    pub authority_replay_basis_digest: ContentHash,
    pub serialized_bytes: u64,
    pub image_hash: ContentHash,
    pub accounting: IndexAccountingV5,
}

/// Caller-owned selection key for the read-only report-v3 projection seam.
/// The selected IDs are borrowed; Store never turns them into report-owned
/// wire data.
#[derive(Clone, Copy)]
pub struct V5SelectionRequest<'a> {
    pub plan_id: &'a StableId,
    pub selected_obligation_ids: &'a BTreeSet<StableId>,
    pub expected_confirmed_offset: u64,
    pub expected_event_count: u64,
    pub expected_tail_hash: &'a ContentHash,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum V5CoverageAxis {
    Denominator,
    Visited,
    Completed,
    EvidenceSupported,
    Verified,
    Accepted,
}

/// One borrowed row or semantic coverage ID in deterministic report order.
pub enum V5SelectionItem<'a> {
    ArtifactRegistration(&'a IndexArtifactRegistrationV5),
    Execution(&'a IndexExecution),
    Claim(&'a IndexClaim),
    Evidence(&'a IndexEvidenceV3AtV5),
    EvidenceBinding(&'a IndexEvidenceBindingV3AtV5),
    Verification(&'a IndexVerificationV3AtV5),
    Decision(&'a IndexDecisionV3AtV5),
    Finding(&'a IndexFindingV3AtV5),
    ClaimAssessment(&'a IndexClaimAssessmentV3AtV5),
    Obstruction(&'a IndexExecution),
    CoverageId {
        axis: V5CoverageAxis,
        id: &'a StableId,
    },
}

pub trait V5SelectionVisitor {
    type Error;

    fn visit(&mut self, item: V5SelectionItem<'_>) -> Result<(), Self::Error>;
}

#[derive(Debug)]
pub enum V5SelectionVisitError<E> {
    Index(IndexError),
    Visitor(E),
}

impl<E> From<IndexError> for V5SelectionVisitError<E> {
    fn from(value: IndexError) -> Self {
        Self::Index(value)
    }
}

impl<E> From<rusqlite::Error> for V5SelectionVisitError<E> {
    fn from(value: rusqlite::Error) -> Self {
        Self::Index(IndexError::from(value))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct V5SelectionCounts {
    pub artifact_registrations: u64,
    pub executions: u64,
    pub claims: u64,
    pub evidence: u64,
    pub evidence_bindings: u64,
    pub verifications: u64,
    pub decisions: u64,
    pub findings: u64,
    pub claim_assessments: u64,
    pub obstructions: u64,
    pub denominator_ids: u64,
    pub visited_ids: u64,
    pub completed_ids: u64,
    pub evidence_supported_ids: u64,
    pub verified_ids: u64,
    pub accepted_ids: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct V5SelectionSummary {
    pub counts: V5SelectionCounts,
    pub max_cas_bytes: u64,
    pub confirmed_offset: u64,
    pub event_count: u64,
    pub marker_fingerprint: [u8; 32],
}

/// Schema-v5 handle.  The descriptor-safe directory admission is shared with
/// the frozen v3 implementation, while all schema and projection semantics
/// remain version-specific.
pub struct DerivedIndexV5<'a> {
    inner: super::DerivedIndex<'a>,
}

/// Opaque authority- and image-bound owner of one validated schema-v5
/// snapshot. It is intentionally neither `Clone` nor serializable: report
/// passes borrow the same typed image through this capability.
pub struct ValidatedIndexSnapshotV5<'index, 'root, 'roots> {
    index: &'index DerivedIndexV5<'root>,
    roots: &'roots AuthorityTrustRootsV4,
    snapshot: IndexSnapshotV5,
    store_root_identity: StoreRootIdentity,
    image_hash: ContentHash,
    marker_fingerprint: [u8; 32],
}

impl ValidatedIndexSnapshotV5<'_, '_, '_> {
    #[must_use]
    pub const fn snapshot(&self) -> &IndexSnapshotV5 {
        &self.snapshot
    }

    fn into_snapshot(self) -> IndexSnapshotV5 {
        self.snapshot
    }
}

impl<'a> DerivedIndexV5<'a> {
    pub fn open(root: &'a StoreRoot) -> Result<Self, IndexError> {
        Ok(Self {
            inner: super::DerivedIndex::open(root)?,
        })
    }

    #[must_use]
    pub const fn limits(&self) -> IndexLimits {
        self.inner.limits()
    }

    #[cfg(test)]
    pub(crate) fn inject_publish_fault(&self, fault: super::PublishFault) {
        self.inner.inject_publish_fault(fault);
    }

    pub fn rebuild_v5(
        &self,
        journal: &EventJournal<'_>,
        roots: &AuthorityTrustRootsV4,
    ) -> Result<IndexRebuildReceiptV5, IndexError> {
        let lock = self.inner.lock_exclusive()?;
        self.inner.audit_candidates(true)?;
        let (session, basis) = journal.replayed_v4_session(roots)?;
        if !journal.matches_store_root(self.inner.root) {
            return Err(IndexError::ProjectionContractViolation);
        }
        let projected = project_verified_source(&session, &basis, self.inner.limits)?;
        let snapshot = projected.snapshot;
        let accounting = projected.accounting;
        let sqlite_limits = v5_sqlite_limits(self.inner.limits)?;
        let connection = build_connection(&snapshot, sqlite_limits, accounting.owned_bytes)?;
        record_v5_image_serialization();
        let image = super::serialize_connection_with_retained(
            &connection,
            sqlite_limits,
            accounting.owned_bytes,
        )?;
        drop(connection);
        let serialized_bytes =
            u64::try_from(image.len()).map_err(|_| IndexError::IntegerOutOfRange)?;
        let image_hash =
            self.publish_v5_image_locked(image, &snapshot, &accounting, sqlite_limits)?;
        lock.verify_unchanged()?;
        Ok(IndexRebuildReceiptV5 {
            run_id: snapshot.marker.run_id.clone(),
            genesis_hash: snapshot.marker.genesis_hash.clone(),
            confirmed_offset: snapshot.marker.confirmed_offset,
            tail_hash: snapshot.marker.tail_hash.clone(),
            event_count: snapshot.marker.event_count,
            policy_revision_hash: snapshot.policy_revision_hash.clone(),
            authority_replay_basis_digest: snapshot.authority_replay_basis_digest.clone(),
            serialized_bytes,
            image_hash,
            accounting,
        })
    }

    pub fn validated_snapshot_current_v5<'index, 'roots>(
        &'index self,
        journal: &EventJournal<'_>,
        roots: &'roots AuthorityTrustRootsV4,
    ) -> Result<ValidatedIndexSnapshotV5<'index, 'a, 'roots>, IndexError> {
        let lock = self.inner.lock_shared()?;
        self.inner.audit_candidates(false)?;
        let (session, basis) = journal.replayed_v4_session(roots)?;
        if !journal.matches_store_root(self.inner.root) {
            return Err(IndexError::ProjectionContractViolation);
        }
        let projected = project_verified_source(&session, &basis, self.inner.limits)?;
        let expected = projected.snapshot;
        let retained = projected.accounting.owned_bytes;
        let sqlite_limits = v5_sqlite_limits(self.inner.limits)?;
        let image = self
            .inner
            .read_active_image_locked_with_limits_and_retained(sqlite_limits, retained)?;
        let image_hash = ContentHash::sha256(&image);
        let connection = super::deserialize_read_only_for_schema(
            image,
            sqlite_limits,
            retained,
            INDEX_SCHEMA_VERSION_V5,
        )
        .map_err(super::normalize_external_image_error)?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if (1..=4).contains(&version) {
            return Err(IndexError::RebuildRequired {
                found: u32::try_from(version).map_err(|_| IndexError::CorruptIndex)?,
                required: 5,
            });
        }
        if version != 5 {
            return Err(IndexError::CorruptIndex);
        }
        let max_json_staging = preflight_v5_structure(&connection, self.inner.limits)
            .map_err(super::normalize_external_image_error)?;
        admit_v5_json_staging(max_json_staging)?;
        validate_v5_structure_after_preflight(&connection, self.inner.limits)
            .map_err(super::normalize_external_image_error)?;
        let integrity: String =
            connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        let fk_violation = connection
            .prepare("PRAGMA foreign_key_check")?
            .query([])?
            .next()?
            .is_some();
        if integrity != "ok" || fk_violation {
            return Err(IndexError::CorruptIndex);
        }
        let committed_marker = marker_v5_from_connection(&connection)?;
        if committed_marker != expected.marker {
            if committed_marker.run_id != expected.marker.run_id {
                return Err(IndexError::CorruptIndex);
            }
            let committed_offset = expected.marker.confirmed_offset;
            let committed_tail = expected.marker.tail_hash.clone();
            // A stale-image audit needs only the roots-bound historical
            // prefix. Release the current full typed view before replaying
            // and projecting that second view.
            drop(expected);
            let prefix = session
                .index_v5_replay_prefix(
                    committed_marker.event_count,
                    committed_marker.confirmed_offset,
                )
                .map_err(|_| IndexError::CorruptIndex)?;
            let indexed = project_verified_source(&prefix, prefix.basis(), self.inner.limits)?;
            if indexed.snapshot.marker != committed_marker {
                return Err(IndexError::CorruptIndex);
            }
            validate_v5_rows(&connection, &indexed.snapshot, self.inner.limits)
                .map_err(super::normalize_external_image_error)?;
            return Err(IndexError::CommittedIndexStale {
                indexed_offset: committed_marker.confirmed_offset,
                indexed_tail: committed_marker.tail_hash,
                committed_offset,
                committed_tail,
            });
        }
        validate_v5_rows(&connection, &expected, self.inner.limits)
            .map_err(super::normalize_external_image_error)?;
        let rebuilt = build_connection(&expected, sqlite_limits, retained)?;
        let rebuilt_bytes =
            super::serialize_connection_with_retained(&rebuilt, sqlite_limits, retained)?;
        if ContentHash::sha256(&rebuilt_bytes) != image_hash {
            return Err(IndexError::CorruptIndex);
        }
        drop(connection);
        lock.verify_unchanged()?;
        let marker_fingerprint = v5_marker_fingerprint(&expected.marker)?;
        Ok(ValidatedIndexSnapshotV5 {
            index: self,
            roots,
            snapshot: expected,
            store_root_identity: self.inner.root.identity().clone(),
            image_hash,
            marker_fingerprint,
        })
    }

    pub fn snapshot_current_v5(
        &self,
        journal: &EventJournal<'_>,
        roots: &AuthorityTrustRootsV4,
    ) -> Result<IndexSnapshotV5, IndexError> {
        Ok(self
            .validated_snapshot_current_v5(journal, roots)?
            .into_snapshot())
    }

    /// Visits the exact report-v3 selection from one roots-bound current-v5
    /// view. Callers may run this twice (measure, then materialize) and compare
    /// the fixed summary to detect any source drift without Store returning an
    /// ID vector or report wire representation.
    pub fn visit_current_v5_selection<V: V5SelectionVisitor>(
        &self,
        journal: &EventJournal<'_>,
        roots: &AuthorityTrustRootsV4,
        request: V5SelectionRequest<'_>,
        visitor: &mut V,
    ) -> Result<V5SelectionSummary, V5SelectionVisitError<V::Error>> {
        self.validated_snapshot_current_v5(journal, roots)?
            .visit_selection(journal, request, visitor)
    }
}

impl ValidatedIndexSnapshotV5<'_, '_, '_> {
    /// Visits a selection against this exact retained snapshot. The active
    /// image and roots-bound journal marker are revalidated under their locks,
    /// but no second `IndexSnapshotV5` is projected or allocated.
    pub fn visit_selection<V: V5SelectionVisitor>(
        &self,
        journal: &EventJournal<'_>,
        request: V5SelectionRequest<'_>,
        visitor: &mut V,
    ) -> Result<V5SelectionSummary, V5SelectionVisitError<V::Error>> {
        let snapshot = &self.snapshot;
        if snapshot.marker.confirmed_offset != request.expected_confirmed_offset
            || snapshot.marker.event_count != request.expected_event_count
            || &snapshot.marker.tail_hash != request.expected_tail_hash
            || v5_marker_fingerprint(&snapshot.marker)? != self.marker_fingerprint
        {
            return Err(IndexError::ProjectionContractViolation.into());
        }
        #[cfg(test)]
        let oracle_summary = {
            struct OracleNoop;
            impl V5SelectionVisitor for OracleNoop {
                type Error = ();
                fn visit(&mut self, _item: V5SelectionItem<'_>) -> Result<(), Self::Error> {
                    Ok(())
                }
            }
            visit_v5_snapshot_selection(snapshot, request, &mut OracleNoop).map_err(|error| {
                match error {
                    V5SelectionVisitError::Index(error) => V5SelectionVisitError::Index(error),
                    V5SelectionVisitError::Visitor(()) => {
                        V5SelectionVisitError::Index(IndexError::ProjectionContractViolation)
                    }
                }
            })?
        };
        let operational = account_snapshot(snapshot, 0, self.index.inner.limits)?.sql_bytes;
        let operational_limit = v5_selection_operational_limit();
        if operational > operational_limit {
            return Err(IndexError::Incomplete {
                limit: operational_limit,
                observed: operational,
            }
            .into());
        }
        let retained = recursive_ownership_charge(snapshot)?;
        let lock = self.index.inner.lock_shared()?;
        self.index.inner.audit_candidates(false)?;
        if self.index.inner.root.identity() != &self.store_root_identity {
            return Err(IndexError::ProjectionContractViolation.into());
        }
        let (session, basis) = journal
            .replayed_v4_session(self.roots)
            .map_err(IndexError::from)?;
        let session_matches_root = journal.matches_store_root(self.index.inner.root);
        let session_offset = session
            .index_v5_confirmed_offset()
            .map_err(IndexError::from)?;
        let session_event_count = session
            .index_v5_replay_projection()
            .map_err(IndexError::from)?
            .event_count;
        let session_tail = basis.confirmed_tail_hash();
        let session_run = basis.run_id();
        if !session_matches_root
            || session_offset != snapshot.marker.confirmed_offset
            || session_event_count != snapshot.marker.event_count
            || session_tail != &snapshot.marker.tail_hash
            || session_run != &snapshot.marker.run_id
            || basis.confirmed_event_count() != snapshot.marker.event_count
            || basis.confirmed_tail_hash() != &snapshot.marker.tail_hash
            || basis.policy_revision_hash() != &snapshot.marker.policy_revision_hash
            || basis.basis_digest() != &snapshot.marker.authority_replay_basis_digest
        {
            return Err(IndexError::ProjectionContractViolation.into());
        }
        let sqlite_limits = v5_sqlite_limits(self.index.inner.limits)?;
        let image = self
            .index
            .inner
            .read_active_image_locked_with_limits_and_retained(sqlite_limits, retained)?;
        if ContentHash::sha256(&image) != self.image_hash {
            return Err(IndexError::CorruptIndex.into());
        }
        let connection = super::deserialize_read_only_for_schema(
            image,
            sqlite_limits,
            retained,
            INDEX_SCHEMA_VERSION_V5,
        )
        .map_err(super::normalize_external_image_error)?;
        let max_json_staging = preflight_v5_structure(&connection, self.index.inner.limits)
            .map_err(super::normalize_external_image_error)?;
        admit_v5_json_staging(max_json_staging)?;
        validate_v5_structure_after_preflight(&connection, self.index.inner.limits)
            .map_err(super::normalize_external_image_error)?;
        let active_marker = marker_v5_from_connection(&connection)?;
        if active_marker != snapshot.marker
            || v5_marker_fingerprint(&active_marker)? != self.marker_fingerprint
        {
            return Err(IndexError::CorruptIndex.into());
        }
        validate_v5_rows(&connection, snapshot, self.index.inner.limits)
            .map_err(super::normalize_external_image_error)?;
        install_v5_selected_obligations(&connection, request.selected_obligation_ids)?;
        let summary = visit_v5_sql_selection(&connection, snapshot, request, visitor)?;
        #[cfg(test)]
        if summary != oracle_summary {
            return Err(IndexError::ProjectionContractViolation.into());
        }
        drop(connection);
        drop(session);
        lock.verify_unchanged()?;
        Ok(summary)
    }
}

fn install_v5_selected_obligations(
    connection: &rusqlite::Connection,
    selected: &BTreeSet<StableId>,
) -> Result<(), IndexError> {
    if selected.is_empty() {
        return Err(IndexError::ProjectionContractViolation);
    }
    connection.pragma_update(None, "query_only", false)?;
    connection
        .execute_batch("CREATE TEMP TABLE selected_obligations(id TEXT PRIMARY KEY) STRICT;")?;
    {
        let mut insert = connection.prepare("INSERT INTO temp.selected_obligations VALUES(?1)")?;
        for id in selected {
            insert.execute([id.as_str()])?;
        }
    }
    connection.pragma_update(None, "query_only", true)?;
    Ok(())
}

fn visit_v5_sql_selection<V: V5SelectionVisitor>(
    connection: &rusqlite::Connection,
    snapshot: &IndexSnapshotV5,
    request: V5SelectionRequest<'_>,
    visitor: &mut V,
) -> Result<V5SelectionSummary, V5SelectionVisitError<V::Error>> {
    let missing: i64 = connection.query_row(
        "SELECT count(*) FROM temp.selected_obligations s LEFT JOIN obligations o ON o.obligation_id=s.id WHERE o.obligation_id IS NULL",
        [],
        |row| row.get(0),
    )?;
    let plan_matches: i64 = connection.query_row(
        "WITH plan_ids(id) AS (SELECT j.value FROM review_plans p, json_each(p.waves_canonical_json) w, json_each(w.value,'$.obligation_ids') j WHERE p.plan_id=?1) SELECT count(*) FROM temp.selected_obligations s WHERE (SELECT count(*) FROM plan_ids p WHERE p.id=s.id)=1",
        rusqlite::params![request.plan_id.as_str()],
        |row| row.get(0),
    )?;
    let selected_count = i64::try_from(request.selected_obligation_ids.len())
        .map_err(|_| IndexError::IntegerOutOfRange)?;
    let bad_scope: i64 = connection.query_row(
        "SELECT count(*) FROM executions e WHERE e.plan_id=?1 AND EXISTS(SELECT 1 FROM json_each(e.obligation_ids_canonical_json) j JOIN temp.selected_obligations s ON s.id=j.value) AND json_array_length(e.obligation_ids_canonical_json)<>1",
        rusqlite::params![request.plan_id.as_str()],
        |row| row.get(0),
    )?;
    if missing != 0 || plan_matches != selected_count || bad_scope != 0 {
        return Err(IndexError::ProjectionContractViolation.into());
    }

    const SX: &str = "WITH sx AS (SELECT e.* FROM executions e JOIN temp.selected_obligations s ON s.id=json_extract(e.obligation_ids_canonical_json,'$[0]') WHERE e.plan_id=?1 AND json_array_length(e.obligation_ids_canonical_json)=1) ";
    let mut counts = V5SelectionCounts::default();
    let selector = V5SqlSelector {
        connection,
        plan_id: request.plan_id,
    };
    macro_rules! emit {
        ($rows:expr, $sql_tail:literal, $id:ident, $variant:ident, $count:ident) => {{
            let sql = format!("{SX}{}", $sql_tail);
            counts.$count = selector.emit_keyed_rows(
                &sql,
                &$rows,
                |row| (&row.$id, row.event_sequence),
                |row| V5SelectionItem::$variant(row),
                visitor,
            )?;
        }};
    }
    emit!(
        snapshot.executions,
        "SELECT event_sequence,execution_id FROM sx ORDER BY event_sequence,execution_id",
        execution_id,
        Execution,
        executions
    );
    emit!(
        snapshot.claims,
        "SELECT c.event_sequence,c.claim_id FROM claims c JOIN sx ON sx.execution_id=c.execution_id ORDER BY c.event_sequence,c.claim_id",
        claim_id,
        Claim,
        claims
    );
    emit!(
        snapshot.evidence_bindings,
        "SELECT b.event_sequence,b.binding_id FROM evidence_bindings_v3 b JOIN claims c ON c.claim_id=b.claim_id JOIN sx ON sx.execution_id=c.execution_id ORDER BY b.event_sequence,b.binding_id",
        binding_id,
        EvidenceBinding,
        evidence_bindings
    );
    emit!(
        snapshot.verifications,
        "SELECT v.event_sequence,v.verification_id FROM verifications_v3 v JOIN claims c ON c.claim_id=v.claim_id JOIN sx ON sx.execution_id=c.execution_id ORDER BY v.event_sequence,v.verification_id",
        verification_id,
        Verification,
        verifications
    );
    emit!(
        snapshot.decisions,
        "SELECT d.event_sequence,d.decision_id FROM decisions_v3 d JOIN claims c ON c.claim_id=d.claim_id JOIN sx ON sx.execution_id=c.execution_id ORDER BY d.event_sequence,d.decision_id",
        decision_id,
        Decision,
        decisions
    );
    emit!(
        snapshot.findings,
        "SELECT f.event_sequence,f.finding_id FROM findings_v3 f JOIN claims c ON c.claim_id=f.claim_id JOIN sx ON sx.execution_id=c.execution_id ORDER BY f.event_sequence,f.finding_id",
        finding_id,
        Finding,
        findings
    );
    let evidence_sql = format!(
        "{SX}SELECT DISTINCT ev.event_sequence,ev.evidence_id FROM evidence_v3 ev JOIN evidence_bindings_v3 b ON b.evidence_id=ev.evidence_id JOIN claims c ON c.claim_id=b.claim_id JOIN sx ON sx.execution_id=c.execution_id ORDER BY ev.event_sequence,ev.evidence_id"
    );
    counts.evidence = selector.emit_keyed_rows(
        &evidence_sql,
        &snapshot.evidence,
        |row| (&row.evidence_id, row.event_sequence),
        V5SelectionItem::Evidence,
        visitor,
    )?;
    let registration_sql = format!(
        "{SX}SELECT DISTINCT ar.event_sequence,ar.registration_id FROM artifact_registrations ar JOIN (SELECT raw_registration_id id FROM sx UNION SELECT ev.input_registration_id FROM evidence_v3 ev JOIN evidence_bindings_v3 b ON b.evidence_id=ev.evidence_id JOIN claims c ON c.claim_id=b.claim_id JOIN sx ON sx.execution_id=c.execution_id UNION SELECT ev.output_registration_id FROM evidence_v3 ev JOIN evidence_bindings_v3 b ON b.evidence_id=ev.evidence_id JOIN claims c ON c.claim_id=b.claim_id JOIN sx ON sx.execution_id=c.execution_id UNION SELECT v.input_registration_id FROM verifications_v3 v JOIN claims c ON c.claim_id=v.claim_id JOIN sx ON sx.execution_id=c.execution_id UNION SELECT v.output_registration_id FROM verifications_v3 v JOIN claims c ON c.claim_id=v.claim_id JOIN sx ON sx.execution_id=c.execution_id) wanted ON wanted.id=ar.registration_id ORDER BY ar.event_sequence,ar.registration_id"
    );
    counts.artifact_registrations = selector.emit_keyed_rows(
        &registration_sql,
        &snapshot.artifact_registrations,
        |row| (&row.registration_id, row.event_sequence),
        V5SelectionItem::ArtifactRegistration,
        visitor,
    )?;
    let assessment_sql = format!(
        "{SX}SELECT 0,a.claim_id FROM claim_assessments_v3 a JOIN claims c ON c.claim_id=a.claim_id JOIN sx ON sx.execution_id=c.execution_id ORDER BY a.claim_id"
    );
    counts.claim_assessments = selector.emit_keyed_rows(
        &assessment_sql,
        &snapshot.claim_assessments,
        |row| (&row.claim_id, 0),
        V5SelectionItem::ClaimAssessment,
        visitor,
    )?;
    let obstruction_sql = format!(
        "{SX}SELECT event_sequence,execution_id FROM sx WHERE outcome_kind<>'structured' ORDER BY event_sequence,execution_id"
    );
    counts.obstructions = selector.emit_keyed_rows(
        &obstruction_sql,
        &snapshot.executions,
        |row| (&row.execution_id, row.event_sequence),
        V5SelectionItem::Obstruction,
        visitor,
    )?;

    counts.denominator_ids = emit_v5_coverage_ids(
        connection,
        "SELECT obligation_id FROM obligations ORDER BY obligation_id",
        request.plan_id,
        &snapshot.obligations,
        V5CoverageAxis::Denominator,
        visitor,
    )?;
    counts.visited_ids = emit_v5_coverage_ids(
        connection,
        &format!(
            "{SX}SELECT DISTINCT json_extract(obligation_ids_canonical_json,'$[0]') FROM sx ORDER BY 1"
        ),
        request.plan_id,
        &snapshot.obligations,
        V5CoverageAxis::Visited,
        visitor,
    )?;
    counts.completed_ids = emit_v5_coverage_ids(
        connection,
        &format!(
            "{SX}SELECT DISTINCT o.obligation_id FROM obligations o JOIN sx ON json_extract(sx.obligation_ids_canonical_json,'$[0]')=o.obligation_id WHERE o.lifecycle='completed' AND sx.outcome_kind='structured' ORDER BY o.obligation_id"
        ),
        request.plan_id,
        &snapshot.obligations,
        V5CoverageAxis::Completed,
        visitor,
    )?;
    counts.evidence_supported_ids = emit_v5_coverage_ids(
        connection,
        &format!(
            "{SX}SELECT DISTINCT json_extract(c.obligation_ids_canonical_json,'$[0]') FROM claims c JOIN sx ON sx.execution_id=c.execution_id JOIN evidence_bindings_v3 b ON b.claim_id=c.claim_id WHERE b.relation='reproduces' ORDER BY 1"
        ),
        request.plan_id,
        &snapshot.obligations,
        V5CoverageAxis::EvidenceSupported,
        visitor,
    )?;
    let verified_sql = format!(
        "{SX}SELECT DISTINCT json_extract(c.obligation_ids_canonical_json,'$[0]') FROM claims c JOIN sx ON sx.execution_id=c.execution_id JOIN verifications_v3 v ON v.claim_id=c.claim_id WHERE v.outcome='passed' AND json_array_length(v.evidence_ids_canonical_json)>0 AND NOT EXISTS (SELECT 1 FROM json_each(v.evidence_ids_canonical_json) cited WHERE NOT EXISTS (SELECT 1 FROM evidence_bindings_v3 b WHERE b.claim_id=c.claim_id AND b.evidence_id=cited.value AND b.relation='reproduces')) ORDER BY 1"
    );
    counts.verified_ids = emit_v5_coverage_ids(
        connection,
        &verified_sql,
        request.plan_id,
        &snapshot.obligations,
        V5CoverageAxis::Verified,
        visitor,
    )?;
    let accepted_sql = "WITH sx AS (SELECT e.* FROM executions e JOIN temp.selected_obligations s ON s.id=json_extract(e.obligation_ids_canonical_json,'$[0]') WHERE e.plan_id=?1 AND json_array_length(e.obligation_ids_canonical_json)=1), qualifying AS (SELECT v.claim_id,v.verification_id FROM verifications_v3 v WHERE v.outcome='passed' AND json_array_length(v.evidence_ids_canonical_json)>0 AND NOT EXISTS (SELECT 1 FROM json_each(v.evidence_ids_canonical_json) cited WHERE NOT EXISTS (SELECT 1 FROM evidence_bindings_v3 b WHERE b.claim_id=v.claim_id AND b.evidence_id=cited.value AND b.relation='reproduces'))) SELECT DISTINCT json_extract(c.obligation_ids_canonical_json,'$[0]') FROM claims c JOIN sx ON sx.execution_id=c.execution_id JOIN claim_assessments_v3 a ON a.claim_id=c.claim_id JOIN findings_v3 f ON f.finding_id=a.current_finding_id AND f.claim_id=c.claim_id JOIN decisions_v3 d ON d.decision_id=a.active_decision_id AND d.claim_id=c.claim_id JOIN qualifying q ON q.claim_id=c.claim_id WHERE a.decision_conflict=0 AND f.status='accepted' AND f.decision_id=d.decision_id AND d.outcome='accept' AND NOT EXISTS (SELECT 1 FROM json_each(f.evidence_ids_canonical_json) fe WHERE NOT EXISTS (SELECT 1 FROM evidence_bindings_v3 b WHERE b.claim_id=c.claim_id AND b.evidence_id=fe.value AND b.relation='reproduces')) AND NOT EXISTS (SELECT 1 FROM evidence_bindings_v3 b WHERE b.claim_id=c.claim_id AND b.relation='reproduces' AND NOT EXISTS (SELECT 1 FROM json_each(f.evidence_ids_canonical_json) fe WHERE fe.value=b.evidence_id)) AND NOT EXISTS (SELECT 1 FROM json_each(f.verification_ids_canonical_json) fv WHERE NOT EXISTS (SELECT 1 FROM qualifying q2 WHERE q2.claim_id=c.claim_id AND q2.verification_id=fv.value)) AND NOT EXISTS (SELECT 1 FROM qualifying q2 WHERE q2.claim_id=c.claim_id AND NOT EXISTS (SELECT 1 FROM json_each(f.verification_ids_canonical_json) fv WHERE fv.value=q2.verification_id)) ORDER BY 1";
    counts.accepted_ids = emit_v5_coverage_ids(
        connection,
        accepted_sql,
        request.plan_id,
        &snapshot.obligations,
        V5CoverageAxis::Accepted,
        visitor,
    )?;
    let max_sql = format!(
        "SELECT COALESCE(MAX(size),0) FROM artifact_registrations WHERE registration_id IN (SELECT registration_id FROM ({registration_sql}))"
    );
    let mut max_statement = connection.prepare(&max_sql)?;
    let max_cas_i64: i64 = match max_statement.parameter_count() {
        1 => max_statement.query_row(rusqlite::params![request.plan_id.as_str()], |row| {
            row.get(0)
        })?,
        _ => return Err(IndexError::ProjectionContractViolation.into()),
    };
    let max_cas_bytes =
        u64::try_from(max_cas_i64).map_err(|_| IndexError::ProjectionContractViolation)?;
    Ok(V5SelectionSummary {
        counts,
        max_cas_bytes,
        confirmed_offset: snapshot.marker.confirmed_offset,
        event_count: snapshot.marker.event_count,
        marker_fingerprint: v5_marker_fingerprint(&snapshot.marker)?,
    })
}

struct V5SqlSelector<'a> {
    connection: &'a rusqlite::Connection,
    plan_id: &'a StableId,
}

impl V5SqlSelector<'_> {
    fn emit_keyed_rows<'a, T, V, K, I>(
        &self,
        sql: &str,
        source: &'a [T],
        key: K,
        item: I,
        visitor: &mut V,
    ) -> Result<u64, V5SelectionVisitError<V::Error>>
    where
        V: V5SelectionVisitor,
        K: Fn(&T) -> (&StableId, u64),
        I: Fn(&'a T) -> V5SelectionItem<'a>,
    {
        let mut statement = self.connection.prepare(sql)?;
        let mut rows = match statement.parameter_count() {
            0 => statement.query([])?,
            1 => statement.query(rusqlite::params![self.plan_id.as_str()])?,
            _ => return Err(IndexError::ProjectionContractViolation.into()),
        };
        let mut cursor = 0_usize;
        let mut count = 0_u64;
        while let Some(row) = rows.next()? {
            let sequence = u64::try_from(row.get::<_, i64>(0)?)
                .map_err(|_| IndexError::ProjectionContractViolation)?;
            let ValueRef::Text(bytes) = row.get_ref(1)? else {
                return Err(IndexError::ProjectionContractViolation.into());
            };
            let id =
                std::str::from_utf8(bytes).map_err(|_| IndexError::ProjectionContractViolation)?;
            while let Some(candidate) = source.get(cursor) {
                let (candidate_id, candidate_sequence) = key(candidate);
                if (candidate_sequence, candidate_id.as_str()) < (sequence, id) {
                    cursor += 1;
                } else {
                    break;
                }
            }
            let candidate = source
                .get(cursor)
                .ok_or(IndexError::ProjectionContractViolation)?;
            let (candidate_id, candidate_sequence) = key(candidate);
            if candidate_sequence != sequence || candidate_id.as_str() != id {
                return Err(IndexError::ProjectionContractViolation.into());
            }
            visitor
                .visit(item(candidate))
                .map_err(V5SelectionVisitError::Visitor)?;
            cursor += 1;
            count = count.checked_add(1).ok_or(IndexError::IntegerOutOfRange)?;
        }
        Ok(count)
    }
}

fn emit_v5_coverage_ids<V: V5SelectionVisitor>(
    connection: &rusqlite::Connection,
    sql: &str,
    plan_id: &StableId,
    denominator: &[IndexObligation],
    axis: V5CoverageAxis,
    visitor: &mut V,
) -> Result<u64, V5SelectionVisitError<V::Error>> {
    let mut statement = connection.prepare(sql)?;
    let mut rows = match statement.parameter_count() {
        0 => statement.query([])?,
        1 => statement.query(rusqlite::params![plan_id.as_str()])?,
        _ => return Err(IndexError::ProjectionContractViolation.into()),
    };
    let mut cursor = 0_usize;
    let mut count = 0_u64;
    while let Some(row) = rows.next()? {
        let ValueRef::Text(bytes) = row.get_ref(0)? else {
            return Err(IndexError::ProjectionContractViolation.into());
        };
        let id = std::str::from_utf8(bytes).map_err(|_| IndexError::ProjectionContractViolation)?;
        while denominator
            .get(cursor)
            .is_some_and(|row| row.obligation_id.as_str() < id)
        {
            cursor += 1;
        }
        let obligation = denominator
            .get(cursor)
            .filter(|row| row.obligation_id.as_str() == id)
            .ok_or(IndexError::ProjectionContractViolation)?;
        visitor
            .visit(V5SelectionItem::CoverageId {
                axis,
                id: &obligation.obligation_id,
            })
            .map_err(V5SelectionVisitError::Visitor)?;
        cursor += 1;
        count = count.checked_add(1).ok_or(IndexError::IntegerOutOfRange)?;
    }
    Ok(count)
}

#[cfg(test)]
fn visit_v5_snapshot_selection<V: V5SelectionVisitor>(
    snapshot: &IndexSnapshotV5,
    request: V5SelectionRequest<'_>,
    visitor: &mut V,
) -> Result<V5SelectionSummary, V5SelectionVisitError<V::Error>> {
    if request.selected_obligation_ids.is_empty() {
        return Err(IndexError::ProjectionContractViolation.into());
    }
    if snapshot.marker.confirmed_offset != request.expected_confirmed_offset
        || snapshot.marker.event_count != request.expected_event_count
        || &snapshot.marker.tail_hash != request.expected_tail_hash
    {
        return Err(IndexError::ProjectionContractViolation.into());
    }
    // Admit the complete internal selection workspace before the first
    // BTree node or decoded ID list is allocated.  The reservation is a
    // deterministic upper image of every ID-bearing row and canonical-ID
    // cell the selector may retain; it intentionally exposes none of those
    // internal sets to the caller.
    let selection_peak =
        v5_selection_operational_upper_bound(snapshot, request.selected_obligation_ids)?;
    let operational_limit = V5_SELECTION_OPERATIONAL_LIMIT;
    if selection_peak > operational_limit {
        return Err(IndexError::Incomplete {
            limit: operational_limit,
            observed: selection_peak,
        }
        .into());
    }
    let universe = snapshot
        .universe
        .as_ref()
        .ok_or(IndexError::ProjectionContractViolation)?;
    let denominator: BTreeSet<_> = snapshot
        .obligations
        .iter()
        .map(|row| row.obligation_id.clone())
        .collect();
    if denominator.len() != snapshot.obligations.len()
        || !request.selected_obligation_ids.is_subset(&denominator)
    {
        return Err(IndexError::ProjectionContractViolation.into());
    }
    let plan = snapshot
        .review_plans
        .iter()
        .find(|row| &row.plan_id == request.plan_id)
        .ok_or(IndexError::ProjectionContractViolation)?;
    if plan.universe_id != universe.universe_id
        || plan.snapshot_id != universe.snapshot_id
        || !v5_plan_contains_all(&plan.waves_canonical_json, request.selected_obligation_ids)?
    {
        return Err(IndexError::ProjectionContractViolation.into());
    }

    let mut execution_ids = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut structured = BTreeSet::new();
    let mut registration_ids = BTreeSet::new();
    for row in snapshot
        .executions
        .iter()
        .filter(|row| &row.plan_id == request.plan_id)
    {
        let scope = v5_strict_ids(&row.obligation_ids_canonical_json)?;
        if scope.is_disjoint(request.selected_obligation_ids) {
            continue;
        }
        if scope.len() != 1 || !scope.is_subset(request.selected_obligation_ids) {
            return Err(IndexError::ProjectionContractViolation.into());
        }
        if row.snapshot_id != universe.snapshot_id
            || !execution_ids.insert(row.execution_id.clone())
        {
            return Err(IndexError::ProjectionContractViolation.into());
        }
        visited.extend(scope.iter().cloned());
        if row.outcome_kind == "structured" {
            structured.extend(scope);
        } else if !matches!(
            row.outcome_kind.as_str(),
            "abstained" | "malformed" | "provider_failure"
        ) {
            return Err(IndexError::ProjectionContractViolation.into());
        }
        registration_ids.insert(row.raw_registration_id.clone());
    }

    let mut claim_ids = BTreeSet::new();
    let mut claim_obligation = BTreeMap::new();
    let mut claims_by_execution = BTreeMap::<StableId, BTreeSet<StableId>>::new();
    for row in snapshot
        .claims
        .iter()
        .filter(|row| execution_ids.contains(&row.execution_id))
    {
        let scope = v5_strict_ids(&row.obligation_ids_canonical_json)?;
        if scope.len() != 1
            || !scope.is_subset(request.selected_obligation_ids)
            || !claim_ids.insert(row.claim_id.clone())
        {
            return Err(IndexError::ProjectionContractViolation.into());
        }
        claim_obligation.insert(
            row.claim_id.clone(),
            scope
                .into_iter()
                .next()
                .ok_or(IndexError::ProjectionContractViolation)?,
        );
        claims_by_execution
            .entry(row.execution_id.clone())
            .or_default()
            .insert(row.claim_id.clone());
    }
    for execution in snapshot
        .executions
        .iter()
        .filter(|row| execution_ids.contains(&row.execution_id))
    {
        let expected = v5_strict_ids(&execution.parsed_claim_ids_canonical_json)?;
        let actual = claims_by_execution.get(&execution.execution_id);
        if actual.is_none_or(|ids| ids != &expected)
            || (execution.outcome_kind == "structured") == expected.is_empty()
        {
            return Err(IndexError::ProjectionContractViolation.into());
        }
    }

    let binding_ids: BTreeSet<_> = snapshot
        .evidence_bindings
        .iter()
        .filter(|row| claim_ids.contains(&row.claim_id))
        .map(|row| row.binding_id.clone())
        .collect();
    let evidence_ids: BTreeSet<_> = snapshot
        .evidence_bindings
        .iter()
        .filter(|row| binding_ids.contains(&row.binding_id))
        .map(|row| row.evidence_id.clone())
        .collect();
    let mut present_evidence_ids = BTreeSet::new();
    for row in snapshot
        .evidence
        .iter()
        .filter(|row| evidence_ids.contains(&row.evidence_id))
    {
        if row.snapshot_id != universe.snapshot_id {
            return Err(IndexError::ProjectionContractViolation.into());
        }
        if !present_evidence_ids.insert(row.evidence_id.clone()) {
            return Err(IndexError::ProjectionContractViolation.into());
        }
        registration_ids.insert(row.input_registration_id.clone());
        registration_ids.insert(row.output_registration_id.clone());
    }
    let verification_ids: BTreeSet<_> = snapshot
        .verifications
        .iter()
        .filter(|row| claim_ids.contains(&row.claim_id))
        .map(|row| row.verification_id.clone())
        .collect();
    for row in snapshot
        .verifications
        .iter()
        .filter(|row| verification_ids.contains(&row.verification_id))
    {
        if !v5_strict_ids(&row.evidence_ids_canonical_json)?.is_subset(&evidence_ids) {
            return Err(IndexError::ProjectionContractViolation.into());
        }
        registration_ids.insert(row.input_registration_id.clone());
        registration_ids.insert(row.output_registration_id.clone());
    }
    let decision_ids: BTreeSet<_> = snapshot
        .decisions
        .iter()
        .filter(|row| claim_ids.contains(&row.claim_id))
        .map(|row| row.decision_id.clone())
        .collect();
    let finding_ids: BTreeSet<_> = snapshot
        .findings
        .iter()
        .filter(|row| claim_ids.contains(&row.claim_id))
        .map(|row| row.finding_id.clone())
        .collect();
    let assessment_ids: BTreeSet<_> = snapshot
        .claim_assessments
        .iter()
        .filter(|row| claim_ids.contains(&row.claim_id))
        .map(|row| row.claim_id.clone())
        .collect();
    let present_registration_ids: BTreeSet<_> = snapshot
        .artifact_registrations
        .iter()
        .filter(|row| registration_ids.contains(&row.registration_id))
        .map(|row| row.registration_id.clone())
        .collect();
    if assessment_ids != claim_ids
        || present_evidence_ids != evidence_ids
        || present_registration_ids != registration_ids
    {
        return Err(IndexError::ProjectionContractViolation.into());
    }
    for row in snapshot
        .decisions
        .iter()
        .filter(|row| decision_ids.contains(&row.decision_id))
    {
        if row.run_id != snapshot.marker.run_id
            || row.universe_id != universe.universe_id
            || row.snapshot_id != universe.snapshot_id
        {
            return Err(IndexError::ProjectionContractViolation.into());
        }
    }

    let completed: BTreeSet<_> = snapshot
        .obligations
        .iter()
        .filter(|row| row.lifecycle == "completed" && structured.contains(&row.obligation_id))
        .map(|row| row.obligation_id.clone())
        .collect();
    let reproduces: BTreeSet<_> = snapshot
        .evidence_bindings
        .iter()
        .filter(|row| binding_ids.contains(&row.binding_id) && row.relation == "reproduces")
        .map(|row| (row.claim_id.clone(), row.evidence_id.clone()))
        .collect();
    let mut reproduces_by_claim = BTreeMap::<StableId, BTreeSet<StableId>>::new();
    for (claim_id, evidence_id) in &reproduces {
        reproduces_by_claim
            .entry(claim_id.clone())
            .or_default()
            .insert(evidence_id.clone());
    }
    let evidence_supported: BTreeSet<_> = reproduces
        .iter()
        .filter_map(|(claim_id, _)| claim_obligation.get(claim_id).cloned())
        .collect();
    let mut passed = BTreeMap::<StableId, BTreeSet<StableId>>::new();
    for row in snapshot
        .verifications
        .iter()
        .filter(|row| verification_ids.contains(&row.verification_id))
    {
        let cited = v5_strict_ids(&row.evidence_ids_canonical_json)?;
        if row.outcome == "passed"
            && !cited.is_empty()
            && cited
                .iter()
                .all(|id| reproduces.contains(&(row.claim_id.clone(), id.clone())))
        {
            passed
                .entry(row.claim_id.clone())
                .or_default()
                .insert(row.verification_id.clone());
        }
    }
    let verified: BTreeSet<_> = passed
        .keys()
        .filter_map(|id| claim_obligation.get(id).cloned())
        .collect();
    let findings_by_id: BTreeMap<_, _> = snapshot
        .findings
        .iter()
        .filter(|row| finding_ids.contains(&row.finding_id))
        .map(|row| (row.finding_id.clone(), row))
        .collect();
    let decisions_by_id: BTreeMap<_, _> = snapshot
        .decisions
        .iter()
        .filter(|row| decision_ids.contains(&row.decision_id))
        .map(|row| (row.decision_id.clone(), row))
        .collect();
    let mut accepted = BTreeSet::new();
    for assessment in snapshot
        .claim_assessments
        .iter()
        .filter(|row| assessment_ids.contains(&row.claim_id))
    {
        if assessment.decision_conflict || !passed.contains_key(&assessment.claim_id) {
            continue;
        }
        let (Some(finding_id), Some(decision_id)) = (
            assessment.current_finding_id.as_ref(),
            assessment.active_decision_id.as_ref(),
        ) else {
            continue;
        };
        let finding = findings_by_id
            .get(finding_id)
            .copied()
            .ok_or(IndexError::ProjectionContractViolation)?;
        let decision = decisions_by_id
            .get(decision_id)
            .copied()
            .ok_or(IndexError::ProjectionContractViolation)?;
        let empty_reproduces = BTreeSet::new();
        let all_reproduces = reproduces_by_claim
            .get(&assessment.claim_id)
            .unwrap_or(&empty_reproduces);
        if finding.claim_id == assessment.claim_id
            && finding.status == "accepted"
            && finding.decision_id.as_ref() == Some(decision_id)
            && decision.claim_id == assessment.claim_id
            && decision.outcome == "accept"
            && &v5_strict_ids(&finding.evidence_ids_canonical_json)? == all_reproduces
            && v5_strict_ids(&finding.verification_ids_canonical_json)?
                == *passed
                    .get(&assessment.claim_id)
                    .ok_or(IndexError::ProjectionContractViolation)?
        {
            accepted.insert(
                claim_obligation
                    .get(&assessment.claim_id)
                    .ok_or(IndexError::ProjectionContractViolation)?
                    .clone(),
            );
        }
    }

    let mut counts = V5SelectionCounts::default();
    macro_rules! emit_rows {
        ($rows:expr, $predicate:expr, $variant:ident, $count:ident) => {
            for row in $rows.iter().filter($predicate) {
                visitor
                    .visit(V5SelectionItem::$variant(row))
                    .map_err(V5SelectionVisitError::Visitor)?;
                counts.$count = counts
                    .$count
                    .checked_add(1)
                    .ok_or(V5SelectionVisitError::Index(IndexError::IntegerOutOfRange))?;
            }
        };
    }
    emit_rows!(
        snapshot.artifact_registrations,
        |row| registration_ids.contains(&row.registration_id),
        ArtifactRegistration,
        artifact_registrations
    );
    emit_rows!(
        snapshot.executions,
        |row| execution_ids.contains(&row.execution_id),
        Execution,
        executions
    );
    emit_rows!(
        snapshot.claims,
        |row| claim_ids.contains(&row.claim_id),
        Claim,
        claims
    );
    emit_rows!(
        snapshot.evidence,
        |row| evidence_ids.contains(&row.evidence_id),
        Evidence,
        evidence
    );
    emit_rows!(
        snapshot.evidence_bindings,
        |row| binding_ids.contains(&row.binding_id),
        EvidenceBinding,
        evidence_bindings
    );
    emit_rows!(
        snapshot.verifications,
        |row| verification_ids.contains(&row.verification_id),
        Verification,
        verifications
    );
    emit_rows!(
        snapshot.decisions,
        |row| decision_ids.contains(&row.decision_id),
        Decision,
        decisions
    );
    emit_rows!(
        snapshot.findings,
        |row| finding_ids.contains(&row.finding_id),
        Finding,
        findings
    );
    emit_rows!(
        snapshot.claim_assessments,
        |row| assessment_ids.contains(&row.claim_id),
        ClaimAssessment,
        claim_assessments
    );
    emit_rows!(
        snapshot.executions,
        |row| execution_ids.contains(&row.execution_id) && row.outcome_kind != "structured",
        Obstruction,
        obstructions
    );

    for (axis, ids, count) in [
        (
            V5CoverageAxis::Denominator,
            &denominator,
            &mut counts.denominator_ids,
        ),
        (V5CoverageAxis::Visited, &visited, &mut counts.visited_ids),
        (
            V5CoverageAxis::Completed,
            &completed,
            &mut counts.completed_ids,
        ),
        (
            V5CoverageAxis::EvidenceSupported,
            &evidence_supported,
            &mut counts.evidence_supported_ids,
        ),
        (
            V5CoverageAxis::Verified,
            &verified,
            &mut counts.verified_ids,
        ),
        (
            V5CoverageAxis::Accepted,
            &accepted,
            &mut counts.accepted_ids,
        ),
    ] {
        for id in ids {
            visitor
                .visit(V5SelectionItem::CoverageId { axis, id })
                .map_err(V5SelectionVisitError::Visitor)?;
            *count = count
                .checked_add(1)
                .ok_or(V5SelectionVisitError::Index(IndexError::IntegerOutOfRange))?;
        }
    }
    let max_cas_bytes = snapshot
        .artifact_registrations
        .iter()
        .filter(|row| registration_ids.contains(&row.registration_id))
        .map(|row| row.size)
        .max()
        .unwrap_or(0);
    Ok(V5SelectionSummary {
        counts,
        max_cas_bytes,
        confirmed_offset: snapshot.marker.confirmed_offset,
        event_count: snapshot.marker.event_count,
        marker_fingerprint: v5_marker_fingerprint(&snapshot.marker)?,
    })
}

#[cfg(test)]
fn v5_selection_operational_upper_bound(
    snapshot: &IndexSnapshotV5,
    selected: &BTreeSet<StableId>,
) -> Result<u64, IndexError> {
    let snapshot_owned = recursive_ownership_charge(snapshot)?;
    let mut id_bytes = 0_u64;
    let mut id_slots = 0_u64;
    let mut add_id = |id: &StableId| -> Result<(), IndexError> {
        id_slots = id_slots.checked_add(1).ok_or(IndexError::Incomplete {
            limit: u64::MAX,
            observed: u64::MAX,
        })?;
        id_bytes = id_bytes
            .checked_add(
                u64::try_from(id.allocated_bytes()).map_err(|_| IndexError::IntegerOutOfRange)?,
            )
            .ok_or(IndexError::Incomplete {
                limit: u64::MAX,
                observed: u64::MAX,
            })?;
        Ok(())
    };
    for id in selected {
        add_id(id)?;
    }
    for row in &snapshot.obligations {
        add_id(&row.obligation_id)?;
    }
    for row in &snapshot.executions {
        add_id(&row.execution_id)?;
        add_id(&row.raw_registration_id)?;
    }
    for row in &snapshot.claims {
        add_id(&row.claim_id)?;
        add_id(&row.execution_id)?;
    }
    for row in &snapshot.evidence_bindings {
        add_id(&row.binding_id)?;
        add_id(&row.claim_id)?;
        add_id(&row.evidence_id)?;
    }
    for row in &snapshot.evidence {
        add_id(&row.evidence_id)?;
        add_id(&row.input_registration_id)?;
        add_id(&row.output_registration_id)?;
    }
    for row in &snapshot.verifications {
        add_id(&row.verification_id)?;
        add_id(&row.claim_id)?;
        add_id(&row.input_registration_id)?;
        add_id(&row.output_registration_id)?;
    }
    for row in &snapshot.decisions {
        add_id(&row.decision_id)?;
        add_id(&row.claim_id)?;
    }
    for row in &snapshot.findings {
        add_id(&row.finding_id)?;
        add_id(&row.claim_id)?;
        if let Some(id) = &row.decision_id {
            add_id(id)?;
        }
    }
    for row in &snapshot.claim_assessments {
        add_id(&row.claim_id)?;
        if let Some(id) = &row.active_decision_id {
            add_id(id)?;
        }
        if let Some(id) = &row.current_finding_id {
            add_id(id)?;
        }
    }
    for row in &snapshot.artifact_registrations {
        add_id(&row.registration_id)?;
    }
    let canonical_id_bytes = snapshot
        .executions
        .iter()
        .map(|row| {
            row.obligation_ids_canonical_json
                .len()
                .saturating_add(row.parsed_claim_ids_canonical_json.len())
        })
        .chain(
            snapshot
                .claims
                .iter()
                .map(|row| row.obligation_ids_canonical_json.len()),
        )
        .chain(
            snapshot
                .verifications
                .iter()
                .map(|row| row.evidence_ids_canonical_json.len()),
        )
        .chain(snapshot.findings.iter().map(|row| {
            row.evidence_ids_canonical_json
                .len()
                .saturating_add(row.verification_ids_canonical_json.len())
        }))
        .try_fold(0_u64, |total, bytes| {
            total
                .checked_add(u64::try_from(bytes).map_err(|_| IndexError::IntegerOutOfRange)?)
                .ok_or(IndexError::Incomplete {
                    limit: u64::MAX,
                    observed: u64::MAX,
                })
        })?;
    // Thirty-six retained copies/node-slots cover the selector's simultaneous
    // sets/maps and one transient decoded canonical-ID collection.  Each
    // slot includes the owned StableId plus a conservative fixed tree node.
    let set_workspace = id_bytes
        .checked_add(id_slots.checked_mul(48).ok_or(IndexError::Incomplete {
            limit: u64::MAX,
            observed: u64::MAX,
        })?)
        .and_then(|value| value.checked_mul(36))
        .and_then(|value| value.checked_add(canonical_id_bytes))
        .ok_or(IndexError::Incomplete {
            limit: u64::MAX,
            observed: u64::MAX,
        })?;
    snapshot_owned
        .checked_add(set_workspace)
        .ok_or(IndexError::Incomplete {
            limit: u64::MAX,
            observed: u64::MAX,
        })
}

#[cfg(test)]
pub(crate) fn v5_selection_sql_operational_charge_for_test(
    snapshot: &IndexSnapshotV5,
    _selected: &BTreeSet<StableId>,
    limits: IndexLimits,
) -> Result<u64, IndexError> {
    Ok(account_snapshot(snapshot, 0, limits)?.sql_bytes)
}

#[cfg(test)]
fn v5_strict_ids(input: &str) -> Result<BTreeSet<StableId>, IndexError> {
    let values: Vec<StableId> =
        serde_json::from_str(input).map_err(|_| IndexError::ProjectionContractViolation)?;
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(IndexError::ProjectionContractViolation);
    }
    Ok(values.into_iter().collect())
}

#[cfg(test)]
#[derive(Deserialize)]
struct V5PlanWave {
    obligation_ids: Vec<StableId>,
    wave_index: u32,
}

#[cfg(test)]
fn v5_plan_contains_all(input: &str, selected: &BTreeSet<StableId>) -> Result<bool, IndexError> {
    let waves: Vec<V5PlanWave> =
        serde_json::from_str(input).map_err(|_| IndexError::ProjectionContractViolation)?;
    if waves
        .windows(2)
        .any(|pair| pair[0].wave_index >= pair[1].wave_index)
    {
        return Err(IndexError::ProjectionContractViolation);
    }
    Ok(selected.iter().all(|wanted| {
        waves
            .iter()
            .flat_map(|wave| wave.obligation_ids.iter())
            .filter(|id| *id == wanted)
            .count()
            == 1
    }))
}

fn v5_marker_fingerprint(marker: &IndexMarkerV5) -> Result<[u8; 32], IndexError> {
    let canonical = canonical_json(marker).map_err(|_| IndexError::ProjectionContractViolation)?;
    let hash = ContentHash::sha256(&canonical);
    let hex = hash
        .as_str()
        .strip_prefix("sha256:")
        .ok_or(IndexError::ProjectionContractViolation)?;
    let mut output = [0_u8; 32];
    for (slot, pair) in output.iter_mut().zip(hex.as_bytes().chunks_exact(2)) {
        let pair =
            std::str::from_utf8(pair).map_err(|_| IndexError::ProjectionContractViolation)?;
        *slot =
            u8::from_str_radix(pair, 16).map_err(|_| IndexError::ProjectionContractViolation)?;
    }
    Ok(output)
}

fn v5_sqlite_limits(mut limits: IndexLimits) -> Result<IndexLimits, IndexError> {
    limits.max_working_bytes = limits
        .max_serialized_bytes
        .checked_mul(3)
        .and_then(|value| value.checked_add(limits.max_working_bytes))
        .and_then(|value| value.checked_add(1024))
        .ok_or(IndexError::InvalidLimits)?;
    Ok(limits)
}

struct ProjectedV5 {
    snapshot: IndexSnapshotV5,
    accounting: IndexAccountingV5,
}

trait ProjectionSourceV5 {
    fn genesis(&self) -> Result<&RunGenesisSnapshot, IndexError>;
    fn replay_projection(&self) -> Result<&ReplayProjectionChargeV5, IndexError>;
    fn visit_projection(
        &self,
        visitor: &mut dyn for<'event> FnMut(
            BorrowedV4EventMetadata<'event>,
            CoreBorrowedProjectionPayloadV4<'event>,
        ),
    ) -> Result<(), IndexError>;
    fn confirmed_offset(&self) -> Result<u64, IndexError>;
    fn obligation_lifecycle(
        &self,
        obligation_id: &StableId,
    ) -> Result<reviewgraphen_core::ObligationLifecycle, IndexError>;
    fn for_each_claim_assessment(
        &self,
        visitor: &mut dyn FnMut(reviewgraphen_core::BorrowedClaimAssessmentProjectionV4<'_>),
    ) -> Result<(), IndexError>;
}

#[allow(dead_code)]
fn artifact_source_v4_kind(source: &ArtifactSourceV4) -> &'static str {
    match source {
        ArtifactSourceV4::RunGenesis { .. } => "run_genesis",
        ArtifactSourceV4::SnapshotIngest { .. } => "snapshot_ingest",
        ArtifactSourceV4::ReviewerExecution { .. } => "reviewer_execution",
        ArtifactSourceV4::VerifierArtifact { .. } => "verifier_artifact",
        ArtifactSourceV4::ExternalHarnessWitness { .. } => "external_harness_witness",
        ArtifactSourceV4::GluingInput { .. } => "gluing_input",
    }
}

fn obligation_body_hash_at_lifecycle(
    obligation: &reviewgraphen_core::Obligation,
    lifecycle: &reviewgraphen_core::ObligationLifecycle,
) -> Result<ContentHash, IndexError> {
    let mut value =
        serde_json::to_value(obligation).map_err(|_| IndexError::ProjectionContractViolation)?;
    let object = value
        .as_object_mut()
        .ok_or(IndexError::ProjectionContractViolation)?;
    object.insert(
        "lifecycle".to_owned(),
        serde_json::to_value(lifecycle).map_err(|_| IndexError::ProjectionContractViolation)?,
    );
    Ok(ContentHash::sha256(
        &canonical_json(&value).map_err(|_| IndexError::ProjectionContractViolation)?,
    ))
}

impl ProjectionSourceV5 for crate::ReplayedV4RunSession<'_, '_> {
    fn genesis(&self) -> Result<&RunGenesisSnapshot, IndexError> {
        Ok(self.index_v5_genesis()?)
    }

    fn replay_projection(&self) -> Result<&ReplayProjectionChargeV5, IndexError> {
        Ok(self.index_v5_replay_projection()?)
    }

    fn visit_projection(
        &self,
        visitor: &mut dyn for<'event> FnMut(
            BorrowedV4EventMetadata<'event>,
            CoreBorrowedProjectionPayloadV4<'event>,
        ),
    ) -> Result<(), IndexError> {
        Ok(self.index_v5_visit_projection(visitor)?)
    }

    fn confirmed_offset(&self) -> Result<u64, IndexError> {
        Ok(self.index_v5_confirmed_offset()?)
    }

    fn obligation_lifecycle(
        &self,
        obligation_id: &StableId,
    ) -> Result<reviewgraphen_core::ObligationLifecycle, IndexError> {
        self.index_v5_obligation_lifecycle(obligation_id)?
            .ok_or(IndexError::ProjectionContractViolation)
    }

    fn for_each_claim_assessment(
        &self,
        visitor: &mut dyn FnMut(reviewgraphen_core::BorrowedClaimAssessmentProjectionV4<'_>),
    ) -> Result<(), IndexError> {
        Ok(self.index_v5_for_each_claim_assessment(visitor)?)
    }
}

impl ProjectionSourceV5 for IndexV5ReplayedPrefix {
    fn genesis(&self) -> Result<&RunGenesisSnapshot, IndexError> {
        Ok(self.genesis())
    }

    fn replay_projection(&self) -> Result<&ReplayProjectionChargeV5, IndexError> {
        Ok(self.projection())
    }

    fn visit_projection(
        &self,
        visitor: &mut dyn for<'event> FnMut(
            BorrowedV4EventMetadata<'event>,
            CoreBorrowedProjectionPayloadV4<'event>,
        ),
    ) -> Result<(), IndexError> {
        Ok(IndexV5ReplayedPrefix::visit_projection(self, visitor)?)
    }

    fn confirmed_offset(&self) -> Result<u64, IndexError> {
        Ok(self.confirmed_offset())
    }

    fn obligation_lifecycle(
        &self,
        obligation_id: &StableId,
    ) -> Result<reviewgraphen_core::ObligationLifecycle, IndexError> {
        self.obligation_lifecycle(obligation_id)
            .ok_or(IndexError::ProjectionContractViolation)
    }

    fn for_each_claim_assessment(
        &self,
        visitor: &mut dyn FnMut(reviewgraphen_core::BorrowedClaimAssessmentProjectionV4<'_>),
    ) -> Result<(), IndexError> {
        self.for_each_claim_assessment(visitor);
        Ok(())
    }
}

fn project_verified_source<S: ProjectionSourceV5>(
    session: &S,
    basis: &AuthorityReplayBasisV4,
    limits: IndexLimits,
) -> Result<ProjectedV5, IndexError> {
    let initial = session.genesis()?;
    let replay = session.replay_projection()?;
    let event_count = replay.event_count;
    if event_count != basis.confirmed_event_count() {
        return Err(IndexError::ProjectionContractViolation);
    }
    if event_count == 0 || replay.confirmed_offset != session.confirmed_offset()? {
        return Err(IndexError::ProjectionContractViolation);
    }
    let confirmed_offset = session.confirmed_offset()?;
    let preflight =
        phase0_verified_projection_v5(session, initial, basis, confirmed_offset, limits)?;
    record_v5_materialization_start();
    let counts = preflight.array_counts;
    let mut materialized = ReplayProjectionRowsV5::with_capacities(&counts)?;
    let mut materialization_error = None;
    session.visit_projection(&mut |metadata, payload| {
        if materialization_error.is_none()
            && let Err(error) = materialized.observe(metadata, payload)
        {
            materialization_error = Some(error);
        }
    })?;
    if let Some(error) = materialization_error {
        return Err(error);
    }
    materialized.validate(&counts, confirmed_offset, event_count)?;
    let ReplayProjectionRowsV5 {
        confirmed_offset: _,
        events,
        obligation_lifecycle,
        executions,
        claims,
        artifact_registrations,
        snapshot_sources,
        context_envelopes,
        review_plans,
        evidence,
        evidence_bindings,
        verifications,
        decisions,
        findings,
        artifact_registrations_v4,
        gluing_input_descriptors,
        context_covers,
        sections,
        restrictions,
        gluing_attempts,
        global_candidates,
        gluing_obstructions,
    } = materialized;
    let marker = IndexMarkerV5 {
        index_schema_version: 5,
        sqlite_user_version: 5,
        projection_contract_version: PROJECTION_CONTRACT_VERSION_V5.to_owned(),
        event_contract_version: EVENT_CONTRACT_VERSION_V4.to_owned(),
        projection_mode: PROJECTION_MODE_V4.to_owned(),
        run_id: basis.run_id().clone(),
        genesis_hash: basis.genesis_hash().clone(),
        confirmed_offset,
        tail_hash: basis.confirmed_tail_hash().clone(),
        event_count,
        policy_revision_hash: basis.policy_revision_hash().clone(),
        authority_replay_basis_digest: basis.basis_digest().clone(),
    };
    let mut snapshot = IndexSnapshotV5 {
        marker,
        events,
        shadows: reserved_vec(counts[1])?,
        projected_findings: reserved_vec(counts[2])?,
        program_objects: reserved_vec(counts[PROGRAM_OBJECTS])?,
        program_relations: reserved_vec(counts[PROGRAM_RELATIONS])?,
        universe: None,
        obligations: reserved_vec(counts[OBLIGATIONS])?,
        obligation_lifecycle,
        executions,
        claims,
        artifact_registrations,
        snapshot_sources,
        context_envelopes,
        review_plans,
        evidence,
        evidence_bindings,
        verifications,
        decisions,
        findings,
        claim_assessments: reserved_vec(counts[CLAIM_ASSESSMENTS])?,
        artifact_registrations_v4,
        gluing_input_descriptors,
        context_covers,
        sections,
        restrictions,
        gluing_attempts,
        global_candidates,
        gluing_obstructions,
        policy_revision_hash: basis.policy_revision_hash().clone(),
        authority_replay_basis_digest: basis.basis_digest().clone(),
    };
    for artifact in initial.program_space().artifacts() {
        snapshot.program_objects.push(IndexProgramObject {
            object_id: artifact.id.clone(),
            object_kind: artifact.kind.clone(),
            body_hash: super::body_hash(artifact)?,
        });
    }
    for relation in initial.program_space().relations() {
        snapshot.program_relations.push(IndexProgramRelation {
            relation_id: relation.id.clone(),
            relation_kind: relation.kind.clone(),
            source_id: relation.source_id.clone(),
            target_ids_canonical_json: super::canonical_ids(relation.target_ids.iter().cloned())?,
            body_hash: super::body_hash(relation)?,
        });
    }
    let universe = initial.universe();
    snapshot.universe = Some(IndexUniverse {
        universe_id: universe.id().clone(),
        snapshot_id: universe.snapshot_id().clone(),
        profile_id: universe.profile_id().to_owned(),
        rule_set_hash: universe.rule_set_hash().clone(),
        extractor_set_hash: universe.extractor_set_hash().clone(),
        policy_version: universe.policy_version().to_owned(),
        rule_pack_version: universe.rule_pack_version().to_owned(),
        body_hash: super::body_hash(universe)?,
    });
    for obligation in initial.obligations() {
        record_projection_obligation_probe();
        let lifecycle = session.obligation_lifecycle(obligation.id())?;
        snapshot.obligations.push(IndexObligation {
            obligation_id: obligation.id().clone(),
            target_kind: obligation.target_kind().to_owned(),
            target_ids_canonical_json: super::canonical_ids(
                obligation.normalized_target_refs().iter().cloned(),
            )?,
            property_id: obligation.property_id().to_owned(),
            lifecycle: super::serialized_enum(&lifecycle)?,
            body_hash: obligation_body_hash_at_lifecycle(obligation, &lifecycle)?,
        });
    }
    let claim_ids = snapshot
        .claims
        .iter()
        .map(|claim| claim.claim_id.clone())
        .collect::<BTreeSet<_>>();
    let mut assessment_error = None;
    session.for_each_claim_assessment(&mut |assessment| {
        if assessment_error.is_none() && !claim_ids.contains(assessment.claim_id()) {
            assessment_error = Some(IndexError::ProjectionContractViolation);
            return;
        }
        if assessment_error.is_none() {
            match project_claim_assessment_v4(assessment, event_count) {
                Ok(row) => snapshot.claim_assessments.push(row),
                Err(error) => assessment_error = Some(error),
            }
        }
    })?;
    if let Some(error) = assessment_error {
        return Err(error);
    }

    #[cfg(any())]
    {
        let mut lifecycles = initial
            .obligations()
            .iter()
            .map(|obligation| (obligation.id().clone(), obligation.lifecycle()))
            .collect::<BTreeMap<_, _>>();
        for envelope in &envelopes[1..inherited_len] {
            let decoded = envelope
                .decode_for_streaming_projection()
                .map_err(|_| IndexError::ProjectionContractViolation)?;
            if let DecodedPayload::ObligationTransition {
                obligation_id,
                next,
            } = decoded.payload()
            {
                let Some(lifecycle) = lifecycles.get_mut(obligation_id) else {
                    return Err(IndexError::ProjectionContractViolation);
                };
                *lifecycle = *next;
            }
        }
        for obligation in initial.obligations() {
            record_projection_obligation_probe();
            let lifecycle = lifecycles
                .get(obligation.id())
                .ok_or(IndexError::ProjectionContractViolation)?;
            snapshot.obligations.push(IndexObligation {
                obligation_id: obligation.id().clone(),
                target_kind: obligation.target_kind().to_owned(),
                target_ids_canonical_json: super::canonical_ids(
                    obligation.normalized_target_refs().iter().cloned(),
                )?,
                property_id: obligation.property_id().to_owned(),
                lifecycle: super::serialized_enum(lifecycle)?,
                body_hash: obligation_body_hash_at_lifecycle(obligation, lifecycle)?,
            });
        }
        let genesis_envelope = &envelopes[0];
        snapshot.events.push(IndexEvent {
            sequence: genesis_envelope.sequence(),
            event_id: genesis_envelope.id().clone(),
            schema: EVENT_CONTRACT_VERSION_V4.to_owned(),
            event_hash: genesis_envelope.event_hash().clone(),
            payload_hash: genesis_envelope.payload_hash().clone(),
            payload_kind: "run_genesis_manifest".to_owned(),
            actor: genesis_envelope.actor().to_owned(),
            logical_time: genesis_envelope.logical_time(),
        });
        snapshot
            .artifact_registrations
            .push(project_registration_opaque_v3(
                genesis_envelope,
                &session.genesis_artifact()?,
            )?);
        for envelope in &envelopes[1..inherited_len] {
            record_projection_decode();
            let decoded = envelope
                .decode_for_streaming_projection()
                .map_err(|_| IndexError::ProjectionContractViolation)?;
            let kind = payload_kind_v5(decoded.payload())?;
            snapshot.events.push(IndexEvent {
                sequence: envelope.sequence(),
                event_id: envelope.id().clone(),
                schema: EVENT_CONTRACT_VERSION_V4.to_owned(),
                event_hash: envelope.event_hash().clone(),
                payload_hash: envelope.payload_hash().clone(),
                payload_kind: kind.to_owned(),
                actor: envelope.actor().to_owned(),
                logical_time: envelope.logical_time(),
            });
            match decoded.payload() {
                DecodedPayload::ObligationTransition {
                    obligation_id,
                    next,
                } => {
                    snapshot
                        .obligation_lifecycle
                        .push(IndexObligationLifecycle {
                            event_sequence: envelope.sequence(),
                            event_id: envelope.id().clone(),
                            obligation_id: obligation_id.clone(),
                            next_lifecycle: super::serialized_enum(next)?,
                        });
                }
                DecodedPayload::ArtifactRegisteredV3(value) => snapshot
                    .artifact_registrations
                    .push(project_registration(envelope, value)?),
                DecodedPayload::SnapshotSourcesRecorded(value) => {
                    for entry in value.entries() {
                        snapshot.snapshot_sources.push(IndexSnapshotSource {
                            event_sequence: envelope.sequence(),
                            event_id: envelope.id().clone(),
                            snapshot_id: value.snapshot_id().clone(),
                            artifact_id: entry.artifact_id().clone(),
                            registration_id: entry.registration_id().clone(),
                            path: entry.path().to_owned(),
                            content_hash: entry.content_hash().clone(),
                            cas_hash: entry.cas_hash().clone(),
                            line_count: entry.line_count(),
                        });
                    }
                }
                DecodedPayload::ReviewPlanRecorded(value) => snapshot
                    .review_plans
                    .push(super::projected_review_plan(envelope, value)?),
                DecodedPayload::ContextEnvelopeProjected(value) => snapshot
                    .context_envelopes
                    .push(super::projected_context_envelope(envelope, value)?),
                DecodedPayload::ReviewExecutionRecorded { execution, claims } => {
                    snapshot
                        .executions
                        .push(project_execution(envelope, execution)?);
                    for claim in claims {
                        snapshot.claims.push(project_claim(envelope, claim)?);
                    }
                }
                DecodedPayload::EvidenceRecordedV3(value) => {
                    snapshot.evidence.push(project_evidence(envelope, value)?)
                }
                DecodedPayload::EvidenceBoundV3(value) => {
                    snapshot
                        .evidence_bindings
                        .push(project_binding(envelope, value)?);
                }
                DecodedPayload::VerificationRecordedV3(value) => {
                    snapshot
                        .verifications
                        .push(project_verification(envelope, value)?);
                }
                DecodedPayload::DecisionRecordedV3(value) => {
                    snapshot.decisions.push(project_decision(envelope, value)?);
                }
                DecodedPayload::FindingRecordedV3(value) => {
                    snapshot.findings.push(project_finding(envelope, value)?);
                }
                DecodedPayload::RunGenesisManifestV3(value) => {
                    snapshot
                        .artifact_registrations
                        .push(project_registration(envelope, value.genesis_artifact())?);
                }
                _ => return Err(IndexError::ProjectionContractViolation),
            }
        }
        for (offset, pair) in [payment, ui].into_iter().flatten().enumerate() {
            let envelope = envelopes
                .get(inherited_len + offset)
                .ok_or(IndexError::ProjectionContractViolation)?;
            let (registration, descriptor) = pair;
            snapshot.events.push(IndexEvent {
                sequence: envelope.sequence(),
                event_id: envelope.id().clone(),
                schema: EVENT_CONTRACT_VERSION_V4.to_owned(),
                event_hash: envelope.event_hash().clone(),
                payload_hash: envelope.payload_hash().clone(),
                payload_kind: "artifact_registered_v4".to_owned(),
                actor: envelope.actor().to_owned(),
                logical_time: envelope.logical_time(),
            });
            let registration_projection = BorrowedRegistrationProjectionV4(&registration);
            let registration_hash = projection_hash(&registration_projection)?;
            let source = BorrowedCanonicalArtifactSourceV4(registration.source());
            let source_value = serde_json::to_value(&source)
                .map_err(|_| IndexError::ProjectionContractViolation)?;
            let descriptor_projection = BorrowedDescriptorProjectionV4(&descriptor);
            let (descriptor_value, descriptor_hash) =
                projection_value_and_hash(&descriptor_projection)?;
            snapshot
                .artifact_registrations_v4
                .push(ArtifactRegistrationV4IndexItem {
                    event_sequence: envelope.sequence(),
                    event_id: envelope.id().clone(),
                    event_actor: envelope.actor().to_owned(),
                    registration_id: registration.registration_id().clone(),
                    schema: "reviewgraphen.artifact_registration.v4".to_owned(),
                    run_id: registration.run_id().clone(),
                    cas_hash: registration.cas_hash().clone(),
                    media_type: registration.media_type().to_owned(),
                    size: registration.size(),
                    sensitivity: super::serialized_enum(&registration.sensitivity())?,
                    source_kind: registration.source().kind().to_owned(),
                    source: source_value,
                    descriptor_id: descriptor.id().clone(),
                    body_hash: registration_hash,
                });
            snapshot
                .gluing_input_descriptors
                .push(GluingInputDescriptorV4IndexItem {
                    event_sequence: envelope.sequence(),
                    event_id: envelope.id().clone(),
                    descriptor: descriptor_value,
                    registration_id: registration.registration_id().clone(),
                    descriptor_hash: registration.cas_hash().clone(),
                    descriptor_size: registration.size(),
                    body_hash: descriptor_hash,
                });
        }
        if let Some(bundle) = bundle {
            let envelope = envelopes
                .get(inherited_len + registration_count)
                .ok_or(IndexError::ProjectionContractViolation)?;
            snapshot.events.push(IndexEvent {
                sequence: envelope.sequence(),
                event_id: envelope.id().clone(),
                schema: EVENT_CONTRACT_VERSION_V4.to_owned(),
                event_hash: envelope.event_hash().clone(),
                payload_hash: envelope.payload_hash().clone(),
                payload_kind: "gluing_bundle_recorded_v4".to_owned(),
                actor: envelope.actor().to_owned(),
                logical_time: envelope.logical_time(),
            });
            let sequence = envelope.sequence();
            let event_id = envelope.id().clone();
            let cover = bundle.cover();
            let cover_projection = BorrowedCoverProjectionV4(&cover);
            let (cover_value, cover_hash) = projection_value_and_hash(&cover_projection)?;
            snapshot.context_covers.push(ContextCoverV4IndexItem {
                event_sequence: sequence,
                event_id: event_id.clone(),
                cover: cover_value,
                body_hash: cover_hash,
            });
            for section in bundle.sections() {
                let section_projection = BorrowedSectionProjectionValueV4(&section);
                let (section_value, section_hash) = projection_value_and_hash(&section_projection)?;
                snapshot.sections.push(SectionV4IndexItem {
                    event_sequence: sequence,
                    event_id: event_id.clone(),
                    section: section_value,
                    body_hash: section_hash,
                });
            }
            let attempt = bundle.attempt();
            let attempt_id = attempt.id().clone();
            for restriction in bundle.restrictions() {
                let restriction_projection = BorrowedRestrictionProjectionValueV4(&restriction);
                let (restriction_value, restriction_hash) =
                    projection_value_and_hash(&restriction_projection)?;
                snapshot.restrictions.push(RestrictionV4IndexItem {
                    event_sequence: sequence,
                    event_id: event_id.clone(),
                    attempt_id: attempt_id.clone(),
                    restriction: restriction_value,
                    body_hash: restriction_hash,
                });
            }
            let attempt_projection = BorrowedAttemptProjectionV4(&attempt);
            let (attempt_value, attempt_hash) = projection_value_and_hash(&attempt_projection)?;
            snapshot.gluing_attempts.push(GluingAttemptV4IndexItem {
                event_sequence: sequence,
                event_id: event_id.clone(),
                attempt: attempt_value,
                body_hash: attempt_hash,
            });
            if let Some(candidate) = bundle.global_candidate() {
                let candidate_projection = BorrowedCandidateProjectionV4(&candidate);
                let (candidate_value, candidate_hash) =
                    projection_value_and_hash(&candidate_projection)?;
                snapshot.global_candidates.push(GlobalCandidateV4IndexItem {
                    event_sequence: sequence,
                    event_id: event_id.clone(),
                    attempt_id: attempt_id.clone(),
                    candidate: candidate_value,
                    body_hash: candidate_hash,
                });
            }
            if let Some(obstruction) = bundle.obstruction() {
                let obstruction_projection = BorrowedObstructionProjectionV4(&obstruction);
                let (obstruction_value, obstruction_hash) =
                    projection_value_and_hash(&obstruction_projection)?;
                snapshot
                    .gluing_obstructions
                    .push(GluingObstructionV4IndexItem {
                        event_sequence: sequence,
                        event_id,
                        obstruction: obstruction_value,
                        body_hash: obstruction_hash,
                    });
            }
        }
        let claim_ids = snapshot
            .claims
            .iter()
            .map(|claim| claim.claim_id.clone())
            .collect::<Vec<_>>();
        for claim_id in claim_ids {
            let Some(assessment) = session.claim_assessment(&claim_id)? else {
                continue;
            };
            if assessment.claim_id() != &claim_id {
                return Err(IndexError::ProjectionContractViolation);
            }
            snapshot
                .claim_assessments
                .push(project_claim_assessment_v4(assessment, event_count)?);
        }
    }
    snapshot
        .program_objects
        .sort_by(|a, b| a.object_id.cmp(&b.object_id));
    snapshot
        .program_relations
        .sort_by(|a, b| a.relation_id.cmp(&b.relation_id));
    snapshot
        .obligations
        .sort_by(|a, b| a.obligation_id.cmp(&b.obligation_id));
    snapshot.snapshot_sources.sort_by(|a, b| {
        (a.event_sequence, &a.artifact_id).cmp(&(b.event_sequence, &b.artifact_id))
    });
    snapshot
        .claim_assessments
        .sort_by(|a, b| a.claim_id.cmp(&b.claim_id));
    if snapshot.events.len()
        != usize::try_from(event_count).map_err(|_| IndexError::IntegerOutOfRange)?
    {
        return Err(IndexError::ProjectionContractViolation);
    }
    record_v5_full_snapshot_construction();
    Ok(ProjectedV5 {
        snapshot,
        accounting: preflight.accounting,
    })
}

#[cfg(test)]
pub(crate) fn v5_preflight_accounting_limits_for_test(
    session: &crate::ReplayedV4RunSession<'_, '_>,
    basis: &AuthorityReplayBasisV4,
    max_rows: u64,
    max_query_bytes: u64,
    max_working_bytes: u64,
) -> Result<(IndexAccountingV5, IndexAccountingV5), IndexError> {
    let limits = IndexLimits {
        max_rows,
        max_serialized_bytes: u64::MAX,
        max_working_bytes,
        max_query_bytes,
        max_statement_bytes: u64::MAX,
    };
    let projected = project_verified_source(session, basis, limits)?;
    let max_event_line_bytes = session
        .index_v5_replay_projection()?
        .charge
        .max_event_line_bytes;
    let oracle = account_snapshot(&projected.snapshot, max_event_line_bytes, limits)?;
    Ok((projected.accounting, oracle))
}

#[cfg(test)]
pub(crate) fn v5_snapshot_for_test(
    session: &crate::ReplayedV4RunSession<'_, '_>,
    basis: &AuthorityReplayBasisV4,
) -> Result<IndexSnapshotV5, IndexError> {
    let limits = IndexLimits {
        max_rows: u64::MAX,
        max_serialized_bytes: u64::MAX,
        max_working_bytes: u64::MAX,
        max_query_bytes: u64::MAX,
        max_statement_bytes: u64::MAX,
    };
    Ok(project_verified_source(session, basis, limits)?.snapshot)
}

#[allow(dead_code)]
const V5_ARRAY_FIELDS: [&str; 27] = [
    "events",
    "shadows",
    "projected_findings",
    "program_objects",
    "program_relations",
    "obligations",
    "obligation_lifecycle",
    "executions",
    "claims",
    "artifact_registrations",
    "snapshot_sources",
    "context_envelopes",
    "review_plans",
    "evidence",
    "evidence_bindings",
    "verifications",
    "decisions",
    "findings",
    "claim_assessments",
    "artifact_registrations_v4",
    "gluing_input_descriptors",
    "context_covers",
    "sections",
    "restrictions",
    "gluing_attempts",
    "global_candidates",
    "gluing_obstructions",
];
#[allow(dead_code)]
const EVENTS: usize = 0;
#[allow(dead_code)]
const PROGRAM_OBJECTS: usize = 3;
#[allow(dead_code)]
const PROGRAM_RELATIONS: usize = 4;
#[allow(dead_code)]
const OBLIGATIONS: usize = 5;
#[allow(dead_code)]
const OBLIGATION_LIFECYCLE: usize = 6;
#[allow(dead_code)]
const EXECUTIONS: usize = 7;
#[allow(dead_code)]
const CLAIMS: usize = 8;
#[allow(dead_code)]
const REGISTRATIONS: usize = 9;
#[allow(dead_code)]
const SNAPSHOT_SOURCES: usize = 10;
#[allow(dead_code)]
const CONTEXT_ENVELOPES: usize = 11;
#[allow(dead_code)]
const REVIEW_PLANS: usize = 12;
#[allow(dead_code)]
const EVIDENCE: usize = 13;
#[allow(dead_code)]
const EVIDENCE_BINDINGS: usize = 14;
#[allow(dead_code)]
const VERIFICATIONS: usize = 15;
#[allow(dead_code)]
const DECISIONS: usize = 16;
#[allow(dead_code)]
const FINDINGS: usize = 17;
#[allow(dead_code)]
const CLAIM_ASSESSMENTS: usize = 18;
const REGISTRATIONS_V4: usize = 19;
const GLUING_DESCRIPTORS: usize = 20;
const CONTEXT_COVERS: usize = 21;
const SECTIONS: usize = 22;
const RESTRICTIONS: usize = 23;
const GLUING_ATTEMPTS: usize = 24;
const GLOBAL_CANDIDATES: usize = 25;
const GLUING_OBSTRUCTIONS: usize = 26;

#[allow(dead_code)]
#[derive(Clone, Copy, Default)]
struct ArrayChargeV5 {
    items: u64,
    json_items: u64,
    owned_items: u64,
}

#[allow(dead_code)]
struct ProjectionChargeV5 {
    arrays: [ArrayChargeV5; V5_ARRAY_FIELDS.len()],
    rows: u64,
    integer_cells: u64,
    text_bytes: u64,
    marker_json: u64,
    marker_owned: u64,
    universe_json: u64,
    universe_owned: u64,
    max_cas_bytes: u64,
    max_event_line_bytes: u64,
    admitted_json_bytes: u64,
    allocation_limit: u64,
}

#[allow(dead_code)]
struct ProjectionPreflightV5 {
    accounting: IndexAccountingV5,
    array_counts: [u64; V5_ARRAY_FIELDS.len()],
}

#[allow(dead_code)]
fn reserved_vec<T>(count: u64) -> Result<Vec<T>, IndexError> {
    let count = usize::try_from(count).map_err(|_| IndexError::IntegerOutOfRange)?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| IndexError::IntegerOutOfRange)?;
    Ok(result)
}

#[allow(dead_code)]
impl ProjectionChargeV5 {
    fn new(marker: &IndexMarkerV5, limits: IndexLimits) -> Result<Self, IndexError> {
        Self::new_from_marker(marker, limits)
    }

    fn new_borrowed(
        marker: &BorrowedIndexMarkerV5<'_>,
        limits: IndexLimits,
    ) -> Result<Self, IndexError> {
        Self::new_from_marker(marker, limits)
    }

    fn new_from_marker<T: Serialize>(marker: &T, limits: IndexLimits) -> Result<Self, IndexError> {
        let marker_json = json_length(marker)?;
        let marker_owned = recursive_ownership_charge(marker)?;
        if marker_json > limits.max_query_bytes {
            return Err(IndexError::Incomplete {
                limit: limits.max_query_bytes,
                observed: marker_json,
            });
        }
        let mut result = Self {
            arrays: [ArrayChargeV5::default(); V5_ARRAY_FIELDS.len()],
            rows: 0,
            integer_cells: 0,
            text_bytes: 0,
            marker_json,
            marker_owned,
            universe_json: 4, // null until the mandatory universe row is observed
            universe_owned: 0,
            max_cas_bytes: 0,
            max_event_line_bytes: 0,
            admitted_json_bytes: marker_json,
            allocation_limit: limits.max_query_bytes,
        };
        result.add_sql(marker, None)?;
        Ok(result)
    }

    fn add_sql<T: Serialize>(&mut self, row: &T, skip: Option<&str>) -> Result<(), IndexError> {
        self.rows = self
            .rows
            .checked_add(1)
            .ok_or(IndexError::IntegerOutOfRange)?;
        charge_sql_row(row, &mut self.integer_cells, &mut self.text_bytes, skip)
    }

    fn add_array<T: Serialize>(
        &mut self,
        field: usize,
        row: &T,
        skip_sql: Option<&str>,
    ) -> Result<(), IndexError> {
        let json = json_length(row)?;
        let owned = recursive_ownership_charge(row)?;
        self.admit_json_allocation(json)?;
        let array = &mut self.arrays[field];
        array.items = array
            .items
            .checked_add(1)
            .ok_or(IndexError::IntegerOutOfRange)?;
        array.json_items = array
            .json_items
            .checked_add(json)
            .ok_or(IndexError::IntegerOutOfRange)?;
        array.owned_items = array
            .owned_items
            .checked_add(owned)
            .and_then(|value| value.checked_add(8))
            .ok_or(IndexError::IntegerOutOfRange)?;
        self.add_sql(row, skip_sql)
    }

    fn add_m5_array<T: Serialize>(
        &mut self,
        index: usize,
        row: &T,
        sql_charge: impl FnOnce(&mut u64, &mut u64) -> Result<(), IndexError>,
    ) -> Result<(), IndexError> {
        let json = json_length(row)?;
        self.admit_json_allocation(json)?;
        let owned = recursive_ownership_charge(row)?;
        let array = self
            .arrays
            .get_mut(index)
            .ok_or(IndexError::ProjectionContractViolation)?;
        array.items = array
            .items
            .checked_add(1)
            .ok_or(IndexError::IntegerOutOfRange)?;
        array.json_items = array
            .json_items
            .checked_add(json)
            .ok_or(IndexError::IntegerOutOfRange)?;
        array.owned_items = array
            .owned_items
            .checked_add(owned)
            .ok_or(IndexError::IntegerOutOfRange)?;
        array.owned_items = array
            .owned_items
            .checked_add(8)
            .ok_or(IndexError::IntegerOutOfRange)?;
        self.rows = self
            .rows
            .checked_add(1)
            .ok_or(IndexError::IntegerOutOfRange)?;
        sql_charge(&mut self.integer_cells, &mut self.text_bytes)
    }

    /// Adds a row measured by the allocation-free borrowed phase.  The
    /// materializing preflight is intentionally not involved here: admission
    /// is complete before any projected DTO or canonical JSON string exists.
    fn add_precomputed_array(
        &mut self,
        index: usize,
        row: BorrowedRowCharge,
    ) -> Result<(), IndexError> {
        self.admit_json_allocation(row.json_bytes)?;
        let array = self
            .arrays
            .get_mut(index)
            .ok_or(IndexError::ProjectionContractViolation)?;
        array.items = array
            .items
            .checked_add(1)
            .ok_or(IndexError::IntegerOutOfRange)?;
        array.json_items = array
            .json_items
            .checked_add(row.json_bytes)
            .ok_or(IndexError::IntegerOutOfRange)?;
        array.owned_items = array
            .owned_items
            .checked_add(row.owned_bytes)
            .and_then(|value| value.checked_add(8))
            .ok_or(IndexError::IntegerOutOfRange)?;
        self.rows = self
            .rows
            .checked_add(1)
            .ok_or(IndexError::IntegerOutOfRange)?;
        self.integer_cells = self
            .integer_cells
            .checked_add(row.integer_cells)
            .ok_or(IndexError::IntegerOutOfRange)?;
        self.text_bytes = self
            .text_bytes
            .checked_add(row.text_bytes)
            .ok_or(IndexError::IntegerOutOfRange)?;
        Ok(())
    }

    fn merge_replay_payloads(&mut self, replay: ReplayPayloadChargeV5) -> Result<(), IndexError> {
        for (target, source) in self.arrays.iter_mut().zip(replay.arrays) {
            target.items = target
                .items
                .checked_add(source.items)
                .ok_or(IndexError::IntegerOutOfRange)?;
            target.json_items = target
                .json_items
                .checked_add(source.json_items)
                .ok_or(IndexError::IntegerOutOfRange)?;
            target.owned_items = target
                .owned_items
                .checked_add(source.owned_items)
                .ok_or(IndexError::IntegerOutOfRange)?;
        }
        let replay_json = replay.arrays.into_iter().try_fold(0_u64, |total, item| {
            total
                .checked_add(item.json_items)
                .ok_or(IndexError::IntegerOutOfRange)
        })?;
        self.admit_json_allocation(replay_json)?;
        self.rows = self
            .rows
            .checked_add(replay.rows)
            .ok_or(IndexError::IntegerOutOfRange)?;
        self.integer_cells = self
            .integer_cells
            .checked_add(replay.integer_cells)
            .ok_or(IndexError::IntegerOutOfRange)?;
        self.text_bytes = self
            .text_bytes
            .checked_add(replay.text_bytes)
            .ok_or(IndexError::IntegerOutOfRange)?;
        self.max_cas_bytes = self.max_cas_bytes.max(replay.max_cas_bytes);
        self.max_event_line_bytes = self.max_event_line_bytes.max(replay.max_event_line_bytes);
        Ok(())
    }

    fn set_universe(&mut self, row: &IndexUniverse) -> Result<(), IndexError> {
        self.universe_json = json_length(row)?;
        self.universe_owned = recursive_ownership_charge(row)?;
        self.admit_json_allocation(self.universe_json)?;
        self.add_sql(row, None)?;
        // SQLite's singleton key is physical but is not returned as a field.
        self.integer_cells = self
            .integer_cells
            .checked_add(1)
            .ok_or(IndexError::IntegerOutOfRange)?;
        Ok(())
    }

    fn set_universe_precomputed(&mut self, row: BorrowedRowCharge) -> Result<(), IndexError> {
        self.universe_json = row.json_bytes;
        self.universe_owned = row.owned_bytes;
        self.admit_json_allocation(row.json_bytes)?;
        self.rows = self
            .rows
            .checked_add(1)
            .ok_or(IndexError::IntegerOutOfRange)?;
        self.integer_cells = self
            .integer_cells
            .checked_add(row.integer_cells)
            .and_then(|value| value.checked_add(1))
            .ok_or(IndexError::IntegerOutOfRange)?;
        self.text_bytes = self
            .text_bytes
            .checked_add(row.text_bytes)
            .ok_or(IndexError::IntegerOutOfRange)?;
        Ok(())
    }

    fn admit_json_allocation(&mut self, bytes: u64) -> Result<(), IndexError> {
        self.admitted_json_bytes = self
            .admitted_json_bytes
            .checked_add(bytes)
            .ok_or(IndexError::IntegerOutOfRange)?;
        if self.admitted_json_bytes > self.allocation_limit {
            return Err(IndexError::Incomplete {
                limit: self.allocation_limit,
                observed: self.admitted_json_bytes,
            });
        }
        Ok(())
    }

    fn admit_transient_json(&self, bytes: u64) -> Result<(), IndexError> {
        if bytes > self.allocation_limit {
            return Err(IndexError::Incomplete {
                limit: self.allocation_limit,
                observed: bytes,
            });
        }
        Ok(())
    }

    fn finish(
        self,
        marker: &IndexMarkerV5,
        expected_rows: u64,
        limits: IndexLimits,
    ) -> Result<ProjectionPreflightV5, IndexError> {
        self.finish_with_marker_hashes(
            &marker.policy_revision_hash,
            &marker.authority_replay_basis_digest,
            expected_rows,
            limits,
        )
    }

    fn finish_borrowed(
        self,
        marker: &BorrowedIndexMarkerV5<'_>,
        expected_rows: u64,
        limits: IndexLimits,
    ) -> Result<ProjectionPreflightV5, IndexError> {
        self.finish_with_marker_hashes(
            marker.policy_revision_hash,
            marker.authority_replay_basis_digest,
            expected_rows,
            limits,
        )
    }

    fn finish_with_marker_hashes(
        self,
        policy_revision_hash: &ContentHash,
        authority_replay_basis_digest: &ContentHash,
        expected_rows: u64,
        limits: IndexLimits,
    ) -> Result<ProjectionPreflightV5, IndexError> {
        if self.rows != expected_rows {
            return Err(IndexError::ProjectionContractViolation);
        }
        if self.rows > limits.max_rows {
            return Err(IndexError::Incomplete {
                limit: limits.max_rows,
                observed: self.rows,
            });
        }
        let sql_bytes = self
            .text_bytes
            .checked_add(
                self.integer_cells
                    .checked_mul(8)
                    .ok_or(IndexError::Incomplete {
                        limit: limits.max_working_bytes,
                        observed: u64::MAX,
                    })?,
            )
            .and_then(|value| value.checked_add(self.rows))
            .ok_or(IndexError::Incomplete {
                limit: limits.max_working_bytes,
                observed: u64::MAX,
            })?;

        let policy = json_length(policy_revision_hash)?;
        let basis = json_length(authority_replay_basis_digest)?;
        let mut query_values = self
            .marker_json
            .checked_add(self.universe_json)
            .and_then(|value| value.checked_add(policy))
            .and_then(|value| value.checked_add(basis))
            .ok_or(IndexError::Incomplete {
                limit: limits.max_query_bytes,
                observed: u64::MAX,
            })?;
        for array in self.arrays {
            let separators = array.items.saturating_sub(1);
            query_values = query_values
                .checked_add(2)
                .and_then(|value| value.checked_add(separators))
                .and_then(|value| value.checked_add(array.json_items))
                .ok_or(IndexError::Incomplete {
                    limit: limits.max_query_bytes,
                    observed: u64::MAX,
                })?;
        }
        const SCALAR_FIELDS: [&str; 4] = [
            "marker",
            "universe",
            "policy_revision_hash",
            "authority_replay_basis_digest",
        ];
        let field_count = u64::try_from(V5_ARRAY_FIELDS.len() + SCALAR_FIELDS.len())
            .map_err(|_| IndexError::IntegerOutOfRange)?;
        let mut key_bytes = 0_u64;
        for key in V5_ARRAY_FIELDS.iter().chain(SCALAR_FIELDS.iter()) {
            key_bytes = key_bytes
                .checked_add(u64::try_from(key.len()).map_err(|_| IndexError::IntegerOutOfRange)?)
                .ok_or(IndexError::IntegerOutOfRange)?;
        }
        // Every field contributes two quotes plus one colon. The surrounding
        // object contributes two braces and `field_count - 1` commas.
        let query_bytes = query_values
            .checked_add(key_bytes)
            .and_then(|value| value.checked_add(field_count.checked_mul(3)?))
            .and_then(|value| value.checked_add(field_count.saturating_sub(1)))
            .and_then(|value| value.checked_add(2))
            .ok_or(IndexError::Incomplete {
                limit: limits.max_query_bytes,
                observed: u64::MAX,
            })?;
        if query_bytes > limits.max_query_bytes {
            return Err(IndexError::Incomplete {
                limit: limits.max_query_bytes,
                observed: query_bytes,
            });
        }
        // ADR-0021 ownership is a semantic charge over the decoded snapshot,
        // not an allocator estimate or a multiple of its canonical encoding.
        let top_level_entries = field_count.checked_mul(16).ok_or(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: u64::MAX,
        })?;
        let mut owned_bytes = top_level_entries
            .checked_add(key_bytes)
            .and_then(|value| value.checked_add(self.marker_owned))
            .and_then(|value| value.checked_add(self.universe_owned))
            .ok_or(IndexError::Incomplete {
                limit: limits.max_working_bytes,
                observed: u64::MAX,
            })?;
        for length in [
            policy_revision_hash.as_str().len(),
            authority_replay_basis_digest.as_str().len(),
        ] {
            owned_bytes = owned_bytes
                .checked_add(u64::try_from(length).map_err(|_| IndexError::IntegerOutOfRange)?)
                .ok_or(IndexError::Incomplete {
                    limit: limits.max_working_bytes,
                    observed: u64::MAX,
                })?;
        }
        for array in self.arrays {
            owned_bytes =
                owned_bytes
                    .checked_add(array.owned_items)
                    .ok_or(IndexError::Incomplete {
                        limit: limits.max_working_bytes,
                        observed: u64::MAX,
                    })?;
        }
        let working_bytes = sql_bytes
            .checked_add(query_bytes)
            .and_then(|value| value.checked_add(owned_bytes))
            .and_then(|value| value.checked_add(self.max_cas_bytes))
            .and_then(|value| value.checked_add(self.max_event_line_bytes))
            .ok_or(IndexError::Incomplete {
                limit: limits.max_working_bytes,
                observed: u64::MAX,
            })?;
        if working_bytes > limits.max_working_bytes {
            return Err(IndexError::Incomplete {
                limit: limits.max_working_bytes,
                observed: working_bytes,
            });
        }
        let array_counts = self.arrays.map(|array| array.items);
        Ok(ProjectionPreflightV5 {
            accounting: IndexAccountingV5 {
                rows: self.rows,
                integer_cells: self.integer_cells,
                text_bytes: self.text_bytes,
                sql_bytes,
                query_bytes,
                owned_bytes,
                observed_object_bytes: self.max_cas_bytes,
                observed_event_line_bytes: self.max_event_line_bytes,
                working_bytes,
            },
            array_counts,
        })
    }
}

fn recursive_ownership_charge<T: ?Sized + Serialize>(value: &T) -> Result<u64, IndexError> {
    value
        .serialize(OwnershipSerializer)
        .map_err(|_| IndexError::IntegerOutOfRange)
}

#[derive(Debug)]
struct OwnershipError;

impl std::fmt::Display for OwnershipError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("recursive ownership overflow")
    }
}

impl std::error::Error for OwnershipError {}

impl ser::Error for OwnershipError {
    fn custom<T: std::fmt::Display>(_message: T) -> Self {
        Self
    }
}

struct OwnershipSerializer;

enum OwnershipCompoundKind {
    Sequence,
    Object,
    VariantObject { outer_key_bytes: u64 },
    VariantSequence { outer_key_bytes: u64 },
}

struct OwnershipCompound {
    total: u64,
    kind: OwnershipCompoundKind,
    pending_key: Option<u64>,
}

impl OwnershipCompound {
    fn sequence() -> Self {
        Self {
            total: 0,
            kind: OwnershipCompoundKind::Sequence,
            pending_key: None,
        }
    }

    fn object() -> Self {
        Self {
            total: 0,
            kind: OwnershipCompoundKind::Object,
            pending_key: None,
        }
    }

    fn add(&mut self, value: u64) -> Result<(), OwnershipError> {
        self.total = self.total.checked_add(value).ok_or(OwnershipError)?;
        Ok(())
    }

    fn add_sequence_value<T: ?Sized + Serialize>(
        &mut self,
        value: &T,
    ) -> Result<(), OwnershipError> {
        self.add(8)?;
        self.add(value.serialize(OwnershipSerializer)?)
    }

    fn add_object_value<T: ?Sized + Serialize>(
        &mut self,
        key_bytes: u64,
        value: &T,
    ) -> Result<(), OwnershipError> {
        self.add(16)?;
        self.add(key_bytes)?;
        self.add(value.serialize(OwnershipSerializer)?)
    }

    fn finish(self) -> Result<u64, OwnershipError> {
        match self.kind {
            OwnershipCompoundKind::Sequence | OwnershipCompoundKind::Object => Ok(self.total),
            OwnershipCompoundKind::VariantObject { outer_key_bytes }
            | OwnershipCompoundKind::VariantSequence { outer_key_bytes } => 16_u64
                .checked_add(outer_key_bytes)
                .and_then(|value| value.checked_add(self.total))
                .ok_or(OwnershipError),
        }
    }
}

impl Serializer for OwnershipSerializer {
    type Ok = u64;
    type Error = OwnershipError;
    type SerializeSeq = OwnershipCompound;
    type SerializeTuple = OwnershipCompound;
    type SerializeTupleStruct = OwnershipCompound;
    type SerializeTupleVariant = OwnershipCompound;
    type SerializeMap = OwnershipCompound;
    type SerializeStruct = OwnershipCompound;
    type SerializeStructVariant = OwnershipCompound;

    fn serialize_bool(self, _value: bool) -> Result<u64, OwnershipError> {
        Ok(1)
    }
    fn serialize_i8(self, _value: i8) -> Result<u64, OwnershipError> {
        Ok(8)
    }
    fn serialize_i16(self, _value: i16) -> Result<u64, OwnershipError> {
        Ok(8)
    }
    fn serialize_i32(self, _value: i32) -> Result<u64, OwnershipError> {
        Ok(8)
    }
    fn serialize_i64(self, _value: i64) -> Result<u64, OwnershipError> {
        Ok(8)
    }
    fn serialize_u8(self, _value: u8) -> Result<u64, OwnershipError> {
        Ok(8)
    }
    fn serialize_u16(self, _value: u16) -> Result<u64, OwnershipError> {
        Ok(8)
    }
    fn serialize_u32(self, _value: u32) -> Result<u64, OwnershipError> {
        Ok(8)
    }
    fn serialize_u64(self, _value: u64) -> Result<u64, OwnershipError> {
        Ok(8)
    }
    fn serialize_f32(self, _value: f32) -> Result<u64, OwnershipError> {
        Ok(8)
    }
    fn serialize_f64(self, _value: f64) -> Result<u64, OwnershipError> {
        Ok(8)
    }
    fn serialize_char(self, value: char) -> Result<u64, OwnershipError> {
        u64::try_from(value.len_utf8()).map_err(|_| OwnershipError)
    }
    fn serialize_str(self, value: &str) -> Result<u64, OwnershipError> {
        u64::try_from(value.len()).map_err(|_| OwnershipError)
    }
    fn serialize_bytes(self, value: &[u8]) -> Result<u64, OwnershipError> {
        let length = u64::try_from(value.len()).map_err(|_| OwnershipError)?;
        length.checked_mul(16).ok_or(OwnershipError)
    }
    fn serialize_none(self) -> Result<u64, OwnershipError> {
        Ok(0)
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<u64, OwnershipError> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<u64, OwnershipError> {
        Ok(0)
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<u64, OwnershipError> {
        Ok(0)
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<u64, OwnershipError> {
        u64::try_from(variant.len()).map_err(|_| OwnershipError)
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<u64, OwnershipError> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<u64, OwnershipError> {
        16_u64
            .checked_add(u64::try_from(variant.len()).map_err(|_| OwnershipError)?)
            .and_then(|total| {
                value
                    .serialize(OwnershipSerializer)
                    .ok()?
                    .checked_add(total)
            })
            .ok_or(OwnershipError)
    }
    fn serialize_seq(self, _length: Option<usize>) -> Result<Self::SerializeSeq, OwnershipError> {
        Ok(OwnershipCompound::sequence())
    }
    fn serialize_tuple(self, _length: usize) -> Result<Self::SerializeTuple, OwnershipError> {
        Ok(OwnershipCompound::sequence())
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeTupleStruct, OwnershipError> {
        Ok(OwnershipCompound::sequence())
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeTupleVariant, OwnershipError> {
        Ok(OwnershipCompound {
            total: 0,
            kind: OwnershipCompoundKind::VariantSequence {
                outer_key_bytes: u64::try_from(variant.len()).map_err(|_| OwnershipError)?,
            },
            pending_key: None,
        })
    }
    fn serialize_map(self, _length: Option<usize>) -> Result<Self::SerializeMap, OwnershipError> {
        Ok(OwnershipCompound::object())
    }
    fn serialize_struct(
        self,
        _name: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeStruct, OwnershipError> {
        Ok(OwnershipCompound::object())
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeStructVariant, OwnershipError> {
        Ok(OwnershipCompound {
            total: 0,
            kind: OwnershipCompoundKind::VariantObject {
                outer_key_bytes: u64::try_from(variant.len()).map_err(|_| OwnershipError)?,
            },
            pending_key: None,
        })
    }
}

impl SerializeSeq for OwnershipCompound {
    type Ok = u64;
    type Error = OwnershipError;
    fn serialize_element<T: ?Sized + Serialize>(
        &mut self,
        value: &T,
    ) -> Result<(), OwnershipError> {
        self.add_sequence_value(value)
    }
    fn end(self) -> Result<u64, OwnershipError> {
        self.finish()
    }
}
impl SerializeTuple for OwnershipCompound {
    type Ok = u64;
    type Error = OwnershipError;
    fn serialize_element<T: ?Sized + Serialize>(
        &mut self,
        value: &T,
    ) -> Result<(), OwnershipError> {
        self.add_sequence_value(value)
    }
    fn end(self) -> Result<u64, OwnershipError> {
        self.finish()
    }
}
impl SerializeTupleStruct for OwnershipCompound {
    type Ok = u64;
    type Error = OwnershipError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), OwnershipError> {
        self.add_sequence_value(value)
    }
    fn end(self) -> Result<u64, OwnershipError> {
        self.finish()
    }
}
impl SerializeTupleVariant for OwnershipCompound {
    type Ok = u64;
    type Error = OwnershipError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), OwnershipError> {
        self.add_sequence_value(value)
    }
    fn end(self) -> Result<u64, OwnershipError> {
        self.finish()
    }
}
impl SerializeStruct for OwnershipCompound {
    type Ok = u64;
    type Error = OwnershipError;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), OwnershipError> {
        self.add_object_value(u64::try_from(key.len()).map_err(|_| OwnershipError)?, value)
    }
    fn end(self) -> Result<u64, OwnershipError> {
        self.finish()
    }
}
impl SerializeStructVariant for OwnershipCompound {
    type Ok = u64;
    type Error = OwnershipError;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), OwnershipError> {
        self.add_object_value(u64::try_from(key.len()).map_err(|_| OwnershipError)?, value)
    }
    fn end(self) -> Result<u64, OwnershipError> {
        self.finish()
    }
}

struct OwnershipMapKeySerializer;
impl Serializer for OwnershipMapKeySerializer {
    type Ok = u64;
    type Error = OwnershipError;
    type SerializeSeq = ser::Impossible<u64, OwnershipError>;
    type SerializeTuple = ser::Impossible<u64, OwnershipError>;
    type SerializeTupleStruct = ser::Impossible<u64, OwnershipError>;
    type SerializeTupleVariant = ser::Impossible<u64, OwnershipError>;
    type SerializeMap = ser::Impossible<u64, OwnershipError>;
    type SerializeStruct = ser::Impossible<u64, OwnershipError>;
    type SerializeStructVariant = ser::Impossible<u64, OwnershipError>;
    fn serialize_str(self, value: &str) -> Result<u64, OwnershipError> {
        u64::try_from(value.len()).map_err(|_| OwnershipError)
    }
    fn serialize_char(self, value: char) -> Result<u64, OwnershipError> {
        u64::try_from(value.len_utf8()).map_err(|_| OwnershipError)
    }
    fn serialize_bool(self, value: bool) -> Result<u64, OwnershipError> {
        Ok(if value { 4 } else { 5 })
    }
    fn serialize_i8(self, value: i8) -> Result<u64, OwnershipError> {
        Ok(value.to_string().len() as u64)
    }
    fn serialize_i16(self, value: i16) -> Result<u64, OwnershipError> {
        Ok(value.to_string().len() as u64)
    }
    fn serialize_i32(self, value: i32) -> Result<u64, OwnershipError> {
        Ok(value.to_string().len() as u64)
    }
    fn serialize_i64(self, value: i64) -> Result<u64, OwnershipError> {
        Ok(value.to_string().len() as u64)
    }
    fn serialize_u8(self, value: u8) -> Result<u64, OwnershipError> {
        Ok(value.to_string().len() as u64)
    }
    fn serialize_u16(self, value: u16) -> Result<u64, OwnershipError> {
        Ok(value.to_string().len() as u64)
    }
    fn serialize_u32(self, value: u32) -> Result<u64, OwnershipError> {
        Ok(value.to_string().len() as u64)
    }
    fn serialize_u64(self, value: u64) -> Result<u64, OwnershipError> {
        Ok(value.to_string().len() as u64)
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<u64, OwnershipError> {
        Ok(variant.len() as u64)
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<u64, OwnershipError> {
        value.serialize(self)
    }
    fn serialize_f32(self, _value: f32) -> Result<u64, OwnershipError> {
        Err(OwnershipError)
    }
    fn serialize_f64(self, _value: f64) -> Result<u64, OwnershipError> {
        Err(OwnershipError)
    }
    fn serialize_bytes(self, _value: &[u8]) -> Result<u64, OwnershipError> {
        Err(OwnershipError)
    }
    fn serialize_none(self) -> Result<u64, OwnershipError> {
        Err(OwnershipError)
    }
    fn serialize_some<T: ?Sized + Serialize>(self, _value: &T) -> Result<u64, OwnershipError> {
        Err(OwnershipError)
    }
    fn serialize_unit(self) -> Result<u64, OwnershipError> {
        Err(OwnershipError)
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<u64, OwnershipError> {
        Err(OwnershipError)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<u64, OwnershipError> {
        Err(OwnershipError)
    }
    fn serialize_seq(self, _length: Option<usize>) -> Result<Self::SerializeSeq, OwnershipError> {
        Err(OwnershipError)
    }
    fn serialize_tuple(self, _length: usize) -> Result<Self::SerializeTuple, OwnershipError> {
        Err(OwnershipError)
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeTupleStruct, OwnershipError> {
        Err(OwnershipError)
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeTupleVariant, OwnershipError> {
        Err(OwnershipError)
    }
    fn serialize_map(self, _length: Option<usize>) -> Result<Self::SerializeMap, OwnershipError> {
        Err(OwnershipError)
    }
    fn serialize_struct(
        self,
        _name: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeStruct, OwnershipError> {
        Err(OwnershipError)
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _length: usize,
    ) -> Result<Self::SerializeStructVariant, OwnershipError> {
        Err(OwnershipError)
    }
}

impl SerializeMap for OwnershipCompound {
    type Ok = u64;
    type Error = OwnershipError;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), OwnershipError> {
        self.pending_key = Some(key.serialize(OwnershipMapKeySerializer)?);
        Ok(())
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), OwnershipError> {
        let key = self.pending_key.take().ok_or(OwnershipError)?;
        self.add_object_value(key, value)
    }
    fn end(self) -> Result<u64, OwnershipError> {
        if self.pending_key.is_some() {
            return Err(OwnershipError);
        }
        self.finish()
    }
}

#[derive(Default)]
struct JsonCountingWriter {
    bytes: u64,
    overflow: bool,
}

impl Write for JsonCountingWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        match self.bytes.checked_add(bytes.len() as u64) {
            Some(next) => self.bytes = next,
            None => {
                self.overflow = true;
                return Err(std::io::Error::other("JSON length overflow"));
            }
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn json_length<T: ?Sized + Serialize>(value: &T) -> Result<u64, IndexError> {
    let mut writer = JsonCountingWriter::default();
    if serde_json::to_writer(&mut writer, value).is_err() {
        return if writer.overflow {
            Err(IndexError::IntegerOutOfRange)
        } else {
            Err(IndexError::ProjectionContractViolation)
        };
    }
    Ok(writer.bytes)
}

/// Measures a value which will be stored as *text containing canonical JSON*.
///
/// `canonical_json` in Core sorts object keys but does not otherwise alter the
/// compact JSON representation, so key ordering cannot affect either length
/// below.  This writer deliberately counts the compact stream as it is
/// emitted: phase-0 must never build the intermediate canonical `Vec<u8>`
/// merely to learn the cost of storing it in a projected `String`.
#[derive(Default)]
struct CanonicalJsonTextCountingWriter {
    raw_bytes: u64,
    json_string_bytes: u64,
    overflow: bool,
}

impl Write for CanonicalJsonTextCountingWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        for byte in bytes {
            self.raw_bytes = self.raw_bytes.checked_add(1).ok_or_else(|| {
                self.overflow = true;
                std::io::Error::other("canonical JSON length overflow")
            })?;
            // A canonical JSON byte stream is itself placed in a JSON string
            // in the public snapshot.  Only quote and backslash gain an
            // additional escape byte; the serializer never emits raw ASCII
            // control bytes.
            let rendered = if matches!(*byte, b'"' | b'\\') { 2 } else { 1 };
            self.json_string_bytes =
                self.json_string_bytes
                    .checked_add(rendered)
                    .ok_or_else(|| {
                        self.overflow = true;
                        std::io::Error::other("canonical JSON string length overflow")
                    })?;
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Returns `(raw_canonical_bytes, JSON-string bytes, recursive ownership)`
/// without retaining a JSON value, a canonical byte vector, or a DTO clone.
/// The second number includes the surrounding quotes.
fn borrowed_canonical_json_text_metrics<T: ?Sized + Serialize>(
    value: &T,
) -> Result<(u64, u64, u64), IndexError> {
    let mut writer = CanonicalJsonTextCountingWriter::default();
    if serde_json::to_writer(&mut writer, value).is_err() {
        return if writer.overflow {
            Err(IndexError::IntegerOutOfRange)
        } else {
            Err(IndexError::ProjectionContractViolation)
        };
    }
    let json_string_bytes = writer
        .json_string_bytes
        .checked_add(2)
        .ok_or(IndexError::IntegerOutOfRange)?;
    Ok((
        writer.raw_bytes,
        json_string_bytes,
        recursive_ownership_charge(value)?,
    ))
}

/// Classifies one top-level serde field without first decoding it into a
/// `serde_json::Value`.  SQLite represents nested objects/arrays as their
/// canonical JSON text and scalar booleans/numbers as integer cells.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BorrowedSqlValue {
    Null,
    Integer,
    Text(u64),
    JsonText(u64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BorrowedFieldMetric {
    sql: BorrowedSqlValue,
    direct_json_bytes: u64,
    direct_owned_bytes: u64,
    canonical_string_json_bytes: u64,
    canonical_string_owned_bytes: u64,
}

/// Exact accounting for one projected row, assembled solely from borrowed
/// source fields.  It mirrors `json_length`/`OwnershipSerializer` for a
/// serde struct while retaining no projected values.
#[derive(Clone, Copy, Debug, Default)]
struct BorrowedRowCharge {
    json_bytes: u64,
    owned_bytes: u64,
    integer_cells: u64,
    text_bytes: u64,
    fields: u64,
}

impl BorrowedRowCharge {
    fn new() -> Self {
        Self {
            // `{}` for the empty struct; field delimiters are added below.
            json_bytes: 2,
            ..Self::default()
        }
    }

    fn key(&mut self, key: &'static str) -> Result<(), IndexError> {
        let key_bytes = u64::try_from(key.len()).map_err(|_| IndexError::IntegerOutOfRange)?;
        // `"key":` plus a comma for every non-first field.
        self.json_bytes = self
            .json_bytes
            .checked_add(key_bytes)
            .and_then(|value| value.checked_add(3))
            .and_then(|value| value.checked_add(u64::from(self.fields != 0)))
            .ok_or(IndexError::IntegerOutOfRange)?;
        // OwnershipSerializer charges object entries as 16 bytes plus key
        // bytes, then the recursively owned field payload.
        self.owned_bytes = self
            .owned_bytes
            .checked_add(16)
            .and_then(|value| value.checked_add(key_bytes))
            .ok_or(IndexError::IntegerOutOfRange)?;
        self.fields = self
            .fields
            .checked_add(1)
            .ok_or(IndexError::IntegerOutOfRange)?;
        Ok(())
    }

    fn direct<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), IndexError> {
        self.key(key)?;
        let json = json_length(value)?;
        let owned = recursive_ownership_charge(value)?;
        self.json_bytes = self
            .json_bytes
            .checked_add(json)
            .ok_or(IndexError::IntegerOutOfRange)?;
        self.owned_bytes = self
            .owned_bytes
            .checked_add(owned)
            .ok_or(IndexError::IntegerOutOfRange)?;
        self.add_sql_value(borrowed_sql_value(value)?)
    }

    /// Adds a projected `String` holding canonical JSON from a borrowed
    /// source component. SQL stores the raw canonical JSON text; snapshot JSON
    /// stores that text as an escaped JSON string.
    fn canonical_string<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), IndexError> {
        self.key(key)?;
        let (raw, escaped, _) = borrowed_canonical_json_text_metrics(value)?;
        self.json_bytes = self
            .json_bytes
            .checked_add(escaped)
            .ok_or(IndexError::IntegerOutOfRange)?;
        self.owned_bytes = self
            .owned_bytes
            .checked_add(raw)
            .ok_or(IndexError::IntegerOutOfRange)?;
        self.text_bytes = self
            .text_bytes
            .checked_add(raw)
            .ok_or(IndexError::IntegerOutOfRange)?;
        Ok(())
    }

    /// Same snapshot/ownership accounting as [`Self::direct`] but deliberately
    /// omits a SQLite cell (for embedded source objects retained only in the
    /// typed snapshot).
    fn json_only<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), IndexError> {
        self.key(key)?;
        self.json_bytes = self
            .json_bytes
            .checked_add(json_length(value)?)
            .ok_or(IndexError::IntegerOutOfRange)?;
        self.owned_bytes = self
            .owned_bytes
            .checked_add(recursive_ownership_charge(value)?)
            .ok_or(IndexError::IntegerOutOfRange)?;
        Ok(())
    }

    fn hash_shape(&mut self, key: &'static str) -> Result<(), IndexError> {
        self.direct(key, &HashShape)
    }

    fn add_sql_value(&mut self, value: BorrowedSqlValue) -> Result<(), IndexError> {
        match value {
            BorrowedSqlValue::Null => Ok(()),
            BorrowedSqlValue::Integer => {
                self.integer_cells = self
                    .integer_cells
                    .checked_add(1)
                    .ok_or(IndexError::IntegerOutOfRange)?;
                Ok(())
            }
            BorrowedSqlValue::Text(bytes) | BorrowedSqlValue::JsonText(bytes) => {
                self.text_bytes = self
                    .text_bytes
                    .checked_add(bytes)
                    .ok_or(IndexError::IntegerOutOfRange)?;
                Ok(())
            }
        }
    }

    fn finish(self) -> Result<Self, IndexError> {
        if self.fields == 0 {
            return Err(IndexError::ProjectionContractViolation);
        }
        Ok(self)
    }
}

#[derive(Default)]
struct BorrowedSqlValueWriter {
    first: Option<u8>,
    bytes: u64,
    string_bytes: u64,
    in_string: bool,
    escaped: bool,
    overflow: bool,
}

impl Write for BorrowedSqlValueWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        for byte in bytes {
            if self.first.is_none() && !byte.is_ascii_whitespace() {
                self.first = Some(*byte);
            }
            self.bytes = self.bytes.checked_add(1).ok_or_else(|| {
                self.overflow = true;
                std::io::Error::other("borrowed SQL value length overflow")
            })?;
            if self.first == Some(b'"') {
                if !self.in_string {
                    self.in_string = true;
                    continue;
                }
                if self.escaped {
                    // serde_json emits the short forms for all ASCII control
                    // characters, so one escaped token always denotes one
                    // source byte here. Non-ASCII UTF-8 is emitted directly.
                    self.string_bytes = self.string_bytes.checked_add(1).ok_or_else(|| {
                        self.overflow = true;
                        std::io::Error::other("borrowed SQL string length overflow")
                    })?;
                    self.escaped = false;
                } else if *byte == b'\\' {
                    self.escaped = true;
                } else if *byte == b'"' {
                    self.in_string = false;
                } else {
                    self.string_bytes = self.string_bytes.checked_add(1).ok_or_else(|| {
                        self.overflow = true;
                        std::io::Error::other("borrowed SQL string length overflow")
                    })?;
                }
            }
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn borrowed_sql_value<T: ?Sized + Serialize>(value: &T) -> Result<BorrowedSqlValue, IndexError> {
    let mut writer = BorrowedSqlValueWriter::default();
    if serde_json::to_writer(&mut writer, value).is_err() || writer.overflow {
        return Err(IndexError::ProjectionContractViolation);
    }
    match writer.first {
        Some(b'n') => Ok(BorrowedSqlValue::Null),
        Some(b'"') if !writer.in_string && !writer.escaped => {
            Ok(BorrowedSqlValue::Text(recursive_ownership_charge(value)?))
        }
        Some(b'{' | b'[') => Ok(BorrowedSqlValue::JsonText(writer.bytes)),
        Some(_) => Ok(BorrowedSqlValue::Integer),
        None => Err(IndexError::ProjectionContractViolation),
    }
}

#[derive(Debug)]
struct BorrowedFieldCollectorError;

impl std::fmt::Display for BorrowedFieldCollectorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("borrowed field collector")
    }
}
impl std::error::Error for BorrowedFieldCollectorError {}
impl ser::Error for BorrowedFieldCollectorError {
    fn custom<T: std::fmt::Display>(_message: T) -> Self {
        Self
    }
}

/// Visits a DTO's direct wire fields in its serde declaration order. The
/// collector is intentionally streaming: it does not retain names, values,
/// maps, or JSON buffers.
struct BorrowedTopLevelFields<'a, F> {
    visit: &'a mut F,
}

struct BorrowedTopLevelStruct<'a, F> {
    visit: &'a mut F,
}

macro_rules! borrowed_field_collector_unsupported {
    ($($name:ident($($arg:ident : $ty:ty),*)),* $(,)?) => {$(
        fn $name(self, $($arg: $ty),*) -> Result<Self::Ok, Self::Error> {
            let _ = ($($arg),*);
            Err(BorrowedFieldCollectorError)
        }
    )*};
}

impl<'a, 'b, F> Serializer for &'a mut BorrowedTopLevelFields<'b, F>
where
    F: FnMut(&'static str, BorrowedFieldMetric) -> Result<(), IndexError>,
{
    type Ok = ();
    type Error = BorrowedFieldCollectorError;
    type SerializeSeq = ser::Impossible<(), BorrowedFieldCollectorError>;
    type SerializeTuple = ser::Impossible<(), BorrowedFieldCollectorError>;
    type SerializeTupleStruct = ser::Impossible<(), BorrowedFieldCollectorError>;
    type SerializeTupleVariant = ser::Impossible<(), BorrowedFieldCollectorError>;
    type SerializeMap = ser::Impossible<(), BorrowedFieldCollectorError>;
    type SerializeStruct = BorrowedTopLevelStruct<'a, F>;
    type SerializeStructVariant = ser::Impossible<(), BorrowedFieldCollectorError>;

    borrowed_field_collector_unsupported!(
        serialize_bool(value: bool), serialize_i8(value: i8), serialize_i16(value: i16),
        serialize_i32(value: i32), serialize_i64(value: i64), serialize_u8(value: u8),
        serialize_u16(value: u16), serialize_u32(value: u32), serialize_u64(value: u64),
        serialize_f32(value: f32), serialize_f64(value: f64), serialize_char(value: char),
        serialize_str(value: &str), serialize_bytes(value: &[u8]), serialize_none(),
        serialize_unit(), serialize_unit_struct(name: &'static str),
        serialize_unit_variant(name: &'static str, index: u32, variant: &'static str)
    );

    fn serialize_some<T: ?Sized + Serialize>(
        self,
        _value: &T,
    ) -> Result<(), BorrowedFieldCollectorError> {
        Err(BorrowedFieldCollectorError)
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _value: &T,
    ) -> Result<(), BorrowedFieldCollectorError> {
        Err(BorrowedFieldCollectorError)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<(), BorrowedFieldCollectorError> {
        Err(BorrowedFieldCollectorError)
    }
    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Err(BorrowedFieldCollectorError)
    }
    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Err(BorrowedFieldCollectorError)
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(BorrowedFieldCollectorError)
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(BorrowedFieldCollectorError)
    }
    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Err(BorrowedFieldCollectorError)
    }
    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(BorrowedTopLevelStruct { visit: self.visit })
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(BorrowedFieldCollectorError)
    }
}

impl<F> SerializeStruct for BorrowedTopLevelStruct<'_, F>
where
    F: FnMut(&'static str, BorrowedFieldMetric) -> Result<(), IndexError>,
{
    type Ok = ();
    type Error = BorrowedFieldCollectorError;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        let sql = borrowed_sql_value(value).map_err(|_| BorrowedFieldCollectorError)?;
        let direct_json_bytes = json_length(value).map_err(|_| BorrowedFieldCollectorError)?;
        let direct_owned_bytes =
            recursive_ownership_charge(value).map_err(|_| BorrowedFieldCollectorError)?;
        let (canonical_string_owned_bytes, canonical_string_json_bytes, _) =
            borrowed_canonical_json_text_metrics(value).map_err(|_| BorrowedFieldCollectorError)?;
        let measured = BorrowedFieldMetric {
            sql,
            direct_json_bytes,
            direct_owned_bytes,
            canonical_string_json_bytes,
            canonical_string_owned_bytes,
        };
        (self.visit)(key, measured).map_err(|_| BorrowedFieldCollectorError)
    }
    fn end(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[allow(dead_code)]
fn charge_borrowed_dto_sql<T: Serialize>(
    value: &T,
    integer_cells: &mut u64,
    text_bytes: &mut u64,
) -> Result<(), IndexError> {
    let mut visit = |_: &'static str, field: BorrowedFieldMetric| match field.sql {
        BorrowedSqlValue::Null => Ok(()),
        BorrowedSqlValue::Integer => {
            *integer_cells = integer_cells
                .checked_add(1)
                .ok_or(IndexError::IntegerOutOfRange)?;
            Ok(())
        }
        BorrowedSqlValue::Text(bytes) | BorrowedSqlValue::JsonText(bytes) => {
            *text_bytes = text_bytes
                .checked_add(bytes)
                .ok_or(IndexError::IntegerOutOfRange)?;
            Ok(())
        }
    };
    value
        .serialize(&mut BorrowedTopLevelFields { visit: &mut visit })
        .map_err(|_| IndexError::ProjectionContractViolation)
}

/// A fixed-width hash placeholder.  Phase-0 only charges representation size;
/// all accepted content hashes use the fixed `sha256:<64 hex>` wire shape.
#[derive(Clone, Copy)]
struct HashShape;
impl Serialize for HashShape {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(
            "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        )
    }
}

#[allow(dead_code)]
#[derive(Serialize)]
struct OpaqueRegistrationV4Row<'a, S> {
    event_sequence: u64,
    event_id: &'a StableId,
    event_actor: &'a str,
    registration_id: &'a StableId,
    schema: &'a str,
    run_id: &'a StableId,
    cas_hash: &'a ContentHash,
    media_type: &'a str,
    size: u64,
    sensitivity: ArtifactSensitivity,
    source_kind: &'static str,
    source: S,
    descriptor_id: &'a StableId,
    body_hash: HashShape,
}

#[allow(dead_code)]
#[derive(Serialize)]
struct OpaqueDescriptorV4Row<'a, D> {
    event_sequence: u64,
    event_id: &'a StableId,
    descriptor: D,
    registration_id: &'a StableId,
    descriptor_hash: &'a ContentHash,
    descriptor_size: u64,
    body_hash: HashShape,
}

macro_rules! opaque_m5_row {
    ($name:ident, $field:ident $(, $owner:ident)?) => {
        #[allow(dead_code)]
        #[derive(Serialize)]
        struct $name<'a, T> {
            event_sequence: u64,
            event_id: &'a StableId,
            $($owner: &'a StableId,)?
            $field: T,
            body_hash: HashShape,
        }
    };
}
opaque_m5_row!(OpaqueCoverV4Row, cover);
opaque_m5_row!(OpaqueSectionV4Row, section);
opaque_m5_row!(OpaqueRestrictionV4Row, restriction, attempt_id);
opaque_m5_row!(OpaqueAttemptV4Row, attempt);
opaque_m5_row!(OpaqueCandidateV4Row, candidate, attempt_id);
opaque_m5_row!(OpaqueObstructionV4Row, obstruction);

#[allow(dead_code)]
fn charge_borrowed_event_tuple(
    event_id: &StableId,
    integer_cells: &mut u64,
    text_bytes: &mut u64,
) -> Result<(), IndexError> {
    *integer_cells = integer_cells
        .checked_add(1)
        .ok_or(IndexError::IntegerOutOfRange)?;
    add_text_charge(text_bytes, event_id.as_str())
}

#[allow(dead_code)]
fn charge_borrowed_m5_dto_item<T: Serialize>(
    event_id: &StableId,
    dto: &T,
    owner: Option<&StableId>,
    integer_cells: &mut u64,
    text_bytes: &mut u64,
) -> Result<(), IndexError> {
    charge_borrowed_event_tuple(event_id, integer_cells, text_bytes)?;
    if let Some(owner) = owner {
        add_text_charge(text_bytes, owner.as_str())?;
    }
    charge_borrowed_dto_sql(dto, integer_cells, text_bytes)?;
    add_text_charge(
        text_bytes,
        "sha256:0000000000000000000000000000000000000000000000000000000000000000",
    )
}

/// Adds only the M5 tail to an already-initialized phase-0 accumulator. This
/// is deliberately separate from inherited traversal: callers can compose it
/// with a borrowed v3 pass before admitting any projected snapshot allocation.
#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
fn phase0_add_m5_rows(
    charge: &mut ProjectionChargeV5,
    envelopes: &[reviewgraphen_core::EventEnvelope],
    inherited_len: usize,
    payment: Option<&(
        reviewgraphen_core::BorrowedArtifactRegistrationProjectionV4<'_>,
        reviewgraphen_core::BorrowedGluingInputDescriptorProjectionV4<'_>,
    )>,
    ui: Option<&(
        reviewgraphen_core::BorrowedArtifactRegistrationProjectionV4<'_>,
        reviewgraphen_core::BorrowedGluingInputDescriptorProjectionV4<'_>,
    )>,
    bundle: Option<&reviewgraphen_core::BorrowedGluingBundleProjectionV4<'_>>,
) -> Result<(), IndexError> {
    for (offset, (registration, descriptor)) in [payment, ui].into_iter().flatten().enumerate() {
        let envelope = envelopes
            .get(inherited_len + offset)
            .ok_or(IndexError::ProjectionContractViolation)?;
        let source = registration.source();
        let registration_row = OpaqueRegistrationV4Row {
            event_sequence: envelope.sequence(),
            event_id: envelope.id(),
            event_actor: envelope.actor(),
            registration_id: registration.registration_id(),
            schema: registration.schema(),
            run_id: registration.run_id(),
            cas_hash: registration.cas_hash(),
            media_type: registration.media_type(),
            size: registration.size(),
            sensitivity: registration.sensitivity(),
            source_kind: source.kind(),
            source: BorrowedCanonicalArtifactSourceV4(source),
            descriptor_id: descriptor.id(),
            body_hash: HashShape,
        };
        charge.max_cas_bytes = charge.max_cas_bytes.max(registration.size());
        charge.add_m5_array(REGISTRATIONS_V4, &registration_row, |integers, text| {
            charge_borrowed_event_tuple(envelope.id(), integers, text)?;
            charge_borrowed_dto_sql(
                &BorrowedRegistrationProjectionV4(registration),
                integers,
                text,
            )?;
            for value in [
                registration_row.source_kind,
                descriptor.id().as_str(),
                "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            ] {
                add_text_charge(text, value)?;
            }
            Ok(())
        })?;
        let descriptor_row = OpaqueDescriptorV4Row {
            event_sequence: envelope.sequence(),
            event_id: envelope.id(),
            descriptor: BorrowedDescriptorProjectionV4(descriptor),
            registration_id: registration.registration_id(),
            descriptor_hash: registration.cas_hash(),
            descriptor_size: registration.size(),
            body_hash: HashShape,
        };
        charge.add_m5_array(GLUING_DESCRIPTORS, &descriptor_row, |integers, text| {
            charge_borrowed_event_tuple(envelope.id(), integers, text)?;
            charge_borrowed_dto_sql(&BorrowedDescriptorProjectionV4(descriptor), integers, text)?;
            for value in [
                registration.registration_id().as_str(),
                registration.cas_hash().as_str(),
                "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            ] {
                add_text_charge(text, value)?;
            }
            *integers = integers
                .checked_add(1)
                .ok_or(IndexError::IntegerOutOfRange)?;
            Ok(())
        })?;
    }
    let Some(bundle) = bundle else {
        return Ok(());
    };
    let offset = usize::from(payment.is_some()) + usize::from(ui.is_some());
    let envelope = envelopes
        .get(inherited_len + offset)
        .ok_or(IndexError::ProjectionContractViolation)?;
    let event_sequence = envelope.sequence();
    let event_id = envelope.id();
    let cover_value = bundle.cover();
    let cover = OpaqueCoverV4Row {
        event_sequence,
        event_id,
        cover: BorrowedCoverProjectionV4(&cover_value),
        body_hash: HashShape,
    };
    charge.add_m5_array(CONTEXT_COVERS, &cover, |integers, text| {
        charge_borrowed_m5_dto_item(
            event_id,
            &BorrowedCoverProjectionV4(&cover_value),
            None,
            integers,
            text,
        )
    })?;
    for section in bundle.sections() {
        let row = OpaqueSectionV4Row {
            event_sequence,
            event_id,
            section: BorrowedSectionProjectionValueV4(&section),
            body_hash: HashShape,
        };
        charge.add_m5_array(SECTIONS, &row, |integers, text| {
            charge_borrowed_m5_dto_item(
                event_id,
                &BorrowedSectionProjectionValueV4(&section),
                None,
                integers,
                text,
            )
        })?;
    }
    let attempt_value = bundle.attempt();
    let attempt_id = attempt_value.id();
    for restriction in bundle.restrictions() {
        let row = OpaqueRestrictionV4Row {
            event_sequence,
            event_id,
            attempt_id,
            restriction: BorrowedRestrictionProjectionValueV4(&restriction),
            body_hash: HashShape,
        };
        charge.add_m5_array(RESTRICTIONS, &row, |integers, text| {
            charge_borrowed_m5_dto_item(
                event_id,
                &BorrowedRestrictionProjectionValueV4(&restriction),
                Some(attempt_id),
                integers,
                text,
            )
        })?;
    }
    let attempt = OpaqueAttemptV4Row {
        event_sequence,
        event_id,
        attempt: BorrowedAttemptProjectionV4(&attempt_value),
        body_hash: HashShape,
    };
    charge.add_m5_array(GLUING_ATTEMPTS, &attempt, |integers, text| {
        charge_borrowed_m5_dto_item(
            event_id,
            &BorrowedAttemptProjectionV4(&attempt_value),
            None,
            integers,
            text,
        )
    })?;
    if let Some(candidate) = bundle.global_candidate() {
        let row = OpaqueCandidateV4Row {
            event_sequence,
            event_id,
            attempt_id,
            candidate: BorrowedCandidateProjectionV4(&candidate),
            body_hash: HashShape,
        };
        charge.add_m5_array(GLOBAL_CANDIDATES, &row, |integers, text| {
            charge_borrowed_m5_dto_item(
                event_id,
                &BorrowedCandidateProjectionV4(&candidate),
                Some(attempt_id),
                integers,
                text,
            )
        })?;
    }
    if let Some(obstruction) = bundle.obstruction() {
        let row = OpaqueObstructionV4Row {
            event_sequence,
            event_id,
            obstruction: BorrowedObstructionProjectionV4(&obstruction),
            body_hash: HashShape,
        };
        charge.add_m5_array(GLUING_OBSTRUCTIONS, &row, |integers, text| {
            charge_borrowed_m5_dto_item(
                event_id,
                &BorrowedObstructionProjectionV4(&obstruction),
                None,
                integers,
                text,
            )
        })?;
    }
    Ok(())
}

trait ProjectionEventMetadataV5 {
    fn sequence(&self) -> u64;
    fn id(&self) -> &StableId;
    fn schema(&self) -> &str;
    fn event_hash(&self) -> &ContentHash;
    fn payload_hash(&self) -> &ContentHash;
    fn actor(&self) -> &str;
    fn logical_time(&self) -> u64;
}

impl ProjectionEventMetadataV5 for reviewgraphen_core::EventEnvelope {
    fn sequence(&self) -> u64 {
        self.sequence()
    }
    fn id(&self) -> &StableId {
        self.id()
    }
    fn schema(&self) -> &str {
        EVENT_CONTRACT_VERSION_V4
    }
    fn event_hash(&self) -> &ContentHash {
        self.event_hash()
    }
    fn payload_hash(&self) -> &ContentHash {
        self.payload_hash()
    }
    fn actor(&self) -> &str {
        self.actor()
    }
    fn logical_time(&self) -> u64 {
        self.logical_time()
    }
}

impl ProjectionEventMetadataV5 for BorrowedV4EventMetadata<'_> {
    fn sequence(&self) -> u64 {
        self.sequence()
    }
    fn id(&self) -> &StableId {
        self.id()
    }
    fn schema(&self) -> &str {
        self.schema()
    }
    fn event_hash(&self) -> &ContentHash {
        self.event_hash()
    }
    fn payload_hash(&self) -> &ContentHash {
        self.payload_hash()
    }
    fn actor(&self) -> &str {
        self.actor()
    }
    fn logical_time(&self) -> u64 {
        self.logical_time()
    }
}

fn phase0_event_base(
    envelope: &impl ProjectionEventMetadataV5,
) -> Result<BorrowedRowCharge, IndexError> {
    let mut row = BorrowedRowCharge::new();
    row.direct("event_sequence", &envelope.sequence())?;
    row.direct("event_id", envelope.id())?;
    Ok(row)
}

fn phase0_event_row(
    envelope: &impl ProjectionEventMetadataV5,
    payload_kind: &'static str,
) -> Result<BorrowedRowCharge, IndexError> {
    let mut row = BorrowedRowCharge::new();
    row.direct("sequence", &envelope.sequence())?;
    row.direct("event_id", envelope.id())?;
    row.direct("schema", envelope.schema())?;
    row.direct("event_hash", envelope.event_hash())?;
    row.direct("payload_hash", envelope.payload_hash())?;
    row.direct("payload_kind", payload_kind)?;
    row.direct("actor", envelope.actor())?;
    row.direct("logical_time", &envelope.logical_time())?;
    row.finish()
}

fn phase0_registration_v3(
    envelope: &impl ProjectionEventMetadataV5,
    value: &BorrowedArtifactRegistrationProjectionV3<'_>,
) -> Result<BorrowedRowCharge, IndexError> {
    let mut row = phase0_event_base(envelope)?;
    row.direct("registration_id", value.registration_id())?;
    row.direct("run_id", value.run_id())?;
    row.direct("cas_hash", value.cas_hash())?;
    row.direct("media_type", value.media_type())?;
    row.direct("size", &value.size())?;
    row.direct("sensitivity", &value.sensitivity())?;
    row.direct("source_kind", &value.source().kind())?;
    let source = BorrowedCanonicalArtifactSourceV3(value.source());
    row.canonical_string("source_canonical_json", &source)?;
    row.json_only("source", &source)?;
    row.hash_shape("body_hash")?;
    row.finish()
}

/// Store-owned serializer over Core's callback-scoped scalar source view.
/// Core guarantees lexicographic complete field order, so this emits the same
/// canonical object without retaining or decoding a source DTO.
struct BorrowedCanonicalArtifactSourceV3<'a>(BorrowedArtifactSourceProjectionV3<'a>);
impl Serialize for BorrowedCanonicalArtifactSourceV3<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        let mut failure = None;
        self.0.visit_fields(|key, value| {
            if failure.is_none()
                && let Err(error) = map.serialize_entry(key, &value)
            {
                failure = Some(error);
            }
        });
        if let Some(error) = failure {
            return Err(error);
        }
        map.end()
    }
}

struct BorrowedRegistrationProjectionV3<'a, 'b>(&'b BorrowedArtifactRegistrationProjectionV3<'a>);
impl Serialize for BorrowedRegistrationProjectionV3<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(7))?;
        map.serialize_entry("cas_hash", self.0.cas_hash())?;
        map.serialize_entry("media_type", self.0.media_type())?;
        map.serialize_entry("registration_id", self.0.registration_id())?;
        map.serialize_entry("run_id", self.0.run_id())?;
        map.serialize_entry("sensitivity", &self.0.sensitivity())?;
        map.serialize_entry("size", &self.0.size())?;
        map.serialize_entry(
            "source",
            &BorrowedCanonicalArtifactSourceV3(self.0.source()),
        )?;
        map.end()
    }
}

struct BorrowedCanonicalArtifactSourceV4<'a>(
    reviewgraphen_core::BorrowedArtifactSourceProjectionV4<'a>,
);
impl Serialize for BorrowedCanonicalArtifactSourceV4<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        let mut failure = None;
        self.0.visit_fields(|key, value| {
            if failure.is_none()
                && let Err(error) = map.serialize_entry(key, &value)
            {
                failure = Some(error);
            }
        });
        if let Some(error) = failure {
            return Err(error);
        }
        map.end()
    }
}

struct BorrowedRegistrationProjectionV4<'a, 'b>(
    &'b reviewgraphen_core::BorrowedArtifactRegistrationProjectionV4<'a>,
);
impl Serialize for BorrowedRegistrationProjectionV4<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_struct("ArtifactRegistrationV4", 8)?;
        map.serialize_field("cas_hash", self.0.cas_hash())?;
        map.serialize_field("id", self.0.registration_id())?;
        map.serialize_field("media_type", self.0.media_type())?;
        map.serialize_field("run_id", self.0.run_id())?;
        map.serialize_field("schema", self.0.schema())?;
        map.serialize_field("sensitivity", &self.0.sensitivity())?;
        map.serialize_field("size", &self.0.size())?;
        map.serialize_field(
            "source",
            &BorrowedCanonicalArtifactSourceV4(self.0.source()),
        )?;
        map.end()
    }
}

struct BorrowedDescriptorProjectionV4<'a, 'b>(
    &'b reviewgraphen_core::BorrowedGluingInputDescriptorProjectionV4<'a>,
);
impl Serialize for BorrowedDescriptorProjectionV4<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_struct("GluingInputDescriptorV4", 11)?;
        map.serialize_field("assignment_key", self.0.assignment_key())?;
        map.serialize_field("assignment_value", &self.0.assignment_value())?;
        map.serialize_field("context_id", self.0.context_id())?;
        map.serialize_field("id", self.0.id())?;
        map.serialize_field("plan_id", self.0.plan_id())?;
        map.serialize_field("profile_descriptor_id", self.0.profile_descriptor_id())?;
        map.serialize_field(
            "qualification_source_ids",
            &BorrowedIteratorSequence(RefCell::new(self.0.qualification_source_ids())),
        )?;
        map.serialize_field("run_id", self.0.run_id())?;
        map.serialize_field("schema", self.0.schema())?;
        map.serialize_field("snapshot_id", self.0.snapshot_id())?;
        map.serialize_field("universe_id", self.0.universe_id())?;
        map.end()
    }
}

fn severity_text(value: Severity) -> &'static str {
    match value {
        Severity::Info => "info",
        Severity::Low => "low",
        Severity::Medium => "medium",
        Severity::High => "high",
        Severity::Critical => "critical",
    }
}

fn phase0_plan_row(
    envelope: &impl ProjectionEventMetadataV5,
    plan: &reviewgraphen_core::BorrowedReviewPlanProjectionV4<'_>,
) -> Result<BorrowedRowCharge, IndexError> {
    let mut row = phase0_event_base(envelope)?;
    row.direct("plan_id", plan.id())?;
    row.direct("universe_id", plan.universe_id())?;
    row.direct("snapshot_id", plan.snapshot_id())?;
    row.direct("planner_input_hash", plan.planner_input_hash())?;
    row.direct("planner_policy_version", plan.planner_policy_version())?;
    row.direct("planner_policy_hash", plan.planner_policy_hash())?;
    row.canonical_string("budget_canonical_json", &BorrowedPlanBudget(plan.budget()))?;
    row.hash_shape("budget_hash")?;
    row.canonical_string(
        "risk_breakdown_canonical_json",
        &BorrowedPlanRiskProjection(RefCell::new(plan.risk_breakdown())),
    )?;
    row.canonical_string(
        "waves_canonical_json",
        &BorrowedPlanWavesProjection(RefCell::new(plan.waves())),
    )?;
    row.canonical_string(
        "deferred_canonical_json",
        &BorrowedPlanDeferredProjection(RefCell::new(plan.deferred())),
    )?;
    row.hash_shape("identity_body_hash")?;
    row.hash_shape("body_hash")?;
    row.finish()
}

struct BorrowedPlanBudget(reviewgraphen_core::BorrowedPlanBudgetProjectionV4);
impl Serialize for BorrowedPlanBudget {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut st = s.serialize_struct("Budget", 2)?;
        st.serialize_field(
            "max_obligations_per_wave",
            &self.0.max_obligations_per_wave(),
        )?;
        st.serialize_field("max_waves", &self.0.max_waves())?;
        st.end()
    }
}
struct BorrowedPlanRiskProjection<'a>(RefCell<reviewgraphen_core::BorrowedPlanRiskIterV4<'a>>);
impl Serialize for BorrowedPlanRiskProjection<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut sequence = s.serialize_seq(None)?;
        for v in self.0.borrow_mut().by_ref() {
            struct R<'a>(reviewgraphen_core::BorrowedPlanRiskProjectionV4<'a>);
            impl Serialize for R<'_> {
                fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                    let mut st = s.serialize_struct("Risk", 3)?;
                    st.serialize_field("id", self.0.id())?;
                    st.serialize_field("impact", &severity_text(self.0.impact()))?;
                    st.serialize_field("likelihood", &severity_text(self.0.likelihood()))?;
                    st.end()
                }
            }
            sequence.serialize_element(&R(v))?;
        }
        sequence.end()
    }
}
struct BorrowedPlanWavesProjection<'a>(RefCell<reviewgraphen_core::BorrowedPlanWaveIterV4<'a>>);
impl Serialize for BorrowedPlanWavesProjection<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(None)?;
        for v in self.0.borrow_mut().by_ref() {
            struct W<'a>(reviewgraphen_core::BorrowedPlanWaveProjectionV4<'a>);
            impl Serialize for W<'_> {
                fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                    let mut st = s.serialize_struct("Wave", 2)?;
                    st.serialize_field(
                        "obligation_ids",
                        &BorrowedIdIter(RefCell::new(self.0.obligation_ids())),
                    )?;
                    st.serialize_field("wave_index", &self.0.wave_index())?;
                    st.end()
                }
            }
            seq.serialize_element(&W(v))?;
        }
        seq.end()
    }
}
struct BorrowedPlanDeferredProjection<'a>(
    RefCell<reviewgraphen_core::BorrowedPlanDeferredIterV4<'a>>,
);
impl Serialize for BorrowedPlanDeferredProjection<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(None)?;
        for v in self.0.borrow_mut().by_ref() {
            struct D<'a>(reviewgraphen_core::BorrowedPlanDeferredProjectionV4<'a>);
            impl Serialize for D<'_> {
                fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                    let mut st = s.serialize_struct("Deferred", 2)?;
                    st.serialize_field("id", self.0.id())?;
                    st.serialize_field("reason", self.0.reason())?;
                    st.end()
                }
            }
            seq.serialize_element(&D(v))?;
        }
        seq.end()
    }
}
struct BorrowedIdIter<'a>(RefCell<reviewgraphen_core::BorrowedStableIdSliceIterV4<'a>>);
impl Serialize for BorrowedIdIter<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(None)?;
        for id in self.0.borrow_mut().by_ref() {
            seq.serialize_element(id)?;
        }
        seq.end()
    }
}

fn phase0_context_row(
    envelope: &impl ProjectionEventMetadataV5,
    value: &reviewgraphen_core::BorrowedContextEnvelopeProjectionV4<'_>,
) -> Result<BorrowedRowCharge, IndexError> {
    let mut row = phase0_event_base(envelope)?;
    row.direct("envelope_id", value.id())?;
    row.direct("snapshot_id", value.snapshot_id())?;
    row.direct("context_policy_version", value.projection_policy_version())?;
    row.direct("context_policy_hash", value.context_policy_hash())?;
    row.canonical_string(
        "candidate_ids_canonical_json",
        &BorrowedIteratorSequence(RefCell::new(value.candidate_source_ids())),
    )?;
    row.canonical_string(
        "obligation_ids_canonical_json",
        &BorrowedIteratorSequence(RefCell::new(value.obligation_ids())),
    )?;
    row.canonical_string(
        "context_policy_canonical_json",
        &BorrowedContextPolicy(value.context_policy()),
    )?;
    row.canonical_string(
        "included_sources_canonical_json",
        &BorrowedIncludedSources(RefCell::new(value.included_sources())),
    )?;
    row.canonical_string(
        "excluded_sources_canonical_json",
        &BorrowedExcludedSources(RefCell::new(value.excluded_sources())),
    )?;
    row.canonical_string(
        "unknowns_canonical_json",
        &BorrowedContextUnknowns(RefCell::new(value.unknowns())),
    )?;
    row.canonical_string(
        "assumptions_canonical_json",
        &BorrowedIteratorSequence(RefCell::new(value.assumptions())),
    )?;
    row.canonical_string(
        "losses_canonical_json",
        &BorrowedContextLosses(RefCell::new(value.losses())),
    )?;
    row.direct("projection_hash", value.projection_hash())?;
    row.hash_shape("body_hash")?;
    row.finish()
}

struct BorrowedIteratorSequence<I>(RefCell<I>);
impl<I> Serialize for BorrowedIteratorSequence<I>
where
    I: Iterator,
    I::Item: Serialize,
{
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(None)?;
        for value in self.0.borrow_mut().by_ref() {
            sequence.serialize_element(&value)?;
        }
        sequence.end()
    }
}

struct BorrowedContextPolicy<'a>(reviewgraphen_core::BorrowedContextPolicyProjectionV4<'a>);
impl Serialize for BorrowedContextPolicy<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        let mut failure = None;
        self.0.visit_scalar_fields(|key, value| {
            if failure.is_some() {
                return;
            }
            let result = (|| {
                if key == "excerpt_lines" {
                    map.serialize_entry(
                        "edge_kind_direction_order",
                        &BorrowedIteratorSequence(RefCell::new(self.0.edge_kind_direction_order())),
                    )?;
                    map.serialize_entry(
                        "exclusion_reason_precedence",
                        &BorrowedIteratorSequence(RefCell::new(
                            self.0.exclusion_reason_precedence(),
                        )),
                    )?;
                } else if key == "max_assumptions" {
                    map.serialize_entry(
                        "loss_descriptions",
                        &BorrowedContextPolicyLossDescriptions(RefCell::new(
                            self.0.loss_descriptions(),
                        )),
                    )?;
                } else if key == "version" {
                    map.serialize_entry(
                        "rules",
                        &BorrowedIteratorSequence(RefCell::new(self.0.rules())),
                    )?;
                    map.serialize_entry(
                        "unknown_descriptions",
                        &BorrowedIteratorSequence(RefCell::new(self.0.unknown_descriptions())),
                    )?;
                }
                map.serialize_entry(key, &value)
            })();
            if let Err(error) = result {
                failure = Some(error);
            }
        });
        if let Some(error) = failure {
            return Err(error);
        }
        map.end()
    }
}

struct BorrowedContextPolicyLossDescriptions(
    RefCell<reviewgraphen_core::BorrowedContextPolicyLossDescriptionIterV4>,
);
impl Serialize for BorrowedContextPolicyLossDescriptions {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(None)?;
        for item in self.0.borrow_mut().by_ref() {
            sequence.serialize_element(&[item.key(), item.description()])?;
        }
        sequence.end()
    }
}

struct BorrowedIncludedSources<'a>(
    RefCell<reviewgraphen_core::BorrowedContextIncludedSourceIterV4<'a>>,
);
impl Serialize for BorrowedIncludedSources<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        struct Item<'a>(reviewgraphen_core::BorrowedContextIncludedSourceProjectionV4<'a>);
        struct Excerpt<'a>(reviewgraphen_core::BorrowedContextExcerptProjectionV4<'a>);
        impl Serialize for Excerpt<'_> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("end_line", &self.0.end_line())?;
                map.serialize_entry("start_line", &self.0.start_line())?;
                map.end()
            }
        }
        impl Serialize for Item<'_> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut map = serializer.serialize_map(Some(7))?;
                map.serialize_entry("artifact_id", self.0.artifact_id())?;
                map.serialize_entry("cas_hash", self.0.cas_hash())?;
                map.serialize_entry("content_hash", self.0.content_hash())?;
                match self.0.excerpt() {
                    Some(excerpt) => map.serialize_entry("excerpt", &Excerpt(excerpt))?,
                    None => map.serialize_entry("excerpt", &Option::<u8>::None)?,
                }
                map.serialize_entry("excerpt_byte_length", &self.0.excerpt_byte_length())?;
                map.serialize_entry("excerpt_hash", self.0.excerpt_hash())?;
                map.serialize_entry("registration_id", self.0.registration_id())?;
                map.end()
            }
        }
        let mut sequence = serializer.serialize_seq(None)?;
        for item in self.0.borrow_mut().by_ref() {
            sequence.serialize_element(&Item(item))?;
        }
        sequence.end()
    }
}

struct BorrowedExcludedSources<'a>(
    RefCell<reviewgraphen_core::BorrowedContextExcludedSourceIterV4<'a>>,
);
impl Serialize for BorrowedExcludedSources<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        struct Item<'a>(reviewgraphen_core::BorrowedContextExcludedSourceProjectionV4<'a>);
        impl Serialize for Item<'_> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("artifact_id", self.0.artifact_id())?;
                map.serialize_entry("reason", self.0.reason())?;
                map.end()
            }
        }
        let mut sequence = serializer.serialize_seq(None)?;
        for item in self.0.borrow_mut().by_ref() {
            sequence.serialize_element(&Item(item))?;
        }
        sequence.end()
    }
}

struct BorrowedContextUnknowns<'a>(RefCell<reviewgraphen_core::BorrowedContextUnknownIterV4<'a>>);
impl Serialize for BorrowedContextUnknowns<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        struct Item<'a>(reviewgraphen_core::BorrowedContextUnknownProjectionV4<'a>);
        impl Serialize for Item<'_> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("description", self.0.description())?;
                map.serialize_entry(
                    "source_ids",
                    &BorrowedIteratorSequence(RefCell::new(self.0.source_ids())),
                )?;
                map.end()
            }
        }
        let mut sequence = serializer.serialize_seq(None)?;
        for item in self.0.borrow_mut().by_ref() {
            sequence.serialize_element(&Item(item))?;
        }
        sequence.end()
    }
}

struct BorrowedContextLosses<'a>(RefCell<reviewgraphen_core::BorrowedContextLossIterV4<'a>>);
impl Serialize for BorrowedContextLosses<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        struct Item<'a>(reviewgraphen_core::BorrowedContextLossProjectionV4<'a>);
        impl Serialize for Item<'_> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut map = serializer.serialize_map(Some(4))?;
                map.serialize_entry(
                    "affected_properties",
                    &BorrowedIteratorSequence(RefCell::new(self.0.affected_properties())),
                )?;
                map.serialize_entry("description", self.0.description())?;
                map.serialize_entry("severity", severity_text(self.0.severity()))?;
                map.serialize_entry(
                    "source_ids",
                    &BorrowedIteratorSequence(RefCell::new(self.0.source_ids())),
                )?;
                map.end()
            }
        }
        let mut sequence = serializer.serialize_seq(None)?;
        for item in self.0.borrow_mut().by_ref() {
            sequence.serialize_element(&Item(item))?;
        }
        sequence.end()
    }
}

fn phase0_execution_row(
    envelope: &impl ProjectionEventMetadataV5,
    value: &reviewgraphen_core::BorrowedReviewExecutionProjectionV4<'_>,
) -> Result<BorrowedRowCharge, IndexError> {
    let mut row = phase0_event_base(envelope)?;
    row.direct("execution_id", value.id())?;
    row.direct("plan_id", value.plan_id())?;
    row.direct("wave_id", value.wave_id())?;
    row.direct("snapshot_id", value.snapshot_id())?;
    row.direct("envelope_id", value.envelope_id())?;
    row.canonical_string(
        "obligation_ids_canonical_json",
        &BorrowedIteratorSequence(RefCell::new(value.obligation_ids())),
    )?;
    row.direct("reviewer_kind", value.reviewer_kind())?;
    row.direct("reviewer_id", value.reviewer_id())?;
    row.direct("provider", &value.provider())?;
    row.direct("model", &value.model())?;
    row.direct("model_revision", &value.model_revision())?;
    row.direct("system_prompt_version", value.system_prompt_version())?;
    row.direct("prompt_template_version", value.prompt_template_version())?;
    row.canonical_string(
        "inference_settings_canonical_json",
        &BorrowedExecutionSettings(RefCell::new(value.inference_settings())),
    )?;
    row.direct("tool_policy_version", value.tool_policy_version())?;
    row.canonical_string("tool_calls_canonical_json", &[] as &[u8])?;
    row.direct("attempt", &value.attempt())?;
    row.direct("raw_registration_id", value.raw_artifact_registration_id())?;
    row.direct("raw_hash", value.raw_artifact_hash())?;
    row.canonical_string(
        "parsed_claim_ids_canonical_json",
        &BorrowedIteratorSequence(RefCell::new(value.parsed_claim_ids())),
    )?;
    let outcome = value.outcome();
    row.direct("outcome_kind", outcome.kind())?;
    row.canonical_string("outcome_canonical_json", &BorrowedExecutionOutcome(outcome))?;
    row.hash_shape("identity_body_hash")?;
    row.hash_shape("body_hash")?;
    row.finish()
}

fn phase0_claim_row(
    envelope: &impl ProjectionEventMetadataV5,
    value: &reviewgraphen_core::BorrowedExecutionClaimProjectionV4<'_>,
) -> Result<BorrowedRowCharge, IndexError> {
    let mut row = phase0_event_base(envelope)?;
    row.direct("claim_id", value.id())?;
    row.direct("execution_id", value.execution_id())?;
    row.canonical_string(
        "obligation_ids_canonical_json",
        &BorrowedIteratorSequence(RefCell::new(value.obligation_ids())),
    )?;
    row.direct("property_id", value.property_id())?;
    row.canonical_string(
        "target_refs_canonical_json",
        &BorrowedIteratorSequence(RefCell::new(value.target_refs())),
    )?;
    row.direct("polarity", value.polarity())?;
    row.direct("disposition", value.disposition())?;
    row.direct("summary", value.summary())?;
    row.canonical_string(
        "source_ids_canonical_json",
        &BorrowedIteratorSequence(RefCell::new(value.source_ids())),
    )?;
    row.canonical_string(
        "assumptions_canonical_json",
        &BorrowedIteratorSequence(RefCell::new(value.assumptions())),
    )?;
    row.canonical_string(
        "requested_evidence_canonical_json",
        &BorrowedIteratorSequence(RefCell::new(value.requested_evidence())),
    )?;
    row.canonical_string(
        "candidate_confidence_canonical_json",
        &value.candidate_confidence(),
    )?;
    row.direct("author_kind", value.author_kind())?;
    row.direct("review_status", value.review_status())?;
    row.hash_shape("identity_body_hash")?;
    row.hash_shape("body_hash")?;
    row.finish()
}

struct BorrowedExecutionSettings<'a>(
    RefCell<reviewgraphen_core::BorrowedExecutionSettingIterV4<'a>>,
);
impl Serialize for BorrowedExecutionSettings<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        for setting in self.0.borrow_mut().by_ref() {
            map.serialize_entry(setting.key(), setting.value())?;
        }
        map.end()
    }
}

macro_rules! iterator_field {
    ($map:expr, $key:literal, $iter:expr) => {
        $map.serialize_field($key, &BorrowedIteratorSequence(RefCell::new($iter)))?
    };
}

struct BorrowedCoverProjectionV4<'a, 'b>(
    &'b reviewgraphen_core::BorrowedContextCoverProjectionV4<'a>,
);
impl Serialize for BorrowedCoverProjectionV4<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_struct("ContextCoverV4", 13)?;
        iterator_field!(map, "cover_domain_ids", self.0.cover_domain_ids());
        iterator_field!(map, "covered_domain_ids", self.0.covered_domain_ids());
        map.serialize_field("id", self.0.id())?;
        map.serialize_field("plan_id", self.0.plan_id())?;
        map.serialize_field("profile_descriptor_id", self.0.profile_descriptor_id())?;
        iterator_field!(map, "required_context_ids", self.0.required_context_ids());
        map.serialize_field("run_id", self.0.run_id())?;
        map.serialize_field("schema", self.0.schema())?;
        iterator_field!(
            map,
            "selected_obligation_ids",
            self.0.selected_obligation_ids()
        );
        map.serialize_field("snapshot_id", self.0.snapshot_id())?;
        iterator_field!(map, "source_ids", self.0.source_ids());
        iterator_field!(map, "uncovered_domain_ids", self.0.uncovered_domain_ids());
        map.serialize_field("universe_id", self.0.universe_id())?;
        map.end()
    }
}

struct BorrowedSectionProjectionValueV4<'a, 'b>(
    &'b reviewgraphen_core::BorrowedSectionProjectionV4<'a>,
);
impl Serialize for BorrowedSectionProjectionValueV4<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_struct("SectionV4", 22)?;
        map.serialize_field("assignment_key", self.0.assignment_key())?;
        map.serialize_field("assignment_value", &self.0.assignment_value())?;
        iterator_field!(map, "binding_ids", self.0.binding_ids());
        map.serialize_field("claim_assessment_id", self.0.claim_assessment_id())?;
        map.serialize_field("claim_id", self.0.claim_id())?;
        map.serialize_field("context_id", self.0.context_id())?;
        map.serialize_field("cover_id", self.0.cover_id())?;
        iterator_field!(map, "decision_ids", self.0.decision_ids());
        iterator_field!(map, "evidence_ids", self.0.evidence_ids());
        iterator_field!(map, "finding_ids", self.0.finding_ids());
        map.serialize_field("id", self.0.id())?;
        map.serialize_field("input_descriptor_id", self.0.input_descriptor_id())?;
        map.serialize_field("input_registration_id", self.0.input_registration_id())?;
        map.serialize_field("invariant_id", self.0.invariant_id())?;
        map.serialize_field("obligation_id", self.0.obligation_id())?;
        map.serialize_field(
            "passed_current_verification",
            &self.0.passed_current_verification(),
        )?;
        map.serialize_field("property_id", self.0.property_id())?;
        iterator_field!(
            map,
            "qualification_source_ids",
            self.0.qualification_source_ids()
        );
        map.serialize_field("schema", self.0.schema())?;
        map.serialize_field("snapshot_id", self.0.snapshot_id())?;
        iterator_field!(map, "source_ids", self.0.source_ids());
        iterator_field!(map, "verification_ids", self.0.verification_ids());
        map.end()
    }
}

struct BorrowedRestrictionProjectionValueV4<'a, 'b>(
    &'b reviewgraphen_core::BorrowedRestrictionProjectionV4<'a>,
);
impl Serialize for BorrowedRestrictionProjectionValueV4<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_struct("RestrictionV4", 14)?;
        map.serialize_field("assignment_key", self.0.assignment_key())?;
        map.serialize_field("assignment_value", &self.0.assignment_value())?;
        iterator_field!(map, "claim_ids", self.0.claim_ids());
        iterator_field!(map, "context_pair", self.0.context_pair());
        iterator_field!(map, "decision_ids", self.0.decision_ids());
        iterator_field!(map, "evidence_ids", self.0.evidence_ids());
        iterator_field!(map, "finding_ids", self.0.finding_ids());
        map.serialize_field("id", self.0.id())?;
        iterator_field!(map, "overlap_member_ids", self.0.overlap_member_ids());
        iterator_field!(
            map,
            "qualification_source_ids",
            self.0.qualification_source_ids()
        );
        map.serialize_field("schema", self.0.schema())?;
        map.serialize_field("section_id", self.0.section_id())?;
        iterator_field!(map, "source_ids", self.0.source_ids());
        iterator_field!(map, "verification_ids", self.0.verification_ids());
        map.end()
    }
}

struct BorrowedAttemptProjectionV4<'a, 'b>(
    &'b reviewgraphen_core::BorrowedGluingAttemptProjectionV4<'a>,
);
impl Serialize for BorrowedAttemptProjectionV4<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_struct("GluingAttemptV4", 18)?;
        iterator_field!(map, "claim_ids", self.0.claim_ids());
        map.serialize_field("cover_id", self.0.cover_id())?;
        iterator_field!(map, "decision_ids", self.0.decision_ids());
        iterator_field!(map, "evidence_ids", self.0.evidence_ids());
        iterator_field!(map, "finding_ids", self.0.finding_ids());
        map.serialize_field("global_candidate_id", &self.0.global_candidate_id())?;
        map.serialize_field("id", self.0.id())?;
        iterator_field!(map, "input_descriptor_ids", self.0.input_descriptor_ids());
        map.serialize_field("invariant_id", self.0.invariant_id())?;
        map.serialize_field("obstruction_id", &self.0.obstruction_id())?;
        map.serialize_field("property_id", self.0.property_id())?;
        iterator_field!(map, "restriction_ids", self.0.restriction_ids());
        map.serialize_field("result", &self.0.result())?;
        map.serialize_field("schema", self.0.schema())?;
        iterator_field!(map, "section_ids", self.0.section_ids());
        map.serialize_field("snapshot_id", self.0.snapshot_id())?;
        iterator_field!(map, "source_ids", self.0.source_ids());
        iterator_field!(map, "verification_ids", self.0.verification_ids());
        map.end()
    }
}

struct BorrowedCandidateProjectionV4<'a, 'b>(
    &'b reviewgraphen_core::BorrowedGlobalCandidateProjectionV4<'a>,
);
impl Serialize for BorrowedCandidateProjectionV4<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_struct("GlobalCandidateV4", 15)?;
        iterator_field!(map, "claim_ids", self.0.claim_ids());
        map.serialize_field("cover_id", self.0.cover_id())?;
        iterator_field!(map, "decision_ids", self.0.decision_ids());
        iterator_field!(map, "evidence_ids", self.0.evidence_ids());
        iterator_field!(map, "finding_ids", self.0.finding_ids());
        map.serialize_field("id", self.0.id())?;
        map.serialize_field("invariant_id", self.0.invariant_id())?;
        map.serialize_field("property_id", self.0.property_id())?;
        iterator_field!(
            map,
            "qualification_source_ids",
            self.0.qualification_source_ids()
        );
        iterator_field!(map, "required_section_ids", self.0.required_section_ids());
        iterator_field!(map, "restriction_ids", self.0.restriction_ids());
        map.serialize_field("schema", self.0.schema())?;
        iterator_field!(map, "source_ids", self.0.source_ids());
        iterator_field!(map, "verification_ids", self.0.verification_ids());
        map.end()
    }
}

struct BorrowedObstructionProjectionV4<'a, 'b>(
    &'b reviewgraphen_core::BorrowedGluingObstructionProjectionV4<'a>,
);
impl Serialize for BorrowedObstructionProjectionV4<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_struct("GluingObstructionV4", 22)?;
        map.serialize_field("affected_invariant_id", self.0.affected_invariant_id())?;
        map.serialize_field("assignment_key", self.0.assignment_key())?;
        map.serialize_field("attempt_id", self.0.attempt_id())?;
        iterator_field!(map, "blocks", self.0.blocks());
        iterator_field!(map, "claim_ids", self.0.claim_ids());
        iterator_field!(
            map,
            "conflicting_context_ids",
            self.0.conflicting_context_ids()
        );
        iterator_field!(map, "decision_ids", self.0.decision_ids());
        iterator_field!(map, "evidence_ids", self.0.evidence_ids());
        iterator_field!(map, "finding_ids", self.0.finding_ids());
        map.serialize_field("human_decision_required", &self.0.human_decision_required())?;
        map.serialize_field("id", self.0.id())?;
        map.serialize_field("kind", &self.0.kind())?;
        map.serialize_field("left_assignment_value", &self.0.left_assignment_value())?;
        iterator_field!(map, "overlap_member_ids", self.0.overlap_member_ids());
        map.serialize_field("required_resolution", &self.0.required_resolution())?;
        map.serialize_field("right_assignment_value", &self.0.right_assignment_value())?;
        map.serialize_field("schema", self.0.schema())?;
        iterator_field!(map, "section_ids", self.0.section_ids());
        map.serialize_field("severity", &self.0.severity())?;
        iterator_field!(map, "source_ids", self.0.source_ids());
        iterator_field!(map, "verification_ids", self.0.verification_ids());
        map.end()
    }
}

struct BorrowedExecutionOutcome<'a>(reviewgraphen_core::BorrowedExecutionOutcomeProjectionV4<'a>);
impl Serialize for BorrowedExecutionOutcome<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        let mut failure = None;
        let mut first = true;
        self.0.visit_fields(|key, value| {
            if failure.is_some() {
                return;
            }
            if let Err(error) = map.serialize_entry(key, &value) {
                failure = Some(error);
                return;
            }
            if first {
                first = false;
                if let Err(error) = map.serialize_entry("kind", self.0.kind()) {
                    failure = Some(error);
                }
            }
        });
        if let Some(error) = failure {
            return Err(error);
        }
        if first {
            map.serialize_entry("kind", self.0.kind())?;
        }
        map.end()
    }
}

fn phase0_evidence_row(
    envelope: &impl ProjectionEventMetadataV5,
    value: &reviewgraphen_core::BorrowedEvidenceProjectionV3<'_>,
) -> Result<BorrowedRowCharge, IndexError> {
    let mut row = phase0_event_base(envelope)?;
    row.direct("evidence_id", value.id())?;
    row.direct("schema", &value.schema())?;
    row.direct("kind", &value.kind())?;
    row.direct("snapshot_id", value.snapshot_id())?;
    row.canonical_string(
        "subject_ids_canonical_json",
        &BorrowedIteratorSequence(RefCell::new(value.subject_ids())),
    )?;
    row.direct("descriptor_id", &value.descriptor_id())?;
    row.direct("procedure_version", &value.procedure_version())?;
    row.direct("input_registration_id", value.input_registration_id())?;
    row.direct("output_registration_id", value.output_registration_id())?;
    row.direct("observation", &value.observation())?;
    row.hash_shape("body_hash")?;
    row.finish()
}

fn phase0_binding_row(
    envelope: &impl ProjectionEventMetadataV5,
    value: &reviewgraphen_core::BorrowedEvidenceBindingProjectionV3<'_>,
) -> Result<BorrowedRowCharge, IndexError> {
    let mut row = phase0_event_base(envelope)?;
    row.direct("binding_id", value.id())?;
    row.direct("schema", &value.schema())?;
    row.direct("claim_id", value.claim_id())?;
    row.direct("evidence_id", value.evidence_id())?;
    row.direct("relation", &value.relation())?;
    row.direct("property_id", value.property_id())?;
    row.hash_shape("body_hash")?;
    row.finish()
}

fn phase0_verification_row(
    envelope: &impl ProjectionEventMetadataV5,
    value: &reviewgraphen_core::BorrowedVerificationProjectionV3<'_>,
) -> Result<BorrowedRowCharge, IndexError> {
    let mut row = phase0_event_base(envelope)?;
    row.direct("verification_id", value.id())?;
    row.direct("schema", value.schema())?;
    row.direct("claim_id", value.claim_id())?;
    row.direct("descriptor_id", value.descriptor_id())?;
    row.direct("procedure_version", value.procedure_version())?;
    row.direct("input_registration_id", value.input_registration_id())?;
    row.direct("output_registration_id", value.output_registration_id())?;
    row.canonical_string(
        "evidence_ids_canonical_json",
        &BorrowedIteratorSequence(RefCell::new(value.evidence_ids())),
    )?;
    row.direct("outcome", value.outcome())?;
    row.canonical_string(
        "limitations_canonical_json",
        &BorrowedIteratorSequence(RefCell::new(value.limitations())),
    )?;
    row.hash_shape("body_hash")?;
    row.finish()
}

fn phase0_decision_row(
    envelope: &impl ProjectionEventMetadataV5,
    value: &reviewgraphen_core::BorrowedDecisionProjectionV3<'_>,
) -> Result<BorrowedRowCharge, IndexError> {
    let mut row = phase0_event_base(envelope)?;
    row.direct("decision_id", value.id())?;
    row.direct("schema", value.schema())?;
    row.direct("policy_revision_hash", value.policy_revision_hash())?;
    row.direct("run_id", value.run_id())?;
    row.direct("universe_id", value.universe_id())?;
    row.direct("claim_id", value.claim_id())?;
    row.direct("property_id", value.property_id())?;
    row.direct("outcome", value.outcome())?;
    row.direct("actor", value.actor())?;
    row.direct("authority_id", value.authority_id())?;
    row.direct("snapshot_id", value.snapshot_id())?;
    row.canonical_string(
        "source_ids_canonical_json",
        &BorrowedIteratorSequence(RefCell::new(value.source_ids())),
    )?;
    row.direct("rationale", value.rationale())?;
    row.direct("issued_at", value.issued_at())?;
    row.direct("expires_at", &value.expires_at())?;
    row.hash_shape("body_hash")?;
    row.finish()
}

fn phase0_finding_row(
    envelope: &impl ProjectionEventMetadataV5,
    value: &reviewgraphen_core::BorrowedFindingProjectionV3<'_>,
) -> Result<BorrowedRowCharge, IndexError> {
    let mut row = phase0_event_base(envelope)?;
    row.direct("finding_id", value.id())?;
    row.direct("schema", value.schema())?;
    row.direct("projection_descriptor_id", value.projection_descriptor_id())?;
    row.direct("claim_id", value.claim_id())?;
    row.direct("status", value.status())?;
    row.canonical_string(
        "evidence_ids_canonical_json",
        &BorrowedIteratorSequence(RefCell::new(value.evidence_ids())),
    )?;
    row.canonical_string(
        "verification_ids_canonical_json",
        &BorrowedIteratorSequence(RefCell::new(value.verification_ids())),
    )?;
    row.direct("decision_id", &value.decision_id())?;
    row.direct("supersedes_finding_id", &value.supersedes_finding_id())?;
    row.hash_shape("body_hash")?;
    row.finish()
}

fn phase0_assessment_row(
    value: &reviewgraphen_core::BorrowedClaimAssessmentProjectionV4<'_>,
    confirmed_event_sequence: u64,
) -> Result<BorrowedRowCharge, IndexError> {
    let mut row = BorrowedRowCharge::new();
    row.direct("claim_id", value.claim_id())?;
    row.direct("disposition", &value.disposition())?;
    row.direct("review_status", &value.review_status())?;
    row.canonical_string(
        "binding_ids_canonical_json",
        &BorrowedIteratorSequence(RefCell::new(value.binding_ids())),
    )?;
    row.canonical_string(
        "evidence_ids_canonical_json",
        &BorrowedIteratorSequence(RefCell::new(value.evidence_ids())),
    )?;
    row.canonical_string(
        "verification_ids_canonical_json",
        &BorrowedIteratorSequence(RefCell::new(value.verification_ids())),
    )?;
    row.canonical_string(
        "decision_ids_canonical_json",
        &BorrowedIteratorSequence(RefCell::new(value.decision_ids())),
    )?;
    row.canonical_string(
        "finding_ids_canonical_json",
        &BorrowedIteratorSequence(RefCell::new(value.finding_ids())),
    )?;
    row.direct("active_decision_id", &value.active_decision_id())?;
    row.direct("current_finding_id", &value.current_finding_id())?;
    row.direct("decision_conflict", &value.decision_conflict())?;
    row.direct("confirmed_event_sequence", &confirmed_event_sequence)?;
    row.finish()
}

fn canonical_iterator_text<I>(iterator: I) -> Result<String, IndexError>
where
    I: Iterator,
    I::Item: Serialize,
{
    let bytes = canonical_json(&BorrowedIteratorSequence(RefCell::new(iterator)))
        .map_err(|_| IndexError::ProjectionContractViolation)?;
    String::from_utf8(bytes).map_err(|_| IndexError::ProjectionContractViolation)
}

fn project_claim_assessment_v4(
    value: reviewgraphen_core::BorrowedClaimAssessmentProjectionV4<'_>,
    confirmed_event_sequence: u64,
) -> Result<IndexClaimAssessmentV3AtV5, IndexError> {
    Ok(IndexClaimAssessmentV3AtV5 {
        claim_id: value.claim_id().clone(),
        disposition: super::serialized_enum(&value.disposition())?,
        review_status: super::serialized_enum(&value.review_status())?,
        binding_ids_canonical_json: canonical_iterator_text(value.binding_ids())?,
        evidence_ids_canonical_json: canonical_iterator_text(value.evidence_ids())?,
        verification_ids_canonical_json: canonical_iterator_text(value.verification_ids())?,
        decision_ids_canonical_json: canonical_iterator_text(value.decision_ids())?,
        finding_ids_canonical_json: canonical_iterator_text(value.finding_ids())?,
        active_decision_id: value.active_decision_id().cloned(),
        current_finding_id: value.current_finding_id().cloned(),
        decision_conflict: value.decision_conflict(),
        confirmed_event_sequence,
    })
}

#[derive(Clone, Copy, Default)]
pub(crate) struct ReplayPayloadChargeV5 {
    arrays: [ArrayChargeV5; V5_ARRAY_FIELDS.len()],
    rows: u64,
    integer_cells: u64,
    text_bytes: u64,
    max_cas_bytes: u64,
    max_event_line_bytes: u64,
}

/// Allocation-free Store accounting retained from the successful Core replay.
/// It contains no projected DTO, ID, hash, JSON value, String, or row vector.
#[derive(Clone, Copy, Default)]
pub(crate) struct ReplayProjectionChargeV5 {
    pub(crate) charge: ReplayPayloadChargeV5,
    pub(crate) confirmed_offset: u64,
    pub(crate) event_count: u64,
}

impl ReplayProjectionChargeV5 {
    pub(crate) fn observe(
        &mut self,
        metadata: BorrowedV4EventMetadata<'_>,
        payload: CoreBorrowedProjectionPayloadV4<'_>,
    ) -> Result<(), IndexError> {
        self.confirmed_offset = self
            .confirmed_offset
            .checked_add(metadata.canonical_line_bytes())
            .ok_or(IndexError::IntegerOutOfRange)?;
        self.event_count = self
            .event_count
            .checked_add(1)
            .ok_or(IndexError::IntegerOutOfRange)?;
        self.charge.observe(metadata, payload)
    }
}

/// Store-owned rows materialized only after exact phase-0 admission from
/// Core's already validated, no-second-decode projection traversal.
#[derive(Default)]
pub(crate) struct ReplayProjectionRowsV5 {
    pub(crate) confirmed_offset: u64,
    pub(crate) events: Vec<IndexEvent>,
    pub(crate) obligation_lifecycle: Vec<IndexObligationLifecycle>,
    pub(crate) executions: Vec<IndexExecution>,
    pub(crate) claims: Vec<IndexClaim>,
    pub(crate) artifact_registrations: Vec<IndexArtifactRegistrationV5>,
    pub(crate) snapshot_sources: Vec<IndexSnapshotSource>,
    pub(crate) context_envelopes: Vec<IndexContextEnvelope>,
    pub(crate) review_plans: Vec<IndexReviewPlan>,
    pub(crate) evidence: Vec<IndexEvidenceV3AtV5>,
    pub(crate) evidence_bindings: Vec<IndexEvidenceBindingV3AtV5>,
    pub(crate) verifications: Vec<IndexVerificationV3AtV5>,
    pub(crate) decisions: Vec<IndexDecisionV3AtV5>,
    pub(crate) findings: Vec<IndexFindingV3AtV5>,
    pub(crate) artifact_registrations_v4: Vec<ArtifactRegistrationV4IndexItem>,
    pub(crate) gluing_input_descriptors: Vec<GluingInputDescriptorV4IndexItem>,
    pub(crate) context_covers: Vec<ContextCoverV4IndexItem>,
    pub(crate) sections: Vec<SectionV4IndexItem>,
    pub(crate) restrictions: Vec<RestrictionV4IndexItem>,
    pub(crate) gluing_attempts: Vec<GluingAttemptV4IndexItem>,
    pub(crate) global_candidates: Vec<GlobalCandidateV4IndexItem>,
    pub(crate) gluing_obstructions: Vec<GluingObstructionV4IndexItem>,
}

fn canonical_projection_text<T: Serialize>(value: &T) -> Result<String, IndexError> {
    String::from_utf8(canonical_json(value).map_err(|_| IndexError::ProjectionContractViolation)?)
        .map_err(|_| IndexError::ProjectionContractViolation)
}

fn ordered_projection_text<T: Serialize>(value: &T) -> Result<String, IndexError> {
    String::from_utf8(
        serde_json::to_vec(value).map_err(|_| IndexError::ProjectionContractViolation)?,
    )
    .map_err(|_| IndexError::ProjectionContractViolation)
}

fn project_execution_opaque(
    metadata: &BorrowedV4EventMetadata<'_>,
    value: &reviewgraphen_core::BorrowedReviewExecutionProjectionV4<'_>,
) -> Result<IndexExecution, IndexError> {
    let outcome = value.outcome();
    Ok(IndexExecution {
        event_sequence: metadata.sequence(),
        event_id: metadata.id().clone(),
        execution_id: value.id().clone(),
        plan_id: value.plan_id().clone(),
        wave_id: value.wave_id().clone(),
        snapshot_id: value.snapshot_id().clone(),
        envelope_id: value.envelope_id().clone(),
        obligation_ids_canonical_json: canonical_projection_text(&BorrowedIteratorSequence(
            RefCell::new(value.obligation_ids()),
        ))?,
        reviewer_kind: value.reviewer_kind().to_owned(),
        reviewer_id: value.reviewer_id().to_owned(),
        provider: value.provider().map(str::to_owned),
        model: value.model().map(str::to_owned),
        model_revision: value.model_revision().map(str::to_owned),
        system_prompt_version: value.system_prompt_version().to_owned(),
        prompt_template_version: value.prompt_template_version().to_owned(),
        inference_settings_canonical_json: canonical_projection_text(&BorrowedExecutionSettings(
            RefCell::new(value.inference_settings()),
        ))?,
        tool_policy_version: value.tool_policy_version().to_owned(),
        tool_calls_canonical_json: "[]".to_owned(),
        attempt: value.attempt(),
        raw_registration_id: value.raw_artifact_registration_id().clone(),
        raw_hash: value.raw_artifact_hash().clone(),
        parsed_claim_ids_canonical_json: canonical_projection_text(&BorrowedIteratorSequence(
            RefCell::new(value.parsed_claim_ids()),
        ))?,
        outcome_kind: outcome.kind().to_owned(),
        outcome_canonical_json: canonical_projection_text(&BorrowedExecutionOutcome(outcome))?,
        identity_body_hash: value
            .identity_body_hash()
            .map_err(|_| IndexError::ProjectionContractViolation)?,
        body_hash: value
            .body_hash()
            .map_err(|_| IndexError::ProjectionContractViolation)?,
    })
}

fn project_claim_opaque(
    metadata: &BorrowedV4EventMetadata<'_>,
    value: &reviewgraphen_core::BorrowedExecutionClaimProjectionV4<'_>,
) -> Result<IndexClaim, IndexError> {
    Ok(IndexClaim {
        event_sequence: metadata.sequence(),
        event_id: metadata.id().clone(),
        claim_id: value.id().clone(),
        execution_id: value.execution_id().clone(),
        obligation_ids_canonical_json: canonical_projection_text(&BorrowedIteratorSequence(
            RefCell::new(value.obligation_ids()),
        ))?,
        property_id: value.property_id().to_owned(),
        target_refs_canonical_json: canonical_projection_text(&BorrowedIteratorSequence(
            RefCell::new(value.target_refs()),
        ))?,
        polarity: value.polarity().to_owned(),
        disposition: value.disposition().to_owned(),
        summary: value.summary().to_owned(),
        source_ids_canonical_json: canonical_projection_text(&BorrowedIteratorSequence(
            RefCell::new(value.source_ids()),
        ))?,
        assumptions_canonical_json: canonical_projection_text(&BorrowedIteratorSequence(
            RefCell::new(value.assumptions()),
        ))?,
        requested_evidence_canonical_json: canonical_projection_text(&BorrowedIteratorSequence(
            RefCell::new(value.requested_evidence()),
        ))?,
        candidate_confidence_canonical_json: canonical_projection_text(
            &value.candidate_confidence(),
        )?,
        author_kind: value.author_kind().to_owned(),
        review_status: value.review_status().to_owned(),
        identity_body_hash: value
            .identity_body_hash()
            .map_err(|_| IndexError::ProjectionContractViolation)?,
        body_hash: value
            .body_hash()
            .map_err(|_| IndexError::ProjectionContractViolation)?,
    })
}

fn project_registration_v4_opaque(
    registrations: &mut Vec<ArtifactRegistrationV4IndexItem>,
    descriptors: &mut Vec<GluingInputDescriptorV4IndexItem>,
    metadata: &BorrowedV4EventMetadata<'_>,
    registration: &reviewgraphen_core::BorrowedArtifactRegistrationProjectionV4<'_>,
) -> Result<(), IndexError> {
    let descriptor = registration
        .descriptor()
        .ok_or(IndexError::ProjectionContractViolation)?;
    let source = BorrowedCanonicalArtifactSourceV4(registration.source());
    let descriptor_projection = BorrowedDescriptorProjectionV4(&descriptor);
    let (descriptor_value, descriptor_hash) = projection_value_and_hash(&descriptor_projection)?;
    registrations.push(ArtifactRegistrationV4IndexItem {
        event_sequence: metadata.sequence(),
        event_id: metadata.id().clone(),
        event_actor: metadata.actor().to_owned(),
        registration_id: registration.registration_id().clone(),
        schema: registration.schema().to_owned(),
        run_id: registration.run_id().clone(),
        cas_hash: registration.cas_hash().clone(),
        media_type: registration.media_type().to_owned(),
        size: registration.size(),
        sensitivity: super::serialized_enum(&registration.sensitivity())?,
        source_kind: registration.source().kind().to_owned(),
        source: serde_json::to_value(&source)
            .map_err(|_| IndexError::ProjectionContractViolation)?,
        descriptor_id: descriptor.id().clone(),
        body_hash: projection_hash(&BorrowedRegistrationProjectionV4(registration))?,
    });
    descriptors.push(GluingInputDescriptorV4IndexItem {
        event_sequence: metadata.sequence(),
        event_id: metadata.id().clone(),
        descriptor: descriptor_value,
        registration_id: registration.registration_id().clone(),
        descriptor_hash: registration.cas_hash().clone(),
        descriptor_size: registration.size(),
        body_hash: descriptor_hash,
    });
    Ok(())
}

fn project_bundle_v4_opaque(
    rows: &mut ReplayProjectionRowsV5,
    metadata: &BorrowedV4EventMetadata<'_>,
    bundle: &reviewgraphen_core::BorrowedGluingBundleProjectionV4<'_>,
) -> Result<(), IndexError> {
    let sequence = metadata.sequence();
    let event_id = metadata.id().clone();
    let cover = bundle.cover();
    let (cover_value, cover_hash) = projection_value_and_hash(&BorrowedCoverProjectionV4(&cover))?;
    rows.context_covers.push(ContextCoverV4IndexItem {
        event_sequence: sequence,
        event_id: event_id.clone(),
        cover: cover_value,
        body_hash: cover_hash,
    });
    for section in bundle.sections() {
        let (value, hash) = projection_value_and_hash(&BorrowedSectionProjectionValueV4(&section))?;
        rows.sections.push(SectionV4IndexItem {
            event_sequence: sequence,
            event_id: event_id.clone(),
            section: value,
            body_hash: hash,
        });
    }
    let attempt = bundle.attempt();
    let attempt_id = attempt.id().clone();
    for restriction in bundle.restrictions() {
        let (value, hash) =
            projection_value_and_hash(&BorrowedRestrictionProjectionValueV4(&restriction))?;
        rows.restrictions.push(RestrictionV4IndexItem {
            event_sequence: sequence,
            event_id: event_id.clone(),
            attempt_id: attempt_id.clone(),
            restriction: value,
            body_hash: hash,
        });
    }
    let (attempt_value, attempt_hash) =
        projection_value_and_hash(&BorrowedAttemptProjectionV4(&attempt))?;
    rows.gluing_attempts.push(GluingAttemptV4IndexItem {
        event_sequence: sequence,
        event_id: event_id.clone(),
        attempt: attempt_value,
        body_hash: attempt_hash,
    });
    if let Some(candidate) = bundle.global_candidate() {
        let (value, hash) = projection_value_and_hash(&BorrowedCandidateProjectionV4(&candidate))?;
        rows.global_candidates.push(GlobalCandidateV4IndexItem {
            event_sequence: sequence,
            event_id: event_id.clone(),
            attempt_id,
            candidate: value,
            body_hash: hash,
        });
    }
    if let Some(obstruction) = bundle.obstruction() {
        let (value, hash) =
            projection_value_and_hash(&BorrowedObstructionProjectionV4(&obstruction))?;
        rows.gluing_obstructions.push(GluingObstructionV4IndexItem {
            event_sequence: sequence,
            event_id,
            obstruction: value,
            body_hash: hash,
        });
    }
    Ok(())
}

impl ReplayProjectionRowsV5 {
    fn with_capacities(counts: &[u64; V5_ARRAY_FIELDS.len()]) -> Result<Self, IndexError> {
        Ok(Self {
            confirmed_offset: 0,
            events: reserved_vec(counts[EVENTS])?,
            obligation_lifecycle: reserved_vec(counts[OBLIGATION_LIFECYCLE])?,
            executions: reserved_vec(counts[EXECUTIONS])?,
            claims: reserved_vec(counts[CLAIMS])?,
            artifact_registrations: reserved_vec(counts[REGISTRATIONS])?,
            snapshot_sources: reserved_vec(counts[SNAPSHOT_SOURCES])?,
            context_envelopes: reserved_vec(counts[CONTEXT_ENVELOPES])?,
            review_plans: reserved_vec(counts[REVIEW_PLANS])?,
            evidence: reserved_vec(counts[EVIDENCE])?,
            evidence_bindings: reserved_vec(counts[EVIDENCE_BINDINGS])?,
            verifications: reserved_vec(counts[VERIFICATIONS])?,
            decisions: reserved_vec(counts[DECISIONS])?,
            findings: reserved_vec(counts[FINDINGS])?,
            artifact_registrations_v4: reserved_vec(counts[REGISTRATIONS_V4])?,
            gluing_input_descriptors: reserved_vec(counts[GLUING_DESCRIPTORS])?,
            context_covers: reserved_vec(counts[CONTEXT_COVERS])?,
            sections: reserved_vec(counts[SECTIONS])?,
            restrictions: reserved_vec(counts[RESTRICTIONS])?,
            gluing_attempts: reserved_vec(counts[GLUING_ATTEMPTS])?,
            global_candidates: reserved_vec(counts[GLOBAL_CANDIDATES])?,
            gluing_obstructions: reserved_vec(counts[GLUING_OBSTRUCTIONS])?,
        })
    }

    fn validate(
        &self,
        counts: &[u64; V5_ARRAY_FIELDS.len()],
        confirmed_offset: u64,
        event_count: u64,
    ) -> Result<(), IndexError> {
        let lengths = [
            (EVENTS, self.events.len()),
            (OBLIGATION_LIFECYCLE, self.obligation_lifecycle.len()),
            (EXECUTIONS, self.executions.len()),
            (CLAIMS, self.claims.len()),
            (REGISTRATIONS, self.artifact_registrations.len()),
            (SNAPSHOT_SOURCES, self.snapshot_sources.len()),
            (CONTEXT_ENVELOPES, self.context_envelopes.len()),
            (REVIEW_PLANS, self.review_plans.len()),
            (EVIDENCE, self.evidence.len()),
            (EVIDENCE_BINDINGS, self.evidence_bindings.len()),
            (VERIFICATIONS, self.verifications.len()),
            (DECISIONS, self.decisions.len()),
            (FINDINGS, self.findings.len()),
            (REGISTRATIONS_V4, self.artifact_registrations_v4.len()),
            (GLUING_DESCRIPTORS, self.gluing_input_descriptors.len()),
            (CONTEXT_COVERS, self.context_covers.len()),
            (SECTIONS, self.sections.len()),
            (RESTRICTIONS, self.restrictions.len()),
            (GLUING_ATTEMPTS, self.gluing_attempts.len()),
            (GLOBAL_CANDIDATES, self.global_candidates.len()),
            (GLUING_OBSTRUCTIONS, self.gluing_obstructions.len()),
        ];
        if self.confirmed_offset != confirmed_offset
            || u64::try_from(self.events.len()).map_err(|_| IndexError::IntegerOutOfRange)?
                != event_count
            || lengths.iter().any(|(index, length)| {
                u64::try_from(*length)
                    .map(|length| length != counts[*index])
                    .unwrap_or(true)
            })
        {
            return Err(IndexError::ProjectionContractViolation);
        }
        Ok(())
    }

    pub(crate) fn observe(
        &mut self,
        metadata: BorrowedV4EventMetadata<'_>,
        payload: CoreBorrowedProjectionPayloadV4<'_>,
    ) -> Result<(), IndexError> {
        let view = payload.view();
        let kind = match &view {
            BorrowedProjectionPayloadV4::RunGenesisManifestV4(_) => "run_genesis_manifest",
            BorrowedProjectionPayloadV4::ObligationTransition(_) => "obligation_transition",
            BorrowedProjectionPayloadV4::ArtifactRegisteredV3(_) => "artifact_registered",
            BorrowedProjectionPayloadV4::SnapshotSourcesRecorded(_) => "snapshot_sources_recorded",
            BorrowedProjectionPayloadV4::ReviewPlanRecorded(_) => "review_plan_recorded",
            BorrowedProjectionPayloadV4::ContextEnvelopeProjected(_) => {
                "context_envelope_projected"
            }
            BorrowedProjectionPayloadV4::ReviewExecutionRecorded(_) => "review_execution_recorded",
            BorrowedProjectionPayloadV4::EvidenceRecordedV3(_) => "evidence_recorded_v3",
            BorrowedProjectionPayloadV4::EvidenceBoundV3(_) => "evidence_bound_v3",
            BorrowedProjectionPayloadV4::VerificationRecordedV3(_) => "verification_recorded_v3",
            BorrowedProjectionPayloadV4::DecisionRecordedV3(_) => "decision_recorded_v3",
            BorrowedProjectionPayloadV4::FindingRecordedV3(_) => "finding_recorded_v3",
            BorrowedProjectionPayloadV4::ArtifactRegisteredV4(_) => "artifact_registered_v4",
            BorrowedProjectionPayloadV4::GluingBundleRecordedV4(_) => "gluing_bundle_recorded_v4",
        };
        self.confirmed_offset = self
            .confirmed_offset
            .checked_add(metadata.canonical_line_bytes())
            .ok_or(IndexError::IntegerOutOfRange)?;
        self.events.push(IndexEvent {
            sequence: metadata.sequence(),
            event_id: metadata.id().clone(),
            schema: metadata.schema().to_owned(),
            event_hash: metadata.event_hash().clone(),
            payload_hash: metadata.payload_hash().clone(),
            payload_kind: kind.to_owned(),
            actor: metadata.actor().to_owned(),
            logical_time: metadata.logical_time(),
        });
        match view {
            BorrowedProjectionPayloadV4::RunGenesisManifestV4(value) => {
                self.artifact_registrations
                    .push(project_registration_opaque_v3(
                        &metadata,
                        &value.genesis_artifact(),
                    )?);
            }
            BorrowedProjectionPayloadV4::ObligationTransition(value) => {
                self.obligation_lifecycle.push(IndexObligationLifecycle {
                    event_sequence: metadata.sequence(),
                    event_id: metadata.id().clone(),
                    obligation_id: value.obligation_id().clone(),
                    next_lifecycle: super::serialized_enum(&value.next())?,
                });
            }
            BorrowedProjectionPayloadV4::ArtifactRegisteredV3(value) => {
                self.artifact_registrations
                    .push(project_registration_opaque_v3(&metadata, &value)?);
            }
            BorrowedProjectionPayloadV4::SnapshotSourcesRecorded(value) => {
                for entry in value.entries() {
                    self.snapshot_sources.push(IndexSnapshotSource {
                        event_sequence: metadata.sequence(),
                        event_id: metadata.id().clone(),
                        snapshot_id: value.snapshot_id().clone(),
                        artifact_id: entry.artifact_id().clone(),
                        registration_id: entry.registration_id().clone(),
                        path: entry.path().to_owned(),
                        content_hash: entry.content_hash().clone(),
                        cas_hash: entry.cas_hash().clone(),
                        line_count: entry.line_count(),
                    });
                }
            }
            BorrowedProjectionPayloadV4::ReviewPlanRecorded(value) => {
                let budget = canonical_projection_text(&BorrowedPlanBudget(value.budget()))?;
                self.review_plans.push(IndexReviewPlan {
                    event_sequence: metadata.sequence(),
                    event_id: metadata.id().clone(),
                    plan_id: value.id().clone(),
                    universe_id: value.universe_id().clone(),
                    snapshot_id: value.snapshot_id().clone(),
                    planner_input_hash: value.planner_input_hash().clone(),
                    planner_policy_version: value.planner_policy_version().to_owned(),
                    planner_policy_hash: value.planner_policy_hash().clone(),
                    budget_hash: ContentHash::sha256(budget.as_bytes()),
                    budget_canonical_json: budget,
                    risk_breakdown_canonical_json: canonical_projection_text(
                        &BorrowedPlanRiskProjection(RefCell::new(value.risk_breakdown())),
                    )?,
                    waves_canonical_json: canonical_projection_text(&BorrowedPlanWavesProjection(
                        RefCell::new(value.waves()),
                    ))?,
                    deferred_canonical_json: canonical_projection_text(
                        &BorrowedPlanDeferredProjection(RefCell::new(value.deferred())),
                    )?,
                    identity_body_hash: value
                        .identity_body_hash()
                        .map_err(|_| IndexError::ProjectionContractViolation)?,
                    body_hash: value
                        .body_hash()
                        .map_err(|_| IndexError::ProjectionContractViolation)?,
                });
            }
            BorrowedProjectionPayloadV4::ContextEnvelopeProjected(value) => {
                self.context_envelopes.push(IndexContextEnvelope {
                    event_sequence: metadata.sequence(),
                    event_id: metadata.id().clone(),
                    envelope_id: value.id().clone(),
                    snapshot_id: value.snapshot_id().clone(),
                    context_policy_version: value.projection_policy_version().to_owned(),
                    context_policy_hash: value.context_policy_hash().clone(),
                    candidate_ids_canonical_json: canonical_projection_text(
                        &BorrowedIteratorSequence(RefCell::new(value.candidate_source_ids())),
                    )?,
                    obligation_ids_canonical_json: canonical_projection_text(
                        &BorrowedIteratorSequence(RefCell::new(value.obligation_ids())),
                    )?,
                    context_policy_canonical_json: ordered_projection_text(
                        &BorrowedContextPolicy(value.context_policy()),
                    )?,
                    included_sources_canonical_json: canonical_projection_text(
                        &BorrowedIncludedSources(RefCell::new(value.included_sources())),
                    )?,
                    excluded_sources_canonical_json: canonical_projection_text(
                        &BorrowedExcludedSources(RefCell::new(value.excluded_sources())),
                    )?,
                    unknowns_canonical_json: canonical_projection_text(&BorrowedContextUnknowns(
                        RefCell::new(value.unknowns()),
                    ))?,
                    assumptions_canonical_json: canonical_projection_text(
                        &BorrowedIteratorSequence(RefCell::new(value.assumptions())),
                    )?,
                    losses_canonical_json: canonical_projection_text(&BorrowedContextLosses(
                        RefCell::new(value.losses()),
                    ))?,
                    projection_hash: value.projection_hash().clone(),
                    body_hash: value
                        .body_hash()
                        .map_err(|_| IndexError::ProjectionContractViolation)?,
                });
            }
            BorrowedProjectionPayloadV4::ReviewExecutionRecorded(value) => {
                self.executions
                    .push(project_execution_opaque(&metadata, &value)?);
                for claim in value.claims() {
                    self.claims.push(project_claim_opaque(&metadata, &claim)?);
                }
            }
            BorrowedProjectionPayloadV4::EvidenceRecordedV3(value) => {
                self.evidence.push(IndexEvidenceV3AtV5 {
                    event_sequence: metadata.sequence(),
                    event_id: metadata.id().clone(),
                    evidence_id: value.id().clone(),
                    schema: value.schema().to_owned(),
                    kind: value.kind().to_owned(),
                    snapshot_id: value.snapshot_id().clone(),
                    subject_ids_canonical_json: canonical_projection_text(
                        &BorrowedIteratorSequence(RefCell::new(value.subject_ids())),
                    )?,
                    descriptor_id: value.descriptor_id().to_owned(),
                    procedure_version: value.procedure_version().to_owned(),
                    input_registration_id: value.input_registration_id().clone(),
                    output_registration_id: value.output_registration_id().clone(),
                    observation: value.observation().to_owned(),
                    body_hash: value
                        .body_hash()
                        .map_err(|_| IndexError::ProjectionContractViolation)?,
                })
            }
            BorrowedProjectionPayloadV4::EvidenceBoundV3(value) => {
                self.evidence_bindings.push(IndexEvidenceBindingV3AtV5 {
                    event_sequence: metadata.sequence(),
                    event_id: metadata.id().clone(),
                    binding_id: value.id().clone(),
                    schema: value.schema().to_owned(),
                    claim_id: value.claim_id().clone(),
                    evidence_id: value.evidence_id().clone(),
                    relation: value.relation().to_owned(),
                    property_id: value.property_id().to_owned(),
                    body_hash: value
                        .body_hash()
                        .map_err(|_| IndexError::ProjectionContractViolation)?,
                })
            }
            BorrowedProjectionPayloadV4::VerificationRecordedV3(value) => {
                self.verifications.push(IndexVerificationV3AtV5 {
                    event_sequence: metadata.sequence(),
                    event_id: metadata.id().clone(),
                    verification_id: value.id().clone(),
                    schema: value.schema().to_owned(),
                    claim_id: value.claim_id().clone(),
                    descriptor_id: value.descriptor_id().to_owned(),
                    procedure_version: value.procedure_version().to_owned(),
                    input_registration_id: value.input_registration_id().clone(),
                    output_registration_id: value.output_registration_id().clone(),
                    evidence_ids_canonical_json: canonical_projection_text(
                        &BorrowedIteratorSequence(RefCell::new(value.evidence_ids())),
                    )?,
                    outcome: value.outcome().to_owned(),
                    limitations_canonical_json: canonical_projection_text(
                        &BorrowedIteratorSequence(RefCell::new(value.limitations())),
                    )?,
                    body_hash: value
                        .body_hash()
                        .map_err(|_| IndexError::ProjectionContractViolation)?,
                })
            }
            BorrowedProjectionPayloadV4::DecisionRecordedV3(value) => {
                self.decisions.push(IndexDecisionV3AtV5 {
                    event_sequence: metadata.sequence(),
                    event_id: metadata.id().clone(),
                    decision_id: value.id().clone(),
                    schema: value.schema().to_owned(),
                    policy_revision_hash: value.policy_revision_hash().clone(),
                    run_id: value.run_id().clone(),
                    universe_id: value.universe_id().clone(),
                    claim_id: value.claim_id().clone(),
                    property_id: value.property_id().to_owned(),
                    outcome: value.outcome().to_owned(),
                    actor: value.actor().to_owned(),
                    authority_id: value.authority_id().to_owned(),
                    snapshot_id: value.snapshot_id().clone(),
                    source_ids_canonical_json: canonical_projection_text(
                        &BorrowedIteratorSequence(RefCell::new(value.source_ids())),
                    )?,
                    rationale: value.rationale().to_owned(),
                    issued_at: value.issued_at().to_owned(),
                    expires_at: value.expires_at().map(str::to_owned),
                    body_hash: value
                        .body_hash()
                        .map_err(|_| IndexError::ProjectionContractViolation)?,
                })
            }
            BorrowedProjectionPayloadV4::FindingRecordedV3(value) => {
                self.findings.push(IndexFindingV3AtV5 {
                    event_sequence: metadata.sequence(),
                    event_id: metadata.id().clone(),
                    finding_id: value.id().clone(),
                    schema: value.schema().to_owned(),
                    projection_descriptor_id: value.projection_descriptor_id().to_owned(),
                    claim_id: value.claim_id().clone(),
                    status: value.status().to_owned(),
                    evidence_ids_canonical_json: canonical_projection_text(
                        &BorrowedIteratorSequence(RefCell::new(value.evidence_ids())),
                    )?,
                    verification_ids_canonical_json: canonical_projection_text(
                        &BorrowedIteratorSequence(RefCell::new(value.verification_ids())),
                    )?,
                    decision_id: value.decision_id().cloned(),
                    supersedes_finding_id: value.supersedes_finding_id().cloned(),
                    body_hash: value
                        .body_hash()
                        .map_err(|_| IndexError::ProjectionContractViolation)?,
                })
            }
            BorrowedProjectionPayloadV4::ArtifactRegisteredV4(value) => {
                project_registration_v4_opaque(
                    &mut self.artifact_registrations_v4,
                    &mut self.gluing_input_descriptors,
                    &metadata,
                    &value,
                )?;
            }
            BorrowedProjectionPayloadV4::GluingBundleRecordedV4(value) => {
                project_bundle_v4_opaque(self, &metadata, &value)?;
            }
        }
        Ok(())
    }
}

fn replay_charge_m5_registration(
    charge: &mut ReplayPayloadChargeV5,
    metadata: &BorrowedV4EventMetadata<'_>,
    registration: &reviewgraphen_core::BorrowedArtifactRegistrationProjectionV4<'_>,
) -> Result<(), IndexError> {
    let descriptor = registration
        .descriptor()
        .ok_or(IndexError::ProjectionContractViolation)?;
    let source = registration.source();
    let registration_row = OpaqueRegistrationV4Row {
        event_sequence: metadata.sequence(),
        event_id: metadata.id(),
        event_actor: metadata.actor(),
        registration_id: registration.registration_id(),
        schema: registration.schema(),
        run_id: registration.run_id(),
        cas_hash: registration.cas_hash(),
        media_type: registration.media_type(),
        size: registration.size(),
        sensitivity: registration.sensitivity(),
        source_kind: source.kind(),
        source: BorrowedCanonicalArtifactSourceV4(source),
        descriptor_id: descriptor.id(),
        body_hash: HashShape,
    };
    charge.max_cas_bytes = charge.max_cas_bytes.max(registration.size());
    charge.add_m5(REGISTRATIONS_V4, &registration_row, |integers, text| {
        charge_borrowed_event_tuple(metadata.id(), integers, text)?;
        charge_borrowed_dto_sql(
            &BorrowedRegistrationProjectionV4(registration),
            integers,
            text,
        )?;
        for value in [
            registration_row.source_kind,
            descriptor.id().as_str(),
            "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        ] {
            add_text_charge(text, value)?;
        }
        Ok(())
    })?;
    let descriptor_row = OpaqueDescriptorV4Row {
        event_sequence: metadata.sequence(),
        event_id: metadata.id(),
        descriptor: BorrowedDescriptorProjectionV4(&descriptor),
        registration_id: registration.registration_id(),
        descriptor_hash: registration.cas_hash(),
        descriptor_size: registration.size(),
        body_hash: HashShape,
    };
    charge.add_m5(GLUING_DESCRIPTORS, &descriptor_row, |integers, text| {
        charge_borrowed_event_tuple(metadata.id(), integers, text)?;
        charge_borrowed_dto_sql(&BorrowedDescriptorProjectionV4(&descriptor), integers, text)?;
        for value in [
            registration.registration_id().as_str(),
            registration.cas_hash().as_str(),
            "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        ] {
            add_text_charge(text, value)?;
        }
        *integers = integers
            .checked_add(1)
            .ok_or(IndexError::IntegerOutOfRange)?;
        Ok(())
    })
}

fn replay_charge_m5_bundle(
    charge: &mut ReplayPayloadChargeV5,
    metadata: &BorrowedV4EventMetadata<'_>,
    bundle: &reviewgraphen_core::BorrowedGluingBundleProjectionV4<'_>,
) -> Result<(), IndexError> {
    let event_id = metadata.id();
    let event_sequence = metadata.sequence();
    let cover_value = bundle.cover();
    let cover = OpaqueCoverV4Row {
        event_sequence,
        event_id,
        cover: BorrowedCoverProjectionV4(&cover_value),
        body_hash: HashShape,
    };
    charge.add_m5(CONTEXT_COVERS, &cover, |integers, text| {
        charge_borrowed_m5_dto_item(
            event_id,
            &BorrowedCoverProjectionV4(&cover_value),
            None,
            integers,
            text,
        )
    })?;
    for section in bundle.sections() {
        let row = OpaqueSectionV4Row {
            event_sequence,
            event_id,
            section: BorrowedSectionProjectionValueV4(&section),
            body_hash: HashShape,
        };
        charge.add_m5(SECTIONS, &row, |integers, text| {
            charge_borrowed_m5_dto_item(
                event_id,
                &BorrowedSectionProjectionValueV4(&section),
                None,
                integers,
                text,
            )
        })?;
    }
    let attempt = bundle.attempt();
    let attempt_id = attempt.id();
    for restriction in bundle.restrictions() {
        let row = OpaqueRestrictionV4Row {
            event_sequence,
            event_id,
            attempt_id,
            restriction: BorrowedRestrictionProjectionValueV4(&restriction),
            body_hash: HashShape,
        };
        charge.add_m5(RESTRICTIONS, &row, |integers, text| {
            charge_borrowed_m5_dto_item(
                event_id,
                &BorrowedRestrictionProjectionValueV4(&restriction),
                Some(attempt_id),
                integers,
                text,
            )
        })?;
    }
    let attempt_row = OpaqueAttemptV4Row {
        event_sequence,
        event_id,
        attempt: BorrowedAttemptProjectionV4(&attempt),
        body_hash: HashShape,
    };
    charge.add_m5(GLUING_ATTEMPTS, &attempt_row, |integers, text| {
        charge_borrowed_m5_dto_item(
            event_id,
            &BorrowedAttemptProjectionV4(&attempt),
            None,
            integers,
            text,
        )
    })?;
    if let Some(candidate) = bundle.global_candidate() {
        let row = OpaqueCandidateV4Row {
            event_sequence,
            event_id,
            attempt_id,
            candidate: BorrowedCandidateProjectionV4(&candidate),
            body_hash: HashShape,
        };
        charge.add_m5(GLOBAL_CANDIDATES, &row, |integers, text| {
            charge_borrowed_m5_dto_item(
                event_id,
                &BorrowedCandidateProjectionV4(&candidate),
                Some(attempt_id),
                integers,
                text,
            )
        })?;
    }
    if let Some(obstruction) = bundle.obstruction() {
        let row = OpaqueObstructionV4Row {
            event_sequence,
            event_id,
            obstruction: BorrowedObstructionProjectionV4(&obstruction),
            body_hash: HashShape,
        };
        charge.add_m5(GLUING_OBSTRUCTIONS, &row, |integers, text| {
            charge_borrowed_m5_dto_item(
                event_id,
                &BorrowedObstructionProjectionV4(&obstruction),
                None,
                integers,
                text,
            )
        })?;
    }
    Ok(())
}

impl ReplayPayloadChargeV5 {
    fn add(&mut self, index: usize, row: BorrowedRowCharge) -> Result<(), IndexError> {
        let array = self
            .arrays
            .get_mut(index)
            .ok_or(IndexError::ProjectionContractViolation)?;
        array.items = array
            .items
            .checked_add(1)
            .ok_or(IndexError::IntegerOutOfRange)?;
        array.json_items = array
            .json_items
            .checked_add(row.json_bytes)
            .ok_or(IndexError::IntegerOutOfRange)?;
        array.owned_items = array
            .owned_items
            .checked_add(row.owned_bytes)
            .and_then(|value| value.checked_add(8))
            .ok_or(IndexError::IntegerOutOfRange)?;
        self.rows = self
            .rows
            .checked_add(1)
            .ok_or(IndexError::IntegerOutOfRange)?;
        self.integer_cells = self
            .integer_cells
            .checked_add(row.integer_cells)
            .ok_or(IndexError::IntegerOutOfRange)?;
        self.text_bytes = self
            .text_bytes
            .checked_add(row.text_bytes)
            .ok_or(IndexError::IntegerOutOfRange)?;
        Ok(())
    }

    fn add_m5<T: Serialize>(
        &mut self,
        index: usize,
        row: &T,
        sql_charge: impl FnOnce(&mut u64, &mut u64) -> Result<(), IndexError>,
    ) -> Result<(), IndexError> {
        let mut integer_cells = 0;
        let mut text_bytes = 0;
        sql_charge(&mut integer_cells, &mut text_bytes)?;
        self.add(
            index,
            BorrowedRowCharge {
                json_bytes: json_length(row)?,
                owned_bytes: recursive_ownership_charge(row)?,
                integer_cells,
                text_bytes,
                fields: 1,
            },
        )
    }

    pub(crate) fn observe(
        &mut self,
        metadata: BorrowedV4EventMetadata<'_>,
        payload: CoreBorrowedProjectionPayloadV4<'_>,
    ) -> Result<(), IndexError> {
        self.max_event_line_bytes = self
            .max_event_line_bytes
            .max(metadata.canonical_line_bytes());
        let kind = match payload.view() {
            BorrowedProjectionPayloadV4::RunGenesisManifestV4(_) => "run_genesis_manifest",
            BorrowedProjectionPayloadV4::ObligationTransition(_) => "obligation_transition",
            BorrowedProjectionPayloadV4::ArtifactRegisteredV3(_) => "artifact_registered",
            BorrowedProjectionPayloadV4::SnapshotSourcesRecorded(_) => "snapshot_sources_recorded",
            BorrowedProjectionPayloadV4::ReviewPlanRecorded(_) => "review_plan_recorded",
            BorrowedProjectionPayloadV4::ContextEnvelopeProjected(_) => {
                "context_envelope_projected"
            }
            BorrowedProjectionPayloadV4::ReviewExecutionRecorded(_) => "review_execution_recorded",
            BorrowedProjectionPayloadV4::EvidenceRecordedV3(_) => "evidence_recorded_v3",
            BorrowedProjectionPayloadV4::EvidenceBoundV3(_) => "evidence_bound_v3",
            BorrowedProjectionPayloadV4::VerificationRecordedV3(_) => "verification_recorded_v3",
            BorrowedProjectionPayloadV4::DecisionRecordedV3(_) => "decision_recorded_v3",
            BorrowedProjectionPayloadV4::FindingRecordedV3(_) => "finding_recorded_v3",
            BorrowedProjectionPayloadV4::ArtifactRegisteredV4(_) => "artifact_registered_v4",
            BorrowedProjectionPayloadV4::GluingBundleRecordedV4(_) => "gluing_bundle_recorded_v4",
        };
        self.add(EVENTS, phase0_event_row(&metadata, kind)?)?;
        match payload.view() {
            BorrowedProjectionPayloadV4::RunGenesisManifestV4(value) => {
                let registration = value.genesis_artifact();
                self.max_cas_bytes = self.max_cas_bytes.max(registration.size());
                self.add(
                    REGISTRATIONS,
                    phase0_registration_v3(&metadata, &registration)?,
                )?;
            }
            BorrowedProjectionPayloadV4::ObligationTransition(value) => {
                let mut row = phase0_event_base(&metadata)?;
                row.direct("obligation_id", value.obligation_id())?;
                row.direct("next_lifecycle", &value.next())?;
                self.add(OBLIGATION_LIFECYCLE, row.finish()?)?;
            }
            BorrowedProjectionPayloadV4::ArtifactRegisteredV3(value) => {
                self.max_cas_bytes = self.max_cas_bytes.max(value.size());
                self.add(REGISTRATIONS, phase0_registration_v3(&metadata, &value)?)?;
            }
            BorrowedProjectionPayloadV4::SnapshotSourcesRecorded(value) => {
                for entry in value.entries() {
                    let mut row = phase0_event_base(&metadata)?;
                    row.direct("snapshot_id", value.snapshot_id())?;
                    row.direct("artifact_id", entry.artifact_id())?;
                    row.direct("registration_id", entry.registration_id())?;
                    row.direct("path", entry.path())?;
                    row.direct("content_hash", entry.content_hash())?;
                    row.direct("cas_hash", entry.cas_hash())?;
                    row.direct("line_count", &entry.line_count())?;
                    self.add(SNAPSHOT_SOURCES, row.finish()?)?;
                }
            }
            BorrowedProjectionPayloadV4::ReviewPlanRecorded(value) => {
                self.add(REVIEW_PLANS, phase0_plan_row(&metadata, &value)?)?;
            }
            BorrowedProjectionPayloadV4::ContextEnvelopeProjected(value) => {
                self.add(CONTEXT_ENVELOPES, phase0_context_row(&metadata, &value)?)?;
            }
            BorrowedProjectionPayloadV4::ReviewExecutionRecorded(value) => {
                self.add(EXECUTIONS, phase0_execution_row(&metadata, &value)?)?;
                for claim in value.claims() {
                    self.add(CLAIMS, phase0_claim_row(&metadata, &claim)?)?;
                }
            }
            BorrowedProjectionPayloadV4::EvidenceRecordedV3(value) => {
                self.add(EVIDENCE, phase0_evidence_row(&metadata, &value)?)?;
            }
            BorrowedProjectionPayloadV4::EvidenceBoundV3(value) => {
                self.add(EVIDENCE_BINDINGS, phase0_binding_row(&metadata, &value)?)?;
            }
            BorrowedProjectionPayloadV4::VerificationRecordedV3(value) => {
                self.add(VERIFICATIONS, phase0_verification_row(&metadata, &value)?)?;
            }
            BorrowedProjectionPayloadV4::DecisionRecordedV3(value) => {
                self.add(DECISIONS, phase0_decision_row(&metadata, &value)?)?;
            }
            BorrowedProjectionPayloadV4::FindingRecordedV3(value) => {
                self.add(FINDINGS, phase0_finding_row(&metadata, &value)?)?;
            }
            BorrowedProjectionPayloadV4::ArtifactRegisteredV4(value) => {
                replay_charge_m5_registration(self, &metadata, &value)?;
            }
            BorrowedProjectionPayloadV4::GluingBundleRecordedV4(value) => {
                replay_charge_m5_bundle(self, &metadata, &value)?;
            }
        }
        Ok(())
    }
}

fn phase0_verified_projection_v5<S: ProjectionSourceV5>(
    session: &S,
    genesis: &RunGenesisSnapshot,
    basis: &AuthorityReplayBasisV4,
    confirmed_offset: u64,
    limits: IndexLimits,
) -> Result<ProjectionPreflightV5, IndexError> {
    let replay = session.replay_projection()?;
    let event_count = replay.event_count;
    let marker = borrowed_index_marker_v5(basis, confirmed_offset, event_count);
    let mut charge = ProjectionChargeV5::new_borrowed(&marker, limits)?;

    for artifact in genesis.program_space().artifacts() {
        let mut row = BorrowedRowCharge::new();
        row.direct("object_id", &artifact.id)?;
        row.direct("object_kind", &artifact.kind)?;
        row.hash_shape("body_hash")?;
        charge.add_precomputed_array(PROGRAM_OBJECTS, row.finish()?)?;
    }
    for relation in genesis.program_space().relations() {
        let mut row = BorrowedRowCharge::new();
        row.direct("relation_id", &relation.id)?;
        row.direct("relation_kind", &relation.kind)?;
        row.direct("source_id", &relation.source_id)?;
        row.canonical_string("target_ids_canonical_json", &relation.target_ids)?;
        row.hash_shape("body_hash")?;
        charge.add_precomputed_array(PROGRAM_RELATIONS, row.finish()?)?;
    }
    let universe = genesis.universe();
    let mut universe_row = BorrowedRowCharge::new();
    universe_row.direct("universe_id", universe.id())?;
    universe_row.direct("snapshot_id", universe.snapshot_id())?;
    universe_row.direct("profile_id", universe.profile_id())?;
    universe_row.direct("rule_set_hash", universe.rule_set_hash())?;
    universe_row.direct("extractor_set_hash", universe.extractor_set_hash())?;
    universe_row.direct("policy_version", universe.policy_version())?;
    universe_row.direct("rule_pack_version", universe.rule_pack_version())?;
    universe_row.hash_shape("body_hash")?;
    charge.set_universe_precomputed(universe_row.finish()?)?;

    for obligation in genesis.obligations() {
        let lifecycle = session.obligation_lifecycle(obligation.id())?;
        let mut row = BorrowedRowCharge::new();
        row.direct("obligation_id", obligation.id())?;
        row.direct("target_kind", obligation.target_kind())?;
        row.canonical_string(
            "target_ids_canonical_json",
            obligation.normalized_target_refs(),
        )?;
        row.direct("property_id", obligation.property_id())?;
        row.direct("lifecycle", &lifecycle)?;
        row.hash_shape("body_hash")?;
        charge.add_precomputed_array(OBLIGATIONS, row.finish()?)?;
    }

    charge.merge_replay_payloads(replay.charge)?;
    let mut assessment_error = None;
    session.for_each_claim_assessment(&mut |assessment| {
        if assessment_error.is_none() {
            match phase0_assessment_row(&assessment, event_count)
                .and_then(|row| charge.add_precomputed_array(CLAIM_ASSESSMENTS, row))
            {
                Ok(()) => {}
                Err(error) => assessment_error = Some(error),
            }
        }
    })?;
    if let Some(error) = assessment_error {
        return Err(error);
    }
    let rows = charge.rows;
    charge.finish_borrowed(&marker, rows, limits)
}

#[cfg(any())]
fn preflight_verified_projection<S: ProjectionSourceV5>(
    session: &S,
    initial: &reviewgraphen_core::ReviewAggregate,
    current: &reviewgraphen_core::ReviewAggregate,
    marker: &IndexMarkerV5,
    expected_rows: u64,
    limits: IndexLimits,
) -> Result<ProjectionPreflightV5, IndexError> {
    let mut charge = ProjectionChargeV5::new(marker, limits)?;
    for artifact in initial.program().artifacts() {
        charge.add_array(
            PROGRAM_OBJECTS,
            &IndexProgramObject {
                object_id: artifact.id.clone(),
                object_kind: artifact.kind.clone(),
                body_hash: super::body_hash(artifact)?,
            },
            None,
        )?;
    }
    for relation in initial.program().relations() {
        charge.add_array(
            PROGRAM_RELATIONS,
            &IndexProgramRelation {
                relation_id: relation.id.clone(),
                relation_kind: relation.kind.clone(),
                source_id: relation.source_id.clone(),
                target_ids_canonical_json: super::canonical_ids(
                    relation.target_ids.iter().cloned(),
                )?,
                body_hash: super::body_hash(relation)?,
            },
            None,
        )?;
    }
    let universe = initial.universe();
    charge.set_universe(&IndexUniverse {
        universe_id: universe.id().clone(),
        snapshot_id: universe.snapshot_id().clone(),
        profile_id: universe.profile_id().to_owned(),
        rule_set_hash: universe.rule_set_hash().clone(),
        extractor_set_hash: universe.extractor_set_hash().clone(),
        policy_version: universe.policy_version().to_owned(),
        rule_pack_version: universe.rule_pack_version().to_owned(),
        body_hash: super::body_hash(universe)?,
    })?;
    for envelope in session.envelopes()? {
        record_projection_decode();
        let line = json_length(envelope)?
            .checked_add(1)
            .ok_or(IndexError::IntegerOutOfRange)?;
        charge.max_event_line_bytes = charge.max_event_line_bytes.max(line);
        let decoded = envelope
            .decode_for_streaming_projection()
            .map_err(|_| IndexError::ProjectionContractViolation)?;
        charge.add_array(
            EVENTS,
            &IndexEvent {
                sequence: envelope.sequence(),
                event_id: envelope.id().clone(),
                schema: EVENT_CONTRACT_VERSION_V4.to_owned(),
                event_hash: envelope.event_hash().clone(),
                payload_hash: envelope.payload_hash().clone(),
                payload_kind: payload_kind_v5(decoded.payload())?.to_owned(),
                actor: envelope.actor().to_owned(),
                logical_time: envelope.logical_time(),
            },
            None,
        )?;
        match decoded.payload() {
            DecodedPayload::ObligationTransition {
                obligation_id,
                next,
            } => {
                let lifecycle = super::serialized_enum(next)?;
                charge.add_array(
                    OBLIGATION_LIFECYCLE,
                    &IndexObligationLifecycle {
                        event_sequence: envelope.sequence(),
                        event_id: envelope.id().clone(),
                        obligation_id: obligation_id.clone(),
                        next_lifecycle: lifecycle,
                    },
                    None,
                )?;
            }
            DecodedPayload::RunGenesisManifestV3(value) => {
                let row = project_registration(envelope, value.genesis_artifact())?;
                charge.max_cas_bytes = charge.max_cas_bytes.max(row.size);
                charge.add_array(REGISTRATIONS, &row, Some("source"))?;
            }
            DecodedPayload::ArtifactRegisteredV3(value) => {
                let row = project_registration(envelope, value)?;
                charge.max_cas_bytes = charge.max_cas_bytes.max(row.size);
                charge.add_array(REGISTRATIONS, &row, Some("source"))?;
            }
            DecodedPayload::SnapshotSourcesRecorded(value) => {
                for entry in value.entries() {
                    charge.add_array(
                        SNAPSHOT_SOURCES,
                        &IndexSnapshotSource {
                            event_sequence: envelope.sequence(),
                            event_id: envelope.id().clone(),
                            snapshot_id: value.snapshot_id().clone(),
                            artifact_id: entry.artifact_id().clone(),
                            registration_id: entry.registration_id().clone(),
                            path: entry.path().to_owned(),
                            content_hash: entry.content_hash().clone(),
                            cas_hash: entry.cas_hash().clone(),
                            line_count: entry.line_count(),
                        },
                        None,
                    )?;
                }
            }
            DecodedPayload::ReviewPlanRecorded(value) => {
                charge.admit_transient_json(json_length(value)?)?;
                charge.add_array(
                    REVIEW_PLANS,
                    &super::projected_review_plan(envelope, value)?,
                    None,
                )?;
            }
            DecodedPayload::ContextEnvelopeProjected(value) => {
                charge.admit_transient_json(json_length(value)?)?;
                charge.add_array(
                    CONTEXT_ENVELOPES,
                    &super::projected_context_envelope(envelope, value)?,
                    None,
                )?;
            }
            DecodedPayload::ReviewExecutionRecorded { execution, claims } => {
                charge.add_array(EXECUTIONS, &project_execution(envelope, execution)?, None)?;
                for claim in claims {
                    charge.add_array(CLAIMS, &project_claim(envelope, claim)?, None)?;
                }
            }
            DecodedPayload::EvidenceRecordedV3(value) => {
                charge.admit_transient_json(json_length(value)?)?;
                charge.add_array(EVIDENCE, &project_evidence(envelope, value)?, None)?
            }
            DecodedPayload::EvidenceBoundV3(value) => {
                charge.admit_transient_json(json_length(value)?)?;
                charge.add_array(EVIDENCE_BINDINGS, &project_binding(envelope, value)?, None)?
            }
            DecodedPayload::VerificationRecordedV3(value) => {
                charge.admit_transient_json(json_length(value)?)?;
                charge.add_array(VERIFICATIONS, &project_verification(envelope, value)?, None)?
            }
            DecodedPayload::DecisionRecordedV3(value) => {
                charge.admit_transient_json(json_length(value)?)?;
                charge.add_array(DECISIONS, &project_decision(envelope, value)?, None)?
            }
            DecodedPayload::FindingRecordedV3(value) => {
                charge.admit_transient_json(json_length(value)?)?;
                charge.add_array(FINDINGS, &project_finding(envelope, value)?, None)?
            }
            _ => return Err(IndexError::ProjectionContractViolation),
        }
    }
    for obligation in current.obligations() {
        record_projection_obligation_probe();
        let lifecycle = super::serialized_enum(&obligation.lifecycle())?;
        charge.add_array(
            OBLIGATIONS,
            &IndexObligation {
                obligation_id: obligation.id().clone(),
                target_kind: obligation.target_kind().to_owned(),
                target_ids_canonical_json: super::canonical_ids(
                    obligation.normalized_target_refs().iter().cloned(),
                )?,
                property_id: obligation.property_id().to_owned(),
                lifecycle,
                body_hash: super::body_hash(obligation)?,
            },
            None,
        )?;
    }
    for assessment in session.claim_assessments()? {
        charge.admit_transient_json(json_length(assessment)?)?;
        let object = canonical_object(assessment)?;
        let claim_id = id(&object, "claim_id")?;
        let confirmed_event_sequence = marker.event_count;
        charge.add_array(
            CLAIM_ASSESSMENTS,
            &IndexClaimAssessmentV3AtV5 {
                confirmed_event_sequence,
                claim_id,
                disposition: string(&object, "disposition")?,
                review_status: string(&object, "review_status")?,
                binding_ids_canonical_json: canonical_component(&object, "binding_ids")?,
                evidence_ids_canonical_json: canonical_component(&object, "evidence_ids")?,
                verification_ids_canonical_json: canonical_component(&object, "verification_ids")?,
                decision_ids_canonical_json: canonical_component(&object, "decision_ids")?,
                finding_ids_canonical_json: canonical_component(&object, "finding_ids")?,
                active_decision_id: optional_id(&object, "active_decision_id")?,
                current_finding_id: optional_id(&object, "current_finding_id")?,
                decision_conflict: boolean(&object, "decision_conflict")?,
            },
            None,
        )?;
    }
    charge.finish(marker, expected_rows, limits)
}

#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
#[cfg(any())]
fn preflight_verified_projection_v5<S: ProjectionSourceV5>(
    session: &S,
    genesis: &RunGenesisSnapshot,
    marker: &IndexMarkerV5,
    inherited_len: usize,
    payment: Option<(&ArtifactRegistrationV4, &GluingInputDescriptorV4)>,
    ui: Option<(&ArtifactRegistrationV4, &GluingInputDescriptorV4)>,
    bundle: Option<&GluingBundleV4>,
    limits: IndexLimits,
) -> Result<ProjectionPreflightV5, IndexError> {
    let envelopes = session.envelopes()?;
    let mut charge = ProjectionChargeV5::new(marker, limits)?;
    for artifact in genesis.program_space().artifacts() {
        charge.add_array(
            PROGRAM_OBJECTS,
            &IndexProgramObject {
                object_id: artifact.id.clone(),
                object_kind: artifact.kind.clone(),
                body_hash: super::body_hash(artifact)?,
            },
            None,
        )?;
    }
    for relation in genesis.program_space().relations() {
        charge.add_array(
            PROGRAM_RELATIONS,
            &IndexProgramRelation {
                relation_id: relation.id.clone(),
                relation_kind: relation.kind.clone(),
                source_id: relation.source_id.clone(),
                target_ids_canonical_json: super::canonical_ids(
                    relation.target_ids.iter().cloned(),
                )?,
                body_hash: super::body_hash(relation)?,
            },
            None,
        )?;
    }
    let universe = genesis.universe();
    charge.set_universe(&IndexUniverse {
        universe_id: universe.id().clone(),
        snapshot_id: universe.snapshot_id().clone(),
        profile_id: universe.profile_id().to_owned(),
        rule_set_hash: universe.rule_set_hash().clone(),
        extractor_set_hash: universe.extractor_set_hash().clone(),
        policy_version: universe.policy_version().to_owned(),
        rule_pack_version: universe.rule_pack_version().to_owned(),
        body_hash: super::body_hash(universe)?,
    })?;
    for obligation in genesis.obligations() {
        let mut lifecycle = obligation.lifecycle();
        for envelope in &envelopes[1..inherited_len] {
            let decoded = envelope
                .decode_for_streaming_projection()
                .map_err(|_| IndexError::ProjectionContractViolation)?;
            if let DecodedPayload::ObligationTransition {
                obligation_id,
                next,
            } = decoded.payload()
                && obligation_id == obligation.id()
            {
                lifecycle = *next;
            }
        }
        charge.add_array(
            OBLIGATIONS,
            &IndexObligation {
                obligation_id: obligation.id().clone(),
                target_kind: obligation.target_kind().to_owned(),
                target_ids_canonical_json: super::canonical_ids(
                    obligation.normalized_target_refs().iter().cloned(),
                )?,
                property_id: obligation.property_id().to_owned(),
                lifecycle: super::serialized_enum(&lifecycle)?,
                body_hash: obligation_body_hash_at_lifecycle(obligation, &lifecycle)?,
            },
            None,
        )?;
    }
    let genesis_envelope = &envelopes[0];
    let line = json_length(genesis_envelope)?
        .checked_add(1)
        .ok_or(IndexError::IntegerOutOfRange)?;
    charge.max_event_line_bytes = charge.max_event_line_bytes.max(line);
    charge.add_array(
        EVENTS,
        &IndexEvent {
            sequence: genesis_envelope.sequence(),
            event_id: genesis_envelope.id().clone(),
            schema: EVENT_CONTRACT_VERSION_V4.to_owned(),
            event_hash: genesis_envelope.event_hash().clone(),
            payload_hash: genesis_envelope.payload_hash().clone(),
            payload_kind: "run_genesis_manifest".to_owned(),
            actor: genesis_envelope.actor().to_owned(),
            logical_time: genesis_envelope.logical_time(),
        },
        None,
    )?;
    let genesis_registration =
        project_registration_opaque_v3(genesis_envelope, &session.genesis_artifact()?)?;
    charge.max_cas_bytes = charge.max_cas_bytes.max(genesis_registration.size);
    charge.add_array(REGISTRATIONS, &genesis_registration, Some("source"))?;
    for envelope in &envelopes[1..inherited_len] {
        let line = json_length(envelope)?
            .checked_add(1)
            .ok_or(IndexError::IntegerOutOfRange)?;
        charge.max_event_line_bytes = charge.max_event_line_bytes.max(line);
        let decoded = envelope
            .decode_for_streaming_projection()
            .map_err(|_| IndexError::ProjectionContractViolation)?;
        charge.add_array(
            EVENTS,
            &IndexEvent {
                sequence: envelope.sequence(),
                event_id: envelope.id().clone(),
                schema: EVENT_CONTRACT_VERSION_V4.to_owned(),
                event_hash: envelope.event_hash().clone(),
                payload_hash: envelope.payload_hash().clone(),
                payload_kind: payload_kind_v5(decoded.payload())?.to_owned(),
                actor: envelope.actor().to_owned(),
                logical_time: envelope.logical_time(),
            },
            None,
        )?;
        match decoded.payload() {
            DecodedPayload::ObligationTransition {
                obligation_id,
                next,
            } => charge.add_array(
                OBLIGATION_LIFECYCLE,
                &IndexObligationLifecycle {
                    event_sequence: envelope.sequence(),
                    event_id: envelope.id().clone(),
                    obligation_id: obligation_id.clone(),
                    next_lifecycle: super::serialized_enum(next)?,
                },
                None,
            )?,
            DecodedPayload::ArtifactRegisteredV3(value) => {
                let row = project_registration(envelope, value)?;
                charge.max_cas_bytes = charge.max_cas_bytes.max(row.size);
                charge.add_array(REGISTRATIONS, &row, Some("source"))?;
            }
            DecodedPayload::SnapshotSourcesRecorded(value) => {
                for entry in value.entries() {
                    charge.add_array(
                        SNAPSHOT_SOURCES,
                        &IndexSnapshotSource {
                            event_sequence: envelope.sequence(),
                            event_id: envelope.id().clone(),
                            snapshot_id: value.snapshot_id().clone(),
                            artifact_id: entry.artifact_id().clone(),
                            registration_id: entry.registration_id().clone(),
                            path: entry.path().to_owned(),
                            content_hash: entry.content_hash().clone(),
                            cas_hash: entry.cas_hash().clone(),
                            line_count: entry.line_count(),
                        },
                        None,
                    )?;
                }
            }
            DecodedPayload::ReviewPlanRecorded(value) => charge.add_array(
                REVIEW_PLANS,
                &super::projected_review_plan(envelope, value)?,
                None,
            )?,
            DecodedPayload::ContextEnvelopeProjected(value) => charge.add_array(
                CONTEXT_ENVELOPES,
                &super::projected_context_envelope(envelope, value)?,
                None,
            )?,
            DecodedPayload::ReviewExecutionRecorded { execution, claims } => {
                charge.add_array(EXECUTIONS, &project_execution(envelope, execution)?, None)?;
                for claim in claims {
                    charge.add_array(CLAIMS, &project_claim(envelope, claim)?, None)?;
                    if let Some(assessment) = session.claim_assessment(claim.id())? {
                        charge.add_array(
                            CLAIM_ASSESSMENTS,
                            &project_claim_assessment_v4(assessment, marker.event_count)?,
                            None,
                        )?;
                    }
                }
            }
            DecodedPayload::EvidenceRecordedV3(v) => {
                charge.add_array(EVIDENCE, &project_evidence(envelope, v)?, None)?
            }
            DecodedPayload::EvidenceBoundV3(v) => {
                charge.add_array(EVIDENCE_BINDINGS, &project_binding(envelope, v)?, None)?
            }
            DecodedPayload::VerificationRecordedV3(v) => {
                charge.add_array(VERIFICATIONS, &project_verification(envelope, v)?, None)?
            }
            DecodedPayload::DecisionRecordedV3(v) => {
                charge.add_array(DECISIONS, &project_decision(envelope, v)?, None)?
            }
            DecodedPayload::FindingRecordedV3(v) => {
                charge.add_array(FINDINGS, &project_finding(envelope, v)?, None)?
            }
            _ => return Err(IndexError::ProjectionContractViolation),
        }
    }
    for (offset, (registration, descriptor)) in [payment, ui].into_iter().flatten().enumerate() {
        let envelope = &envelopes[inherited_len + offset];
        let line = json_length(envelope)?
            .checked_add(1)
            .ok_or(IndexError::IntegerOutOfRange)?;
        charge.max_event_line_bytes = charge.max_event_line_bytes.max(line);
        charge.add_array(
            EVENTS,
            &IndexEvent {
                sequence: envelope.sequence(),
                event_id: envelope.id().clone(),
                schema: EVENT_CONTRACT_VERSION_V4.to_owned(),
                event_hash: envelope.event_hash().clone(),
                payload_hash: envelope.payload_hash().clone(),
                payload_kind: "artifact_registered_v4".to_owned(),
                actor: envelope.actor().to_owned(),
                logical_time: envelope.logical_time(),
            },
            None,
        )?;
        let reg = ArtifactRegistrationV4IndexItem {
            event_sequence: envelope.sequence(),
            event_id: envelope.id().clone(),
            event_actor: envelope.actor().to_owned(),
            registration_id: registration.id().clone(),
            schema: "reviewgraphen.artifact_registration.v4".to_owned(),
            run_id: registration.run_id().clone(),
            cas_hash: registration.cas_hash().clone(),
            media_type: registration.media_type().to_owned(),
            size: registration.size(),
            sensitivity: super::serialized_enum(&registration.sensitivity())?,
            source_kind: artifact_source_v4_kind(registration.source()).to_owned(),
            source: serde_json::to_value(registration.source())
                .map_err(|_| IndexError::ProjectionContractViolation)?,
            descriptor_id: descriptor.id().clone(),
            body_hash: super::body_hash(registration)?,
        };
        charge.max_cas_bytes = charge.max_cas_bytes.max(reg.size);
        charge.add_m5_array(REGISTRATIONS_V4, &reg, |ints, text| {
            charge_registration_v4_sql(&reg, ints, text)
        })?;
        let desc = GluingInputDescriptorV4IndexItem {
            event_sequence: envelope.sequence(),
            event_id: envelope.id().clone(),
            descriptor: serde_json::to_value(descriptor)
                .map_err(|_| IndexError::ProjectionContractViolation)?,
            registration_id: registration.id().clone(),
            descriptor_hash: registration.cas_hash().clone(),
            descriptor_size: registration.size(),
            body_hash: super::body_hash(descriptor)?,
        };
        charge.add_m5_array(GLUING_DESCRIPTORS, &desc, |ints, text| {
            charge_descriptor_v4_sql(&desc, ints, text)
        })?;
    }
    if let Some(bundle) = bundle {
        let envelope =
            &envelopes[inherited_len + usize::from(payment.is_some()) + usize::from(ui.is_some())];
        let line = json_length(envelope)?
            .checked_add(1)
            .ok_or(IndexError::IntegerOutOfRange)?;
        charge.max_event_line_bytes = charge.max_event_line_bytes.max(line);
        charge.add_array(
            EVENTS,
            &IndexEvent {
                sequence: envelope.sequence(),
                event_id: envelope.id().clone(),
                schema: EVENT_CONTRACT_VERSION_V4.to_owned(),
                event_hash: envelope.event_hash().clone(),
                payload_hash: envelope.payload_hash().clone(),
                payload_kind: "gluing_bundle_recorded_v4".to_owned(),
                actor: envelope.actor().to_owned(),
                logical_time: envelope.logical_time(),
            },
            None,
        )?;
        preflight_bundle_rows(&mut charge, envelope, bundle)?;
    }
    let rows = charge.rows;
    charge.finish(marker, rows, limits)
}

#[allow(dead_code)]
fn payload_kind_v5(payload: &DecodedPayload) -> Result<&'static str, IndexError> {
    Ok(match payload {
        DecodedPayload::ObligationTransition { .. } => "obligation_transition",
        DecodedPayload::RunGenesisManifestV3(_) => "run_genesis_manifest",
        DecodedPayload::ArtifactRegisteredV3(_) => "artifact_registered",
        DecodedPayload::SnapshotSourcesRecorded(_) => "snapshot_sources_recorded",
        DecodedPayload::ReviewPlanRecorded(_) => "review_plan_recorded",
        DecodedPayload::ContextEnvelopeProjected(_) => "context_envelope_projected",
        DecodedPayload::ReviewExecutionRecorded { .. } => "review_execution_recorded",
        DecodedPayload::EvidenceRecordedV3(_) => "evidence_recorded_v3",
        DecodedPayload::EvidenceBoundV3(_) => "evidence_bound_v3",
        DecodedPayload::VerificationRecordedV3(_) => "verification_recorded_v3",
        DecodedPayload::DecisionRecordedV3(_) => "decision_recorded_v3",
        DecodedPayload::FindingRecordedV3(_) => "finding_recorded_v3",
        _ => return Err(IndexError::ProjectionContractViolation),
    })
}

fn canonical_object<T: Serialize>(value: &T) -> Result<Map<String, Value>, IndexError> {
    serde_json::to_value(value)
        .map_err(|_| IndexError::ProjectionContractViolation)?
        .as_object()
        .cloned()
        .ok_or(IndexError::ProjectionContractViolation)
}
fn string(object: &Map<String, Value>, key: &str) -> Result<String, IndexError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(IndexError::ProjectionContractViolation)
}
fn id(object: &Map<String, Value>, key: &str) -> Result<StableId, IndexError> {
    StableId::parse(string(object, key)?).map_err(|_| IndexError::ProjectionContractViolation)
}
fn hash(object: &Map<String, Value>, key: &str) -> Result<ContentHash, IndexError> {
    ContentHash::parse(string(object, key)?).map_err(|_| IndexError::ProjectionContractViolation)
}
fn optional_id(object: &Map<String, Value>, key: &str) -> Result<Option<StableId>, IndexError> {
    match object
        .get(key)
        .ok_or(IndexError::ProjectionContractViolation)?
    {
        Value::Null => Ok(None),
        Value::String(value) => StableId::parse(value.clone())
            .map(Some)
            .map_err(|_| IndexError::ProjectionContractViolation),
        _ => Err(IndexError::ProjectionContractViolation),
    }
}
fn boolean(object: &Map<String, Value>, key: &str) -> Result<bool, IndexError> {
    object
        .get(key)
        .and_then(Value::as_bool)
        .ok_or(IndexError::ProjectionContractViolation)
}
fn canonical_component(object: &Map<String, Value>, key: &str) -> Result<String, IndexError> {
    let value = object
        .get(key)
        .ok_or(IndexError::ProjectionContractViolation)?;
    String::from_utf8(canonical_json(value).map_err(|_| IndexError::ProjectionContractViolation)?)
        .map_err(|_| IndexError::ProjectionContractViolation)
}
fn optional_string(object: &Map<String, Value>, key: &str) -> Result<Option<String>, IndexError> {
    match object
        .get(key)
        .ok_or(IndexError::ProjectionContractViolation)?
    {
        Value::Null => Ok(None),
        Value::String(value) => Ok(Some(value.clone())),
        _ => Err(IndexError::ProjectionContractViolation),
    }
}
#[allow(dead_code)]
fn body_hash<T: Serialize>(value: &T) -> Result<ContentHash, IndexError> {
    super::body_hash(value)
}

fn projection_value_and_hash<T: Serialize>(value: &T) -> Result<(Value, ContentHash), IndexError> {
    let canonical = canonical_json(value).map_err(|_| IndexError::ProjectionContractViolation)?;
    let projected =
        serde_json::to_value(value).map_err(|_| IndexError::ProjectionContractViolation)?;
    Ok((projected, ContentHash::sha256(&canonical)))
}

fn projection_hash<T: Serialize>(value: &T) -> Result<ContentHash, IndexError> {
    Ok(ContentHash::sha256(
        &canonical_json(value).map_err(|_| IndexError::ProjectionContractViolation)?,
    ))
}

#[allow(dead_code)]
fn project_registration(
    envelope: &reviewgraphen_core::EventEnvelope,
    value: &reviewgraphen_core::ArtifactRegisteredV3,
) -> Result<IndexArtifactRegistrationV5, IndexError> {
    let source_kind = source_kind_and_run_id(value.source()).0.to_owned();
    let source_canonical_json = String::from_utf8(
        canonical_json(value.source()).map_err(|_| IndexError::ProjectionContractViolation)?,
    )
    .map_err(|_| IndexError::ProjectionContractViolation)?;
    Ok(IndexArtifactRegistrationV5 {
        event_sequence: envelope.sequence(),
        event_id: envelope.id().clone(),
        registration_id: value.registration_id().clone(),
        run_id: value.run_id().clone(),
        cas_hash: value.cas_hash().clone(),
        media_type: value.media_type().to_owned(),
        size: value.size(),
        sensitivity: super::serialized_enum(&value.sensitivity())?,
        source_kind,
        source_canonical_json,
        source: serde_json::to_value(value.source())
            .map_err(|_| IndexError::ProjectionContractViolation)?,
        body_hash: body_hash(value)?,
    })
}

fn project_registration_opaque_v3(
    envelope: &impl ProjectionEventMetadataV5,
    value: &BorrowedArtifactRegistrationProjectionV3<'_>,
) -> Result<IndexArtifactRegistrationV5, IndexError> {
    let source = BorrowedCanonicalArtifactSourceV3(value.source());
    let source_value =
        serde_json::to_value(&source).map_err(|_| IndexError::ProjectionContractViolation)?;
    let source_canonical_json = String::from_utf8(
        canonical_json(&source).map_err(|_| IndexError::ProjectionContractViolation)?,
    )
    .map_err(|_| IndexError::ProjectionContractViolation)?;
    Ok(IndexArtifactRegistrationV5 {
        event_sequence: envelope.sequence(),
        event_id: envelope.id().clone(),
        registration_id: value.registration_id().clone(),
        run_id: value.run_id().clone(),
        cas_hash: value.cas_hash().clone(),
        media_type: value.media_type().to_owned(),
        size: value.size(),
        sensitivity: super::serialized_enum(&value.sensitivity())?,
        source_kind: value.source().kind().to_owned(),
        source_canonical_json,
        source: source_value,
        body_hash: ContentHash::sha256(
            &canonical_json(&BorrowedRegistrationProjectionV3(value))
                .map_err(|_| IndexError::ProjectionContractViolation)?,
        ),
    })
}

#[allow(dead_code)]
fn project_evidence(
    envelope: &reviewgraphen_core::EventEnvelope,
    value: &reviewgraphen_core::EvidenceV3,
) -> Result<IndexEvidenceV3AtV5, IndexError> {
    let o = canonical_object(value)?;
    Ok(IndexEvidenceV3AtV5 {
        event_sequence: envelope.sequence(),
        event_id: envelope.id().clone(),
        evidence_id: id(&o, "id")?,
        schema: string(&o, "schema")?,
        kind: string(&o, "kind")?,
        snapshot_id: id(&o, "snapshot_id")?,
        subject_ids_canonical_json: canonical_component(&o, "subject_ids")?,
        descriptor_id: string(&o, "descriptor_id")?,
        procedure_version: string(&o, "procedure_version")?,
        input_registration_id: id(&o, "input_registration_id")?,
        output_registration_id: id(&o, "output_registration_id")?,
        observation: string(&o, "observation")?,
        body_hash: value
            .body_hash()
            .map_err(|_| IndexError::ProjectionContractViolation)?,
    })
}
#[allow(dead_code)]
fn project_binding(
    envelope: &reviewgraphen_core::EventEnvelope,
    value: &reviewgraphen_core::EvidenceBindingV3,
) -> Result<IndexEvidenceBindingV3AtV5, IndexError> {
    let o = canonical_object(value)?;
    Ok(IndexEvidenceBindingV3AtV5 {
        event_sequence: envelope.sequence(),
        event_id: envelope.id().clone(),
        binding_id: id(&o, "id")?,
        schema: string(&o, "schema")?,
        claim_id: id(&o, "claim_id")?,
        evidence_id: id(&o, "evidence_id")?,
        relation: string(&o, "relation")?,
        property_id: string(&o, "property_id")?,
        body_hash: value
            .body_hash()
            .map_err(|_| IndexError::ProjectionContractViolation)?,
    })
}
#[allow(dead_code)]
fn project_verification(
    envelope: &reviewgraphen_core::EventEnvelope,
    value: &reviewgraphen_core::VerificationV3,
) -> Result<IndexVerificationV3AtV5, IndexError> {
    let o = canonical_object(value)?;
    Ok(IndexVerificationV3AtV5 {
        event_sequence: envelope.sequence(),
        event_id: envelope.id().clone(),
        verification_id: id(&o, "id")?,
        schema: string(&o, "schema")?,
        claim_id: id(&o, "claim_id")?,
        descriptor_id: string(&o, "descriptor_id")?,
        procedure_version: string(&o, "procedure_version")?,
        input_registration_id: id(&o, "input_registration_id")?,
        output_registration_id: id(&o, "output_registration_id")?,
        evidence_ids_canonical_json: canonical_component(&o, "evidence_ids")?,
        outcome: string(&o, "outcome")?,
        limitations_canonical_json: canonical_component(&o, "limitations")?,
        body_hash: value
            .body_hash()
            .map_err(|_| IndexError::ProjectionContractViolation)?,
    })
}
#[allow(dead_code)]
fn project_decision(
    envelope: &reviewgraphen_core::EventEnvelope,
    value: &reviewgraphen_core::DecisionV3,
) -> Result<IndexDecisionV3AtV5, IndexError> {
    let o = canonical_object(value)?;
    Ok(IndexDecisionV3AtV5 {
        event_sequence: envelope.sequence(),
        event_id: envelope.id().clone(),
        decision_id: id(&o, "id")?,
        schema: string(&o, "schema")?,
        policy_revision_hash: hash(&o, "policy_revision_hash")?,
        run_id: id(&o, "run_id")?,
        universe_id: id(&o, "universe_id")?,
        claim_id: id(&o, "claim_id")?,
        property_id: string(&o, "property_id")?,
        outcome: string(&o, "outcome")?,
        actor: string(&o, "actor")?,
        authority_id: string(&o, "authority_id")?,
        snapshot_id: id(&o, "snapshot_id")?,
        source_ids_canonical_json: canonical_component(&o, "source_ids")?,
        rationale: string(&o, "rationale")?,
        issued_at: string(&o, "issued_at")?,
        expires_at: optional_string(&o, "expires_at")?,
        body_hash: value
            .body_hash()
            .map_err(|_| IndexError::ProjectionContractViolation)?,
    })
}
#[allow(dead_code)]
fn project_finding(
    envelope: &reviewgraphen_core::EventEnvelope,
    value: &reviewgraphen_core::FindingV3,
) -> Result<IndexFindingV3AtV5, IndexError> {
    let o = canonical_object(value)?;
    Ok(IndexFindingV3AtV5 {
        event_sequence: envelope.sequence(),
        event_id: envelope.id().clone(),
        finding_id: id(&o, "id")?,
        schema: string(&o, "schema")?,
        projection_descriptor_id: string(&o, "projection_descriptor_id")?,
        claim_id: id(&o, "claim_id")?,
        status: string(&o, "status")?,
        evidence_ids_canonical_json: canonical_component(&o, "evidence_ids")?,
        verification_ids_canonical_json: canonical_component(&o, "verification_ids")?,
        decision_id: optional_id(&o, "decision_id")?,
        supersedes_finding_id: optional_id(&o, "supersedes_finding_id")?,
        body_hash: value
            .body_hash()
            .map_err(|_| IndexError::ProjectionContractViolation)?,
    })
}

#[allow(dead_code)]
fn project_execution(
    envelope: &reviewgraphen_core::EventEnvelope,
    value: &reviewgraphen_core::ExecutionRecord,
) -> Result<IndexExecution, IndexError> {
    let outcome_kind = match value.outcome() {
        reviewgraphen_core::ExecutionOutcome::Structured => "structured",
        reviewgraphen_core::ExecutionOutcome::Abstained { .. } => "abstained",
        reviewgraphen_core::ExecutionOutcome::Malformed { .. } => "malformed",
        reviewgraphen_core::ExecutionOutcome::ProviderFailure { .. } => "provider_failure",
    };
    Ok(IndexExecution {
        event_sequence: envelope.sequence(),
        event_id: envelope.id().clone(),
        execution_id: value.id().clone(),
        plan_id: value.plan_id().clone(),
        wave_id: value.wave_id().clone(),
        snapshot_id: value.snapshot_id().clone(),
        envelope_id: value.envelope_id().clone(),
        obligation_ids_canonical_json: super::canonical_ids(
            value.obligation_ids().iter().cloned(),
        )?,
        reviewer_kind: value.reviewer_kind().to_owned(),
        reviewer_id: value.reviewer_id().to_owned(),
        provider: value.provider().map(str::to_owned),
        model: value.model().map(str::to_owned),
        model_revision: value.model_revision().map(str::to_owned),
        system_prompt_version: value.system_prompt_version().to_owned(),
        prompt_template_version: value.prompt_template_version().to_owned(),
        inference_settings_canonical_json: String::from_utf8(
            canonical_json(value.inference_settings())
                .map_err(|_| IndexError::ProjectionContractViolation)?,
        )
        .map_err(|_| IndexError::ProjectionContractViolation)?,
        tool_policy_version: value.tool_policy_version().to_owned(),
        tool_calls_canonical_json: "[]".to_owned(),
        attempt: value.attempt(),
        raw_registration_id: value.raw_artifact_registration_id().clone(),
        raw_hash: value.raw_artifact_hash().clone(),
        parsed_claim_ids_canonical_json: super::canonical_ids(
            value.parsed_claim_ids().iter().cloned(),
        )?,
        outcome_kind: outcome_kind.to_owned(),
        outcome_canonical_json: String::from_utf8(
            canonical_json(value.outcome()).map_err(|_| IndexError::ProjectionContractViolation)?,
        )
        .map_err(|_| IndexError::ProjectionContractViolation)?,
        identity_body_hash: value
            .identity_body_hash()
            .map_err(|_| IndexError::ProjectionContractViolation)?,
        body_hash: value
            .body_hash()
            .map_err(|_| IndexError::ProjectionContractViolation)?,
    })
}

#[allow(dead_code)]
fn project_claim(
    envelope: &reviewgraphen_core::EventEnvelope,
    value: &reviewgraphen_core::ExecutionClaimV2,
) -> Result<IndexClaim, IndexError> {
    Ok(IndexClaim {
        event_sequence: envelope.sequence(),
        event_id: envelope.id().clone(),
        claim_id: value.id().clone(),
        execution_id: value.execution_id().clone(),
        obligation_ids_canonical_json: super::canonical_ids(
            value.obligation_ids().iter().cloned(),
        )?,
        property_id: value.property_id().to_owned(),
        target_refs_canonical_json: super::canonical_ids(value.target_refs().iter().cloned())?,
        polarity: super::serialized_enum(&value.polarity())?,
        disposition: super::serialized_enum(&value.disposition())?,
        summary: value.summary().to_owned(),
        source_ids_canonical_json: super::canonical_ids(value.source_ids().iter().cloned())?,
        assumptions_canonical_json: super::canonical_string_set(value.assumptions())?,
        requested_evidence_canonical_json: super::canonical_string_set(value.requested_evidence())?,
        candidate_confidence_canonical_json: String::from_utf8(
            canonical_json(&value.candidate_confidence())
                .map_err(|_| IndexError::ProjectionContractViolation)?,
        )
        .map_err(|_| IndexError::ProjectionContractViolation)?,
        author_kind: super::serialized_enum(&value.author_kind())?,
        review_status: super::serialized_enum(&value.review_status())?,
        identity_body_hash: value
            .identity_body_hash()
            .map_err(|_| IndexError::ProjectionContractViolation)?,
        body_hash: value
            .body_hash()
            .map_err(|_| IndexError::ProjectionContractViolation)?,
    })
}

fn add_text_charge(total: &mut u64, text: &str) -> Result<(), IndexError> {
    *total = total
        .checked_add(u64::try_from(text.len()).map_err(|_| IndexError::IntegerOutOfRange)?)
        .ok_or(IndexError::IntegerOutOfRange)?;
    Ok(())
}

fn charge_m5_dto<T: Serialize>(
    value: &T,
    integer_cells: &mut u64,
    text_bytes: &mut u64,
) -> Result<(), IndexError> {
    let object = canonical_object(value)?;
    for value in object.values() {
        match value {
            Value::Null => {}
            Value::Bool(_) | Value::Number(_) => {
                *integer_cells = integer_cells
                    .checked_add(1)
                    .ok_or(IndexError::IntegerOutOfRange)?;
            }
            Value::String(text) => add_text_charge(text_bytes, text)?,
            Value::Array(_) | Value::Object(_) => {
                let bytes =
                    canonical_json(value).map_err(|_| IndexError::ProjectionContractViolation)?;
                *text_bytes = text_bytes
                    .checked_add(
                        u64::try_from(bytes.len()).map_err(|_| IndexError::IntegerOutOfRange)?,
                    )
                    .ok_or(IndexError::IntegerOutOfRange)?;
            }
        }
    }
    Ok(())
}

fn charge_m5_event_tuple(
    event_id: &StableId,
    integer_cells: &mut u64,
    text_bytes: &mut u64,
) -> Result<(), IndexError> {
    *integer_cells = integer_cells
        .checked_add(1)
        .ok_or(IndexError::IntegerOutOfRange)?;
    add_text_charge(text_bytes, event_id.as_str())
}

#[allow(dead_code)]
fn charge_registration_v4_sql(
    row: &ArtifactRegistrationV4IndexItem,
    integer_cells: &mut u64,
    text_bytes: &mut u64,
) -> Result<(), IndexError> {
    charge_m5_event_tuple(&row.event_id, integer_cells, text_bytes)?;
    for text in [
        row.registration_id.as_str(),
        row.schema.as_str(),
        row.run_id.as_str(),
        row.cas_hash.as_str(),
        row.media_type.as_str(),
        row.sensitivity.as_str(),
        row.source_kind.as_str(),
        row.descriptor_id.as_str(),
        row.body_hash.as_str(),
    ] {
        add_text_charge(text_bytes, text)?;
    }
    *integer_cells = integer_cells
        .checked_add(1)
        .ok_or(IndexError::IntegerOutOfRange)?;
    let source =
        canonical_json(&row.source).map_err(|_| IndexError::ProjectionContractViolation)?;
    *text_bytes = text_bytes
        .checked_add(u64::try_from(source.len()).map_err(|_| IndexError::IntegerOutOfRange)?)
        .ok_or(IndexError::IntegerOutOfRange)?;
    Ok(())
}

#[allow(dead_code)]
fn charge_descriptor_v4_sql(
    row: &GluingInputDescriptorV4IndexItem,
    integer_cells: &mut u64,
    text_bytes: &mut u64,
) -> Result<(), IndexError> {
    charge_m5_event_tuple(&row.event_id, integer_cells, text_bytes)?;
    charge_m5_dto(&row.descriptor, integer_cells, text_bytes)?;
    for text in [
        row.registration_id.as_str(),
        row.descriptor_hash.as_str(),
        row.body_hash.as_str(),
    ] {
        add_text_charge(text_bytes, text)?;
    }
    *integer_cells = integer_cells
        .checked_add(1)
        .ok_or(IndexError::IntegerOutOfRange)?;
    Ok(())
}

#[allow(dead_code)]
fn charge_cover_v4_sql(
    row: &ContextCoverV4IndexItem,
    integer_cells: &mut u64,
    text_bytes: &mut u64,
) -> Result<(), IndexError> {
    charge_m5_event_tuple(&row.event_id, integer_cells, text_bytes)?;
    charge_m5_dto(&row.cover, integer_cells, text_bytes)?;
    add_text_charge(text_bytes, row.body_hash.as_str())
}

#[allow(dead_code)]
fn charge_section_v4_sql(
    row: &SectionV4IndexItem,
    integer_cells: &mut u64,
    text_bytes: &mut u64,
) -> Result<(), IndexError> {
    charge_m5_event_tuple(&row.event_id, integer_cells, text_bytes)?;
    charge_m5_dto(&row.section, integer_cells, text_bytes)?;
    add_text_charge(text_bytes, row.body_hash.as_str())
}

#[allow(dead_code)]
fn charge_restriction_v4_sql(
    row: &RestrictionV4IndexItem,
    integer_cells: &mut u64,
    text_bytes: &mut u64,
) -> Result<(), IndexError> {
    charge_m5_event_tuple(&row.event_id, integer_cells, text_bytes)?;
    add_text_charge(text_bytes, row.attempt_id.as_str())?;
    charge_m5_dto(&row.restriction, integer_cells, text_bytes)?;
    add_text_charge(text_bytes, row.body_hash.as_str())
}

#[allow(dead_code)]
fn charge_attempt_v4_sql(
    row: &GluingAttemptV4IndexItem,
    integer_cells: &mut u64,
    text_bytes: &mut u64,
) -> Result<(), IndexError> {
    charge_m5_event_tuple(&row.event_id, integer_cells, text_bytes)?;
    charge_m5_dto(&row.attempt, integer_cells, text_bytes)?;
    add_text_charge(text_bytes, row.body_hash.as_str())
}

#[allow(dead_code)]
fn charge_candidate_v4_sql(
    row: &GlobalCandidateV4IndexItem,
    integer_cells: &mut u64,
    text_bytes: &mut u64,
) -> Result<(), IndexError> {
    charge_m5_event_tuple(&row.event_id, integer_cells, text_bytes)?;
    add_text_charge(text_bytes, row.attempt_id.as_str())?;
    charge_m5_dto(&row.candidate, integer_cells, text_bytes)?;
    add_text_charge(text_bytes, row.body_hash.as_str())
}

#[allow(dead_code)]
fn charge_obstruction_v4_sql(
    row: &GluingObstructionV4IndexItem,
    integer_cells: &mut u64,
    text_bytes: &mut u64,
) -> Result<(), IndexError> {
    charge_m5_event_tuple(&row.event_id, integer_cells, text_bytes)?;
    charge_m5_dto(&row.obstruction, integer_cells, text_bytes)?;
    add_text_charge(text_bytes, row.body_hash.as_str())
}

#[allow(dead_code)]
fn preflight_bundle_rows(
    charge: &mut ProjectionChargeV5,
    envelope: &reviewgraphen_core::EventEnvelope,
    bundle: &GluingBundleV4,
) -> Result<(), IndexError> {
    let event_sequence = envelope.sequence();
    let event_id = envelope.id().clone();
    let cover = ContextCoverV4IndexItem {
        event_sequence,
        event_id: event_id.clone(),
        cover: serde_json::to_value(bundle.cover())
            .map_err(|_| IndexError::ProjectionContractViolation)?,
        body_hash: super::body_hash(bundle.cover())?,
    };
    charge.add_m5_array(CONTEXT_COVERS, &cover, |integers, text| {
        charge_cover_v4_sql(&cover, integers, text)
    })?;
    for value in bundle.sections() {
        let row = SectionV4IndexItem {
            event_sequence,
            event_id: event_id.clone(),
            section: serde_json::to_value(value)
                .map_err(|_| IndexError::ProjectionContractViolation)?,
            body_hash: super::body_hash(value)?,
        };
        charge.add_m5_array(SECTIONS, &row, |integers, text| {
            charge_section_v4_sql(&row, integers, text)
        })?;
    }
    let attempt_id = bundle.attempt().id().clone();
    for value in bundle.restrictions() {
        let row = RestrictionV4IndexItem {
            event_sequence,
            event_id: event_id.clone(),
            attempt_id: attempt_id.clone(),
            restriction: serde_json::to_value(value)
                .map_err(|_| IndexError::ProjectionContractViolation)?,
            body_hash: super::body_hash(value)?,
        };
        charge.add_m5_array(RESTRICTIONS, &row, |integers, text| {
            charge_restriction_v4_sql(&row, integers, text)
        })?;
    }
    let attempt = GluingAttemptV4IndexItem {
        event_sequence,
        event_id: event_id.clone(),
        attempt: serde_json::to_value(bundle.attempt())
            .map_err(|_| IndexError::ProjectionContractViolation)?,
        body_hash: super::body_hash(bundle.attempt())?,
    };
    charge.add_m5_array(GLUING_ATTEMPTS, &attempt, |integers, text| {
        charge_attempt_v4_sql(&attempt, integers, text)
    })?;
    if let Some(value) = bundle.global_candidate() {
        let row = GlobalCandidateV4IndexItem {
            event_sequence,
            event_id: event_id.clone(),
            attempt_id: attempt_id.clone(),
            candidate: serde_json::to_value(value)
                .map_err(|_| IndexError::ProjectionContractViolation)?,
            body_hash: super::body_hash(value)?,
        };
        charge.add_m5_array(GLOBAL_CANDIDATES, &row, |integers, text| {
            charge_candidate_v4_sql(&row, integers, text)
        })?;
    }
    if let Some(value) = bundle.obstruction() {
        let row = GluingObstructionV4IndexItem {
            event_sequence,
            event_id,
            obstruction: serde_json::to_value(value)
                .map_err(|_| IndexError::ProjectionContractViolation)?,
            body_hash: super::body_hash(value)?,
        };
        charge.add_m5_array(GLUING_OBSTRUCTIONS, &row, |integers, text| {
            charge_obstruction_v4_sql(&row, integers, text)
        })?;
    }
    Ok(())
}

fn account_snapshot(
    snapshot: &IndexSnapshotV5,
    max_event_line_bytes: u64,
    limits: IndexLimits,
) -> Result<IndexAccountingV5, IndexError> {
    let rows = 1_u64
        .checked_add(
            u64::try_from(snapshot.events.len()).map_err(|_| IndexError::IntegerOutOfRange)?,
        )
        .and_then(|v| v.checked_add(snapshot.program_objects.len() as u64))
        .and_then(|v| v.checked_add(snapshot.program_relations.len() as u64))
        .and_then(|v| v.checked_add(u64::from(snapshot.universe.is_some())))
        .and_then(|v| v.checked_add(snapshot.obligations.len() as u64))
        .and_then(|v| v.checked_add(snapshot.obligation_lifecycle.len() as u64))
        .and_then(|v| v.checked_add(snapshot.executions.len() as u64))
        .and_then(|v| v.checked_add(snapshot.claims.len() as u64))
        .and_then(|v| v.checked_add(snapshot.artifact_registrations.len() as u64))
        .and_then(|v| v.checked_add(snapshot.snapshot_sources.len() as u64))
        .and_then(|v| v.checked_add(snapshot.context_envelopes.len() as u64))
        .and_then(|v| v.checked_add(snapshot.review_plans.len() as u64))
        .and_then(|v| v.checked_add(snapshot.shadows.len() as u64))
        .and_then(|v| v.checked_add(snapshot.projected_findings.len() as u64))
        .and_then(|v| v.checked_add(snapshot.evidence.len() as u64))
        .and_then(|v| v.checked_add(snapshot.evidence_bindings.len() as u64))
        .and_then(|v| v.checked_add(snapshot.verifications.len() as u64))
        .and_then(|v| v.checked_add(snapshot.decisions.len() as u64))
        .and_then(|v| v.checked_add(snapshot.findings.len() as u64))
        .and_then(|v| v.checked_add(snapshot.claim_assessments.len() as u64))
        .and_then(|v| v.checked_add(snapshot.artifact_registrations_v4.len() as u64))
        .and_then(|v| v.checked_add(snapshot.gluing_input_descriptors.len() as u64))
        .and_then(|v| v.checked_add(snapshot.context_covers.len() as u64))
        .and_then(|v| v.checked_add(snapshot.sections.len() as u64))
        .and_then(|v| v.checked_add(snapshot.restrictions.len() as u64))
        .and_then(|v| v.checked_add(snapshot.gluing_attempts.len() as u64))
        .and_then(|v| v.checked_add(snapshot.global_candidates.len() as u64))
        .and_then(|v| v.checked_add(snapshot.gluing_obstructions.len() as u64))
        .ok_or(IndexError::IntegerOutOfRange)?;
    if rows > limits.max_rows {
        return Err(IndexError::Incomplete {
            limit: limits.max_rows,
            observed: rows,
        });
    }
    let mut integer_cells = 0_u64;
    let mut text_bytes = 0_u64;
    charge_sql_row(&snapshot.marker, &mut integer_cells, &mut text_bytes, None)?;
    for row in &snapshot.events {
        charge_sql_row(row, &mut integer_cells, &mut text_bytes, None)?;
    }
    for row in &snapshot.program_objects {
        charge_sql_row(row, &mut integer_cells, &mut text_bytes, None)?;
    }
    for row in &snapshot.program_relations {
        charge_sql_row(row, &mut integer_cells, &mut text_bytes, None)?;
    }
    if let Some(row) = &snapshot.universe {
        charge_sql_row(row, &mut integer_cells, &mut text_bytes, None)?;
        integer_cells = integer_cells
            .checked_add(1)
            .ok_or(IndexError::IntegerOutOfRange)?;
    }
    for row in &snapshot.obligations {
        charge_sql_row(row, &mut integer_cells, &mut text_bytes, None)?;
    }
    for row in &snapshot.obligation_lifecycle {
        charge_sql_row(row, &mut integer_cells, &mut text_bytes, None)?;
    }
    for row in &snapshot.executions {
        charge_sql_row(row, &mut integer_cells, &mut text_bytes, None)?;
    }
    for row in &snapshot.claims {
        charge_sql_row(row, &mut integer_cells, &mut text_bytes, None)?;
    }
    for row in &snapshot.artifact_registrations {
        charge_sql_row(row, &mut integer_cells, &mut text_bytes, Some("source"))?;
    }
    for row in &snapshot.snapshot_sources {
        charge_sql_row(row, &mut integer_cells, &mut text_bytes, None)?;
    }
    for row in &snapshot.context_envelopes {
        charge_sql_row(row, &mut integer_cells, &mut text_bytes, None)?;
    }
    for row in &snapshot.review_plans {
        charge_sql_row(row, &mut integer_cells, &mut text_bytes, None)?;
    }
    for row in &snapshot.shadows {
        charge_sql_row(row, &mut integer_cells, &mut text_bytes, None)?;
    }
    for row in &snapshot.projected_findings {
        charge_sql_row(row, &mut integer_cells, &mut text_bytes, None)?;
    }
    for row in &snapshot.evidence {
        charge_sql_row(row, &mut integer_cells, &mut text_bytes, None)?;
    }
    for row in &snapshot.evidence_bindings {
        charge_sql_row(row, &mut integer_cells, &mut text_bytes, None)?;
    }
    for row in &snapshot.verifications {
        charge_sql_row(row, &mut integer_cells, &mut text_bytes, None)?;
    }
    for row in &snapshot.decisions {
        charge_sql_row(row, &mut integer_cells, &mut text_bytes, None)?;
    }
    for row in &snapshot.findings {
        charge_sql_row(row, &mut integer_cells, &mut text_bytes, None)?;
    }
    for row in &snapshot.claim_assessments {
        charge_sql_row(row, &mut integer_cells, &mut text_bytes, None)?;
    }
    for row in &snapshot.artifact_registrations_v4 {
        charge_m5_event_tuple(&row.event_id, &mut integer_cells, &mut text_bytes)?;
        for text in [
            row.registration_id.as_str(),
            row.schema.as_str(),
            row.run_id.as_str(),
            row.cas_hash.as_str(),
            row.media_type.as_str(),
            row.sensitivity.as_str(),
            row.source_kind.as_str(),
            row.descriptor_id.as_str(),
            row.body_hash.as_str(),
        ] {
            add_text_charge(&mut text_bytes, text)?;
        }
        integer_cells = integer_cells
            .checked_add(1)
            .ok_or(IndexError::IntegerOutOfRange)?;
        let source =
            canonical_json(&row.source).map_err(|_| IndexError::ProjectionContractViolation)?;
        text_bytes = text_bytes
            .checked_add(u64::try_from(source.len()).map_err(|_| IndexError::IntegerOutOfRange)?)
            .ok_or(IndexError::IntegerOutOfRange)?;
    }
    for row in &snapshot.gluing_input_descriptors {
        charge_m5_event_tuple(&row.event_id, &mut integer_cells, &mut text_bytes)?;
        charge_m5_dto(&row.descriptor, &mut integer_cells, &mut text_bytes)?;
        for text in [
            row.registration_id.as_str(),
            row.descriptor_hash.as_str(),
            row.body_hash.as_str(),
        ] {
            add_text_charge(&mut text_bytes, text)?;
        }
        integer_cells = integer_cells
            .checked_add(1)
            .ok_or(IndexError::IntegerOutOfRange)?;
    }
    for row in &snapshot.context_covers {
        charge_m5_event_tuple(&row.event_id, &mut integer_cells, &mut text_bytes)?;
        charge_m5_dto(&row.cover, &mut integer_cells, &mut text_bytes)?;
        add_text_charge(&mut text_bytes, row.body_hash.as_str())?;
    }
    for row in &snapshot.sections {
        charge_m5_event_tuple(&row.event_id, &mut integer_cells, &mut text_bytes)?;
        charge_m5_dto(&row.section, &mut integer_cells, &mut text_bytes)?;
        add_text_charge(&mut text_bytes, row.body_hash.as_str())?;
    }
    for row in &snapshot.restrictions {
        charge_m5_event_tuple(&row.event_id, &mut integer_cells, &mut text_bytes)?;
        add_text_charge(&mut text_bytes, row.attempt_id.as_str())?;
        charge_m5_dto(&row.restriction, &mut integer_cells, &mut text_bytes)?;
        add_text_charge(&mut text_bytes, row.body_hash.as_str())?;
    }
    for row in &snapshot.gluing_attempts {
        charge_m5_event_tuple(&row.event_id, &mut integer_cells, &mut text_bytes)?;
        charge_m5_dto(&row.attempt, &mut integer_cells, &mut text_bytes)?;
        add_text_charge(&mut text_bytes, row.body_hash.as_str())?;
    }
    for row in &snapshot.global_candidates {
        charge_m5_event_tuple(&row.event_id, &mut integer_cells, &mut text_bytes)?;
        add_text_charge(&mut text_bytes, row.attempt_id.as_str())?;
        charge_m5_dto(&row.candidate, &mut integer_cells, &mut text_bytes)?;
        add_text_charge(&mut text_bytes, row.body_hash.as_str())?;
    }
    for row in &snapshot.gluing_obstructions {
        charge_m5_event_tuple(&row.event_id, &mut integer_cells, &mut text_bytes)?;
        charge_m5_dto(&row.obstruction, &mut integer_cells, &mut text_bytes)?;
        add_text_charge(&mut text_bytes, row.body_hash.as_str())?;
    }
    let sql_bytes = text_bytes
        .checked_add(
            integer_cells
                .checked_mul(8)
                .ok_or(IndexError::IntegerOutOfRange)?,
        )
        .and_then(|v| v.checked_add(rows))
        .ok_or(IndexError::IntegerOutOfRange)?;
    let query_bytes = json_length(snapshot)?;
    if query_bytes > limits.max_query_bytes {
        return Err(IndexError::Incomplete {
            limit: limits.max_query_bytes,
            observed: query_bytes,
        });
    }
    let owned_bytes = recursive_ownership_charge(snapshot)?;
    let max_cas = snapshot
        .artifact_registrations
        .iter()
        .map(|r| r.size)
        .chain(snapshot.artifact_registrations_v4.iter().map(|r| r.size))
        .max()
        .unwrap_or(0);
    let working_bytes = sql_bytes
        .checked_add(query_bytes)
        .and_then(|v| v.checked_add(owned_bytes))
        .and_then(|v| v.checked_add(max_cas))
        .and_then(|v| v.checked_add(max_event_line_bytes))
        .ok_or(IndexError::IntegerOutOfRange)?;
    if working_bytes > limits.max_working_bytes {
        return Err(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: working_bytes,
        });
    }
    Ok(IndexAccountingV5 {
        rows,
        integer_cells,
        text_bytes,
        sql_bytes,
        query_bytes,
        owned_bytes,
        observed_object_bytes: max_cas,
        observed_event_line_bytes: max_event_line_bytes,
        working_bytes,
    })
}

#[cfg(test)]
fn oracle_recursive_ownership(value: &Value) -> Result<u64, IndexError> {
    match value {
        Value::Null => Ok(0),
        Value::Bool(_) => Ok(1),
        Value::Number(_) => Ok(8),
        Value::String(text) => u64::try_from(text.len()).map_err(|_| IndexError::IntegerOutOfRange),
        Value::Array(items) => {
            let slots = u64::try_from(items.len())
                .map_err(|_| IndexError::IntegerOutOfRange)?
                .checked_mul(8)
                .ok_or(IndexError::IntegerOutOfRange)?;
            items.iter().try_fold(slots, |total, item| {
                total
                    .checked_add(oracle_recursive_ownership(item)?)
                    .ok_or(IndexError::IntegerOutOfRange)
            })
        }
        Value::Object(entries) => {
            let base = u64::try_from(entries.len())
                .map_err(|_| IndexError::IntegerOutOfRange)?
                .checked_mul(16)
                .ok_or(IndexError::IntegerOutOfRange)?;
            entries.iter().try_fold(base, |total, (key, value)| {
                total
                    .checked_add(
                        u64::try_from(key.len()).map_err(|_| IndexError::IntegerOutOfRange)?,
                    )
                    .and_then(|next| {
                        oracle_recursive_ownership(value)
                            .ok()
                            .and_then(|owned| next.checked_add(owned))
                    })
                    .ok_or(IndexError::IntegerOutOfRange)
            })
        }
    }
}

fn charge_sql_row<T: Serialize>(
    row: &T,
    integers: &mut u64,
    text: &mut u64,
    skip: Option<&str>,
) -> Result<(), IndexError> {
    let object = serde_json::to_value(row).map_err(|_| IndexError::ProjectionContractViolation)?;
    let object = object
        .as_object()
        .ok_or(IndexError::ProjectionContractViolation)?;
    for (key, value) in object {
        if skip == Some(key.as_str()) || value.is_null() {
            continue;
        }
        match value {
            Value::String(value) => {
                *text = (*text)
                    .checked_add(
                        u64::try_from(value.len()).map_err(|_| IndexError::IntegerOutOfRange)?,
                    )
                    .ok_or(IndexError::IntegerOutOfRange)?
            }
            Value::Number(_) | Value::Bool(_) => {
                *integers = (*integers)
                    .checked_add(1)
                    .ok_or(IndexError::IntegerOutOfRange)?
            }
            _ => return Err(IndexError::ProjectionContractViolation),
        }
    }
    Ok(())
}

fn build_connection(
    snapshot: &IndexSnapshotV5,
    limits: IndexLimits,
    retained: u64,
) -> Result<rusqlite::Connection, IndexError> {
    record_v5_sqlite_build();
    super::preflight_build_connection(limits, retained)?;
    let connection = rusqlite::Connection::open_in_memory_with_flags(
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
            | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
            | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    super::configure_connection(&connection, limits, retained)?;
    super::checked_batch(&connection, SCHEMA_V5, limits)?;
    connection.pragma_update(None, "user_version", INDEX_SCHEMA_VERSION_V5)?;
    let tx = connection.unchecked_transaction()?;
    tx.pragma_update(None, "defer_foreign_keys", true)?;
    insert_snapshot(&tx, snapshot)?;
    tx.commit()?;
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    let fk_violation = connection
        .prepare("PRAGMA foreign_key_check")?
        .query([])?
        .next()?
        .is_some();
    if integrity != "ok" || fk_violation {
        return Err(IndexError::ProjectionContractViolation);
    }
    Ok(connection)
}

fn insert_snapshot(tx: &rusqlite::Transaction<'_>, s: &IndexSnapshotV5) -> Result<(), IndexError> {
    use rusqlite::params;
    let m = &s.marker;
    tx.execute("INSERT INTO index_meta VALUES(1,5,?1,'reviewgraphen.review_event.v4','v4_gluing',?2,?3,?4,?5,?6,?7,?8)", params![m.projection_contract_version,m.run_id.to_string(),m.genesis_hash.to_string(),super::to_i64(m.confirmed_offset)?,m.tail_hash.to_string(),super::to_i64(m.event_count)?,m.policy_revision_hash.to_string(),m.authority_replay_basis_digest.to_string()]).map_err(super::map_sql)?;
    for r in &s.events {
        tx.execute(
            "INSERT INTO events VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                super::to_i64(r.sequence)?,
                r.event_id.to_string(),
                r.schema,
                r.event_hash.to_string(),
                r.payload_hash.to_string(),
                r.payload_kind,
                r.actor,
                super::to_i64(r.logical_time)?
            ],
        )
        .map_err(super::map_sql)?;
    }
    for r in &s.program_objects {
        tx.execute(
            "INSERT INTO program_objects VALUES(?1,?2,?3)",
            params![
                r.object_id.to_string(),
                r.object_kind,
                r.body_hash.to_string()
            ],
        )
        .map_err(super::map_sql)?;
    }
    for r in &s.program_relations {
        tx.execute(
            "INSERT INTO program_relations VALUES(?1,?2,?3,?4,?5)",
            params![
                r.relation_id.to_string(),
                r.relation_kind,
                r.source_id.to_string(),
                r.target_ids_canonical_json,
                r.body_hash.to_string()
            ],
        )
        .map_err(super::map_sql)?;
    }
    if let Some(r) = &s.universe {
        tx.execute(
            "INSERT INTO universe VALUES(1,?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                r.universe_id.to_string(),
                r.snapshot_id.to_string(),
                r.profile_id,
                r.rule_set_hash.to_string(),
                r.extractor_set_hash.to_string(),
                r.policy_version,
                r.rule_pack_version,
                r.body_hash.to_string()
            ],
        )
        .map_err(super::map_sql)?;
    }
    for r in &s.obligations {
        tx.execute(
            "INSERT INTO obligations VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                r.obligation_id.to_string(),
                r.target_kind,
                r.target_ids_canonical_json,
                r.property_id,
                r.lifecycle,
                r.body_hash.to_string()
            ],
        )
        .map_err(super::map_sql)?;
    }
    for r in &s.obligation_lifecycle {
        tx.execute(
            "INSERT INTO obligation_lifecycle VALUES(?1,?2,?3,?4)",
            params![
                super::to_i64(r.event_sequence)?,
                r.event_id.to_string(),
                r.obligation_id.to_string(),
                r.next_lifecycle
            ],
        )
        .map_err(super::map_sql)?;
    }
    for r in &s.artifact_registrations {
        tx.execute(
            "INSERT INTO artifact_registrations VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                super::to_i64(r.event_sequence)?,
                r.event_id.to_string(),
                r.registration_id.to_string(),
                r.run_id.to_string(),
                r.cas_hash.to_string(),
                r.media_type,
                super::to_i64(r.size)?,
                r.sensitivity,
                r.source_kind,
                r.source_canonical_json,
                r.body_hash.to_string()
            ],
        )
        .map_err(super::map_sql)?;
    }
    for r in &s.snapshot_sources {
        tx.execute(
            "INSERT INTO snapshot_source_index VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                super::to_i64(r.event_sequence)?,
                r.event_id.to_string(),
                r.snapshot_id.to_string(),
                r.artifact_id.to_string(),
                r.registration_id.to_string(),
                r.path,
                r.content_hash.to_string(),
                r.cas_hash.to_string(),
                super::to_i64(r.line_count)?
            ],
        )
        .map_err(super::map_sql)?;
    }
    for r in &s.review_plans {
        tx.execute(
            "INSERT INTO review_plans VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![
                super::to_i64(r.event_sequence)?,
                r.event_id.to_string(),
                r.plan_id.to_string(),
                r.universe_id.to_string(),
                r.snapshot_id.to_string(),
                r.planner_input_hash.to_string(),
                r.planner_policy_version,
                r.planner_policy_hash.to_string(),
                r.budget_canonical_json,
                r.budget_hash.to_string(),
                r.risk_breakdown_canonical_json,
                r.waves_canonical_json,
                r.deferred_canonical_json,
                r.identity_body_hash.to_string(),
                r.body_hash.to_string()
            ],
        )
        .map_err(super::map_sql)?;
    }
    for r in &s.context_envelopes {
        tx.execute("INSERT INTO context_envelopes VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",params![super::to_i64(r.event_sequence)?,r.event_id.to_string(),r.envelope_id.to_string(),r.snapshot_id.to_string(),r.context_policy_version,r.context_policy_hash.to_string(),r.candidate_ids_canonical_json,r.obligation_ids_canonical_json,r.context_policy_canonical_json,r.included_sources_canonical_json,r.excluded_sources_canonical_json,r.unknowns_canonical_json,r.assumptions_canonical_json,r.losses_canonical_json,r.projection_hash.to_string(),r.body_hash.to_string()]).map_err(super::map_sql)?;
    }
    for r in &s.executions {
        tx.execute("INSERT INTO executions VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26)",params![super::to_i64(r.event_sequence)?,r.event_id.to_string(),r.execution_id.to_string(),r.plan_id.to_string(),r.wave_id.to_string(),r.snapshot_id.to_string(),r.envelope_id.to_string(),r.obligation_ids_canonical_json,r.reviewer_kind,r.reviewer_id,r.provider,r.model,r.model_revision,r.system_prompt_version,r.prompt_template_version,r.inference_settings_canonical_json,r.tool_policy_version,r.tool_calls_canonical_json,i64::from(r.attempt),r.raw_registration_id.to_string(),r.raw_hash.to_string(),r.parsed_claim_ids_canonical_json,r.outcome_kind,r.outcome_canonical_json,r.identity_body_hash.to_string(),r.body_hash.to_string()]).map_err(super::map_sql)?;
    }
    for r in &s.claims {
        tx.execute("INSERT INTO claims VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",params![super::to_i64(r.event_sequence)?,r.event_id.to_string(),r.claim_id.to_string(),r.execution_id.to_string(),r.obligation_ids_canonical_json,r.property_id,r.target_refs_canonical_json,r.polarity,r.disposition,r.summary,r.source_ids_canonical_json,r.assumptions_canonical_json,r.requested_evidence_canonical_json,r.candidate_confidence_canonical_json,r.author_kind,r.review_status,r.identity_body_hash.to_string(),r.body_hash.to_string()]).map_err(super::map_sql)?;
    }
    for r in &s.shadows {
        tx.execute(
            "INSERT INTO unreconciled_authority_records VALUES(?1,?2,?3,?4,?5,0)",
            params![
                super::to_i64(r.event_sequence)?,
                r.event_id.to_string(),
                r.record_id.to_string(),
                r.kind,
                r.body_hash.to_string()
            ],
        )
        .map_err(super::map_sql)?;
    }
    for r in &s.projected_findings {
        tx.execute(
            "INSERT INTO projected_findings VALUES(?1,?2,?3,?4,?5)",
            params![
                super::to_i64(r.event_sequence)?,
                r.event_id.to_string(),
                r.finding_id.to_string(),
                r.body_hash.to_string(),
                r.projection_status
            ],
        )
        .map_err(super::map_sql)?;
    }
    for r in &s.evidence {
        tx.execute(
            "INSERT INTO evidence_v3 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                super::to_i64(r.event_sequence)?,
                r.event_id.to_string(),
                r.evidence_id.to_string(),
                r.schema,
                r.kind,
                r.snapshot_id.to_string(),
                r.subject_ids_canonical_json,
                r.descriptor_id,
                r.procedure_version,
                r.input_registration_id.to_string(),
                r.output_registration_id.to_string(),
                r.observation,
                r.body_hash.to_string()
            ],
        )
        .map_err(super::map_sql)?;
    }
    for r in &s.evidence_bindings {
        tx.execute(
            "INSERT INTO evidence_bindings_v3 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                super::to_i64(r.event_sequence)?,
                r.event_id.to_string(),
                r.binding_id.to_string(),
                r.schema,
                r.claim_id.to_string(),
                r.evidence_id.to_string(),
                r.relation,
                r.property_id,
                r.body_hash.to_string()
            ],
        )
        .map_err(super::map_sql)?;
    }
    for r in &s.verifications {
        tx.execute(
            "INSERT INTO verifications_v3 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                super::to_i64(r.event_sequence)?,
                r.event_id.to_string(),
                r.verification_id.to_string(),
                r.schema,
                r.claim_id.to_string(),
                r.descriptor_id,
                r.procedure_version,
                r.input_registration_id.to_string(),
                r.output_registration_id.to_string(),
                r.evidence_ids_canonical_json,
                r.outcome,
                r.limitations_canonical_json,
                r.body_hash.to_string()
            ],
        )
        .map_err(super::map_sql)?;
    }
    for r in &s.decisions {
        tx.execute("INSERT INTO decisions_v3 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",params![super::to_i64(r.event_sequence)?,r.event_id.to_string(),r.decision_id.to_string(),r.schema,r.policy_revision_hash.to_string(),r.run_id.to_string(),r.universe_id.to_string(),r.claim_id.to_string(),r.property_id,r.outcome,r.actor,r.authority_id,r.snapshot_id.to_string(),r.source_ids_canonical_json,r.rationale,r.issued_at,r.expires_at,r.body_hash.to_string()]).map_err(super::map_sql)?;
    }
    for r in &s.findings {
        tx.execute(
            "INSERT INTO findings_v3 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                super::to_i64(r.event_sequence)?,
                r.event_id.to_string(),
                r.finding_id.to_string(),
                r.schema,
                r.projection_descriptor_id,
                r.claim_id.to_string(),
                r.status,
                r.evidence_ids_canonical_json,
                r.verification_ids_canonical_json,
                r.decision_id.as_ref().map(ToString::to_string),
                r.supersedes_finding_id.as_ref().map(ToString::to_string),
                r.body_hash.to_string()
            ],
        )
        .map_err(super::map_sql)?;
    }
    for r in &s.claim_assessments {
        tx.execute(
            "INSERT INTO claim_assessments_v3 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                r.claim_id.to_string(),
                r.disposition,
                r.review_status,
                r.binding_ids_canonical_json,
                r.evidence_ids_canonical_json,
                r.verification_ids_canonical_json,
                r.decision_ids_canonical_json,
                r.finding_ids_canonical_json,
                r.active_decision_id.as_ref().map(ToString::to_string),
                r.current_finding_id.as_ref().map(ToString::to_string),
                i64::from(r.decision_conflict),
                super::to_i64(r.confirmed_event_sequence)?
            ],
        )
        .map_err(super::map_sql)?;
    }
    for r in &s.artifact_registrations_v4 {
        let source = String::from_utf8(
            canonical_json(&r.source).map_err(|_| IndexError::ProjectionContractViolation)?,
        )
        .map_err(|_| IndexError::ProjectionContractViolation)?;
        tx.execute("INSERT INTO artifact_registrations_v4 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)", params![
            super::to_i64(r.event_sequence)?, r.event_id.to_string(), r.registration_id.to_string(), r.schema,
            r.run_id.to_string(), r.cas_hash.to_string(), r.media_type, super::to_i64(r.size)?, r.sensitivity,
            r.source_kind, source, r.descriptor_id.to_string(), r.body_hash.to_string()
        ]).map_err(super::map_sql)?;
    }
    for r in &s.gluing_input_descriptors {
        let o = canonical_object(&r.descriptor)?;
        tx.execute("INSERT INTO gluing_input_descriptors_v4 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)", params![
            super::to_i64(r.event_sequence)?, r.event_id.to_string(), id(&o,"id")?.to_string(), r.registration_id.to_string(),
            string(&o,"schema")?, id(&o,"run_id")?.to_string(), id(&o,"snapshot_id")?.to_string(), id(&o,"universe_id")?.to_string(),
            id(&o,"plan_id")?.to_string(), string(&o,"profile_descriptor_id")?, id(&o,"context_id")?.to_string(),
            string(&o,"assignment_key")?, string(&o,"assignment_value")?, canonical_component(&o,"qualification_source_ids")?,
            r.descriptor_hash.to_string(), super::to_i64(r.descriptor_size)?, r.body_hash.to_string()
        ]).map_err(super::map_sql)?;
    }
    for r in &s.context_covers {
        let o = canonical_object(&r.cover)?;
        tx.execute("INSERT INTO context_covers_v4 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)", params![
            super::to_i64(r.event_sequence)?, r.event_id.to_string(), id(&o,"id")?.to_string(), string(&o,"schema")?,
            id(&o,"run_id")?.to_string(), id(&o,"snapshot_id")?.to_string(), id(&o,"universe_id")?.to_string(), id(&o,"plan_id")?.to_string(),
            string(&o,"profile_descriptor_id")?, canonical_component(&o,"selected_obligation_ids")?, canonical_component(&o,"required_context_ids")?,
            canonical_component(&o,"cover_domain_ids")?, canonical_component(&o,"covered_domain_ids")?, canonical_component(&o,"uncovered_domain_ids")?,
            canonical_component(&o,"source_ids")?, r.body_hash.to_string()
        ]).map_err(super::map_sql)?;
    }
    for r in &s.sections {
        let o = canonical_object(&r.section)?;
        tx.execute("INSERT INTO sections_v4 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25)", params![
            super::to_i64(r.event_sequence)?, r.event_id.to_string(), id(&o,"id")?.to_string(), string(&o,"schema")?, id(&o,"cover_id")?.to_string(),
            id(&o,"context_id")?.to_string(), id(&o,"snapshot_id")?.to_string(), string(&o,"property_id")?, id(&o,"invariant_id")?.to_string(),
            id(&o,"obligation_id")?.to_string(), id(&o,"claim_id")?.to_string(), id(&o,"claim_assessment_id")?.to_string(), id(&o,"input_descriptor_id")?.to_string(),
            id(&o,"input_registration_id")?.to_string(), string(&o,"assignment_key")?, string(&o,"assignment_value")?, i64::from(boolean(&o,"passed_current_verification")?),
            canonical_component(&o,"source_ids")?, canonical_component(&o,"qualification_source_ids")?, canonical_component(&o,"binding_ids")?, canonical_component(&o,"evidence_ids")?,
            canonical_component(&o,"verification_ids")?, canonical_component(&o,"decision_ids")?, canonical_component(&o,"finding_ids")?, r.body_hash.to_string()
        ]).map_err(super::map_sql)?;
    }
    for r in &s.gluing_attempts {
        let o = canonical_object(&r.attempt)?;
        tx.execute("INSERT INTO gluing_attempts_v4 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)", params![
            super::to_i64(r.event_sequence)?, r.event_id.to_string(), id(&o,"id")?.to_string(), string(&o,"schema")?, id(&o,"cover_id")?.to_string(),
            id(&o,"snapshot_id")?.to_string(), string(&o,"property_id")?, id(&o,"invariant_id")?.to_string(), canonical_component(&o,"input_descriptor_ids")?,
            canonical_component(&o,"section_ids")?, canonical_component(&o,"restriction_ids")?, string(&o,"result")?, optional_id(&o,"global_candidate_id")?.map(|v|v.to_string()),
            optional_id(&o,"obstruction_id")?.map(|v|v.to_string()), canonical_component(&o,"source_ids")?, canonical_component(&o,"claim_ids")?, canonical_component(&o,"evidence_ids")?,
            canonical_component(&o,"verification_ids")?, canonical_component(&o,"decision_ids")?, canonical_component(&o,"finding_ids")?, r.body_hash.to_string()
        ]).map_err(super::map_sql)?;
    }
    for r in &s.restrictions {
        let o = canonical_object(&r.restriction)?;
        tx.execute("INSERT INTO restrictions_v4 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)", params![
            super::to_i64(r.event_sequence)?, r.event_id.to_string(), r.attempt_id.to_string(), id(&o,"id")?.to_string(), string(&o,"schema")?, id(&o,"section_id")?.to_string(),
            canonical_component(&o,"context_pair")?, canonical_component(&o,"overlap_member_ids")?, string(&o,"assignment_key")?, string(&o,"assignment_value")?,
            canonical_component(&o,"source_ids")?, canonical_component(&o,"qualification_source_ids")?, canonical_component(&o,"claim_ids")?, canonical_component(&o,"evidence_ids")?,
            canonical_component(&o,"verification_ids")?, canonical_component(&o,"decision_ids")?, canonical_component(&o,"finding_ids")?, r.body_hash.to_string()
        ]).map_err(super::map_sql)?;
    }
    for r in &s.global_candidates {
        let o = canonical_object(&r.candidate)?;
        tx.execute("INSERT INTO global_candidates_v4 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)", params![
            super::to_i64(r.event_sequence)?, r.event_id.to_string(), r.attempt_id.to_string(), id(&o,"id")?.to_string(), string(&o,"schema")?, id(&o,"cover_id")?.to_string(),
            id(&o,"invariant_id")?.to_string(), string(&o,"property_id")?, canonical_component(&o,"required_section_ids")?, canonical_component(&o,"restriction_ids")?,
            canonical_component(&o,"qualification_source_ids")?, canonical_component(&o,"source_ids")?, canonical_component(&o,"claim_ids")?, canonical_component(&o,"evidence_ids")?,
            canonical_component(&o,"verification_ids")?, canonical_component(&o,"decision_ids")?, canonical_component(&o,"finding_ids")?, r.body_hash.to_string()
        ]).map_err(super::map_sql)?;
    }
    for r in &s.gluing_obstructions {
        let o = canonical_object(&r.obstruction)?;
        let attempt_id = id(&o, "attempt_id")?;
        tx.execute("INSERT INTO gluing_obstructions_v4 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24)", params![
            super::to_i64(r.event_sequence)?, r.event_id.to_string(), attempt_id.to_string(), id(&o,"id")?.to_string(), string(&o,"schema")?, string(&o,"kind")?,
            canonical_component(&o,"conflicting_context_ids")?, canonical_component(&o,"section_ids")?, canonical_component(&o,"overlap_member_ids")?, string(&o,"assignment_key")?,
            optional_string(&o,"left_assignment_value")?, optional_string(&o,"right_assignment_value")?, canonical_component(&o,"source_ids")?, canonical_component(&o,"claim_ids")?,
            canonical_component(&o,"evidence_ids")?, canonical_component(&o,"verification_ids")?, canonical_component(&o,"decision_ids")?, canonical_component(&o,"finding_ids")?,
            id(&o,"affected_invariant_id")?.to_string(), string(&o,"severity")?, string(&o,"required_resolution")?, i64::from(boolean(&o,"human_decision_required")?),
            canonical_component(&o,"blocks")?, r.body_hash.to_string()
        ]).map_err(super::map_sql)?;
    }
    Ok(())
}

impl DerivedIndexV5<'_> {
    fn publish_v5_image_locked(
        &self,
        image: Vec<u8>,
        snapshot: &IndexSnapshotV5,
        accounting: &IndexAccountingV5,
        sqlite_limits: IndexLimits,
    ) -> Result<ContentHash, IndexError> {
        super::check_image_len(image.len(), self.inner.limits)?;
        self.inner.audit_candidates(true)?;
        let hash = ContentHash::sha256(&image);
        let mut nonce = [0_u8; 32];
        getrandom(&mut nonce, GetRandomFlags::empty()).map_err(crate::StoreError::Io)?;
        let name = super::candidate_name(&nonce);
        let tmp = fs::openat(
            &self.inner.indexes,
            ".",
            OFlags::RDWR | OFlags::TMPFILE | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )
        .map_err(|error| {
            if error == rustix::io::Errno::NOSYS || error == rustix::io::Errno::OPNOTSUPP {
                IndexError::UnsupportedPlatform
            } else {
                IndexError::Store(crate::StoreError::Io(error))
            }
        })?;
        super::verify_index_fd(&tmp, FileType::RegularFile, 0o600)?;
        let mut file = File::from(tmp);
        file.write_all(&image)?;
        #[cfg(test)]
        if self
            .inner
            .take_publish_fault(super::PublishFault::AfterWrite)
        {
            return Err(super::test_publish_failure("v5 after candidate write"));
        }
        file.sync_all()?;
        #[cfg(test)]
        if self
            .inner
            .take_publish_fault(super::PublishFault::BeforeCandidateSync)
        {
            return Err(super::test_publish_failure(
                "v5 after candidate file sync before link",
            ));
        }
        let tmp: OwnedFd = file.into();
        fs::linkat(&tmp, "", &self.inner.indexes, &name, AtFlags::EMPTY_PATH).map_err(|error| {
            if error == rustix::io::Errno::EXIST {
                IndexError::IndexPathRace
            } else {
                IndexError::Store(crate::StoreError::Io(error))
            }
        })?;
        #[cfg(test)]
        if self
            .inner
            .take_publish_fault(super::PublishFault::AfterCandidateLinkBeforeDirectorySync)
        {
            return Err(super::test_publish_failure(
                "v5 after candidate link before directory sync",
            ));
        }
        fs::fsync(&self.inner.indexes)?;
        #[cfg(test)]
        if self
            .inner
            .take_publish_fault(super::PublishFault::AfterCandidateSync)
        {
            return Err(super::test_publish_failure(
                "v5 after candidate directory sync",
            ));
        }
        let expected_hash = hash.clone();
        drop(image);
        #[cfg(test)]
        if self
            .inner
            .take_publish_fault(super::PublishFault::AfterImageDropBeforeCandidateRead)
        {
            return Err(super::test_publish_failure(
                "v5 before candidate read allocation",
            ));
        }
        let candidate = self.inner.open_entry(&name)?;
        super::ensure_same_inode(&fs::fstat(&tmp)?, &fs::fstat(&candidate)?)?;
        #[cfg(test)]
        if self
            .inner
            .take_publish_fault(super::PublishFault::AfterCandidateInodeCheck)
        {
            return Err(super::test_publish_failure(
                "v5 after candidate inode check",
            ));
        }
        let candidate_bytes = super::read_fd_exact_with_retained(
            candidate,
            sqlite_limits.max_serialized_bytes,
            accounting.owned_bytes,
            sqlite_limits.max_working_bytes,
        )?;
        if ContentHash::sha256(&candidate_bytes) != expected_hash {
            return Err(IndexError::CorruptIndex);
        }
        #[cfg(test)]
        if self
            .inner
            .take_publish_fault(super::PublishFault::AfterCandidateHashCheck)
        {
            return Err(super::test_publish_failure("v5 after candidate hash check"));
        }
        validate_v5_image(
            candidate_bytes,
            snapshot,
            sqlite_limits,
            accounting.owned_bytes,
        )?;
        #[cfg(test)]
        if self
            .inner
            .take_publish_fault(super::PublishFault::AfterValidation)
        {
            return Err(super::test_publish_failure("v5 after candidate validation"));
        }
        fs::renameat(
            &self.inner.indexes,
            &name,
            &self.inner.indexes,
            super::ACTIVE_FILE,
        )
        .map_err(|error| IndexError::Store(crate::StoreError::Io(error)))?;
        #[cfg(test)]
        if self
            .inner
            .take_publish_fault(super::PublishFault::AfterRename)
        {
            return Err(IndexError::PublicationDurabilityUncertain {
                run_id: snapshot.marker.run_id.clone(),
                tail_hash: snapshot.marker.tail_hash.clone(),
                image_hash: expected_hash,
            });
        }
        let durable = (|| -> Result<(), IndexError> {
            let active = self.inner.open_entry(super::ACTIVE_FILE)?;
            super::ensure_same_inode(&fs::fstat(&tmp)?, &fs::fstat(&active)?)?;
            #[cfg(test)]
            if self
                .inner
                .take_publish_fault(super::PublishFault::AfterActiveInodeCheck)
            {
                return Err(super::test_publish_failure("v5 after active inode check"));
            }
            let bytes = super::read_fd_exact_with_retained(
                active,
                sqlite_limits.max_serialized_bytes,
                accounting.owned_bytes,
                sqlite_limits.max_working_bytes,
            )?;
            if ContentHash::sha256(&bytes) != expected_hash {
                return Err(IndexError::CorruptIndex);
            }
            #[cfg(test)]
            if self
                .inner
                .take_publish_fault(super::PublishFault::AfterActiveHashCheck)
            {
                return Err(super::test_publish_failure("v5 after active hash check"));
            }
            #[cfg(test)]
            if self
                .inner
                .take_publish_fault(super::PublishFault::AfterActiveVerify)
            {
                return Err(super::test_publish_failure("v5 after active verification"));
            }
            #[cfg(test)]
            if self
                .inner
                .take_publish_fault(super::PublishFault::BeforeFinalDirectorySync)
            {
                return Err(super::test_publish_failure(
                    "v5 before final directory sync",
                ));
            }
            fs::fsync(&self.inner.indexes)?;
            Ok(())
        })();
        if durable.is_err() {
            return Err(IndexError::PublicationDurabilityUncertain {
                run_id: snapshot.marker.run_id.clone(),
                tail_hash: snapshot.marker.tail_hash.clone(),
                image_hash: expected_hash,
            });
        }
        Ok(hash)
    }
}

fn marker_v5_from_connection(
    connection: &rusqlite::Connection,
) -> Result<IndexMarkerV5, IndexError> {
    let raw: (i64, String, String, String, String, String, i64, String, i64, String, String) =
        connection
            .query_row(
                "SELECT index_schema_version,projection_contract_version,event_contract_version,projection_mode,run_id,genesis_hash,confirmed_offset,tail_hash,event_count,policy_revision_hash,authority_replay_basis_digest FROM index_meta WHERE singleton=1",
                [],
                |row| {
                    Ok((
                        row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?,
                        row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?,
                        row.get(10)?,
                    ))
                },
            )
            .map_err(|_| IndexError::CorruptIndex)?;
    let marker = IndexMarkerV5 {
        index_schema_version: u64::try_from(raw.0).map_err(|_| IndexError::CorruptIndex)?,
        sqlite_user_version: INDEX_SCHEMA_VERSION_V5.into(),
        projection_contract_version: raw.1,
        event_contract_version: raw.2,
        projection_mode: raw.3,
        run_id: StableId::parse(raw.4).map_err(|_| IndexError::CorruptIndex)?,
        genesis_hash: ContentHash::parse(raw.5).map_err(|_| IndexError::CorruptIndex)?,
        confirmed_offset: u64::try_from(raw.6).map_err(|_| IndexError::CorruptIndex)?,
        tail_hash: ContentHash::parse(raw.7).map_err(|_| IndexError::CorruptIndex)?,
        event_count: u64::try_from(raw.8).map_err(|_| IndexError::CorruptIndex)?,
        policy_revision_hash: ContentHash::parse(raw.9).map_err(|_| IndexError::CorruptIndex)?,
        authority_replay_basis_digest: ContentHash::parse(raw.10)
            .map_err(|_| IndexError::CorruptIndex)?,
    };
    if marker.index_schema_version != u64::from(INDEX_SCHEMA_VERSION_V5)
        || marker.projection_contract_version != PROJECTION_CONTRACT_VERSION_V5
        || marker.event_contract_version != EVENT_CONTRACT_VERSION_V4
        || marker.projection_mode != PROJECTION_MODE_V4
    {
        return Err(IndexError::CorruptIndex);
    }
    Ok(marker)
}

fn validate_v5_image(
    bytes: Vec<u8>,
    expected: &IndexSnapshotV5,
    limits: IndexLimits,
    retained: u64,
) -> Result<(), IndexError> {
    let actual_hash = ContentHash::sha256(&bytes);
    let connection =
        super::deserialize_read_only_for_schema(bytes, limits, retained, INDEX_SCHEMA_VERSION_V5)
            .map_err(super::normalize_external_image_error)?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != 5 {
        return Err(IndexError::CorruptIndex);
    }
    validate_v5_structure(&connection, limits)?;
    let marker: (String,String,String,String) = connection.query_row(
        "SELECT projection_contract_version,event_contract_version,projection_mode,authority_replay_basis_digest FROM index_meta WHERE singleton=1",
        [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
    ).map_err(|_| IndexError::CorruptIndex)?;
    if marker
        != (
            PROJECTION_CONTRACT_VERSION_V5.to_owned(),
            EVENT_CONTRACT_VERSION_V4.to_owned(),
            PROJECTION_MODE_V4.to_owned(),
            expected.authority_replay_basis_digest.to_string(),
        )
    {
        return Err(IndexError::CorruptIndex);
    }
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok"
        || connection
            .prepare("PRAGMA foreign_key_check")?
            .query([])?
            .next()?
            .is_some()
    {
        return Err(IndexError::CorruptIndex);
    }
    validate_v5_rows(&connection, expected, limits)?;
    drop(connection);
    // Validate every row/value by independently rebuilding the complete
    // source-derived image after releasing the candidate buffer.
    let expected_connection = build_connection(expected, limits, retained)?;
    let expected_bytes =
        super::serialize_connection_with_retained(&expected_connection, limits, retained)?;
    if ContentHash::sha256(&expected_bytes) != actual_hash {
        return Err(IndexError::CorruptIndex);
    }
    Ok(())
}

enum ExpectedCellV5<'a> {
    U64(u64),
    Bool(bool),
    Text(&'a str),
    OptionalText(Option<&'a str>),
    Id(&'a StableId),
    OptionalId(Option<&'a StableId>),
    Hash(&'a ContentHash),
    CanonicalJson(&'a str),
    ContextPolicy(&'a str),
    ArtifactSource {
        canonical: &'a str,
        source: &'a Value,
        source_kind: &'a str,
        run_id: &'a StableId,
    },
}

fn corrupt<T>() -> Result<T, IndexError> {
    Err(IndexError::CorruptIndex)
}

fn compare_text(
    actual: ValueRef<'_>,
    expected: &[u8],
    limits: IndexLimits,
) -> Result<(), IndexError> {
    let ValueRef::Text(actual) = actual else {
        return corrupt();
    };
    let length = u64::try_from(actual.len()).map_err(|_| IndexError::IntegerOutOfRange)?;
    if length > limits.max_query_bytes || actual != expected {
        return corrupt();
    }
    Ok(())
}

fn source_kind_and_run_id(source: &ArtifactSourceV3) -> (&'static str, &StableId) {
    match source {
        ArtifactSourceV3::RunGenesis { run_id } => ("run_genesis", run_id),
        ArtifactSourceV3::SnapshotIngest { run_id, .. } => ("snapshot_ingest", run_id),
        ArtifactSourceV3::ReviewerExecution { run_id, .. } => ("reviewer_execution", run_id),
        ArtifactSourceV3::VerifierArtifact { run_id, .. } => ("verifier_artifact", run_id),
        ArtifactSourceV3::ExternalHarnessWitness { run_id, .. } => {
            ("external_harness_witness", run_id)
        }
    }
}

fn source_v4_kind_and_run_id(source: &ArtifactSourceV4) -> (&'static str, &StableId) {
    match source {
        ArtifactSourceV4::RunGenesis { run_id } => ("run_genesis", run_id),
        ArtifactSourceV4::SnapshotIngest { run_id, .. } => ("snapshot_ingest", run_id),
        ArtifactSourceV4::ReviewerExecution { run_id, .. } => ("reviewer_execution", run_id),
        ArtifactSourceV4::VerifierArtifact { run_id, .. } => ("verifier_artifact", run_id),
        ArtifactSourceV4::ExternalHarnessWitness { run_id, .. } => {
            ("external_harness_witness", run_id)
        }
        ArtifactSourceV4::GluingInput { run_id, .. } => ("gluing_input", run_id),
    }
}

fn compare_v5_row(
    row: &Row<'_>,
    expected: &[ExpectedCellV5<'_>],
    limits: IndexLimits,
) -> Result<(), IndexError> {
    if row.as_ref().column_count() != expected.len() {
        return corrupt();
    }
    for (index, expected) in expected.iter().enumerate() {
        let actual = row.get_ref(index).map_err(|_| IndexError::CorruptIndex)?;
        match expected {
            ExpectedCellV5::U64(value) => {
                let ValueRef::Integer(actual) = actual else {
                    return corrupt();
                };
                if actual != super::to_i64(*value)? {
                    return corrupt();
                }
            }
            ExpectedCellV5::Bool(value) => {
                let ValueRef::Integer(actual) = actual else {
                    return corrupt();
                };
                if actual != i64::from(*value) {
                    return corrupt();
                }
            }
            ExpectedCellV5::Text(value) => compare_text(actual, value.as_bytes(), limits)?,
            ExpectedCellV5::OptionalText(value) => match value {
                Some(value) => compare_text(actual, value.as_bytes(), limits)?,
                None if matches!(actual, ValueRef::Null) => {}
                None => return corrupt(),
            },
            ExpectedCellV5::Id(value) => {
                let value = value.to_string();
                compare_text(actual, value.as_bytes(), limits)?;
            }
            ExpectedCellV5::OptionalId(value) => match value {
                Some(value) => {
                    let value = value.to_string();
                    compare_text(actual, value.as_bytes(), limits)?;
                }
                None if matches!(actual, ValueRef::Null) => {}
                None => return corrupt(),
            },
            ExpectedCellV5::Hash(value) => {
                let value = value.to_string();
                compare_text(actual, value.as_bytes(), limits)?;
            }
            ExpectedCellV5::CanonicalJson(value) => {
                compare_text(actual, value.as_bytes(), limits)?;
                let parsed: Value =
                    serde_json::from_str(value).map_err(|_| IndexError::CorruptIndex)?;
                if canonical_json(&parsed).map_err(|_| IndexError::CorruptIndex)?
                    != value.as_bytes()
                {
                    return corrupt();
                }
            }
            ExpectedCellV5::ContextPolicy(value) => {
                compare_text(actual, value.as_bytes(), limits)?;
                let canonical = reviewgraphen_core::ContextPolicyV1::baseline()
                    .canonical_bytes()
                    .map_err(|_| IndexError::CorruptIndex)?;
                if canonical != value.as_bytes() {
                    return corrupt();
                }
            }
            ExpectedCellV5::ArtifactSource {
                canonical,
                source,
                source_kind,
                run_id,
            } => {
                compare_text(actual, canonical.as_bytes(), limits)?;
                let decoded: Value =
                    serde_json::from_str(canonical).map_err(|_| IndexError::CorruptIndex)?;
                if &decoded != *source
                    || canonical_json(&decoded).map_err(|_| IndexError::CorruptIndex)?
                        != canonical.as_bytes()
                {
                    return corrupt();
                }
                let object = decoded.as_object().ok_or(IndexError::CorruptIndex)?;
                let decoded_kind = object
                    .get("kind")
                    .and_then(Value::as_str)
                    .ok_or(IndexError::CorruptIndex)?;
                let decoded_run_id = object
                    .get("run_id")
                    .and_then(Value::as_str)
                    .ok_or(IndexError::CorruptIndex)?;
                if decoded_kind != *source_kind || decoded_run_id != run_id.as_str() {
                    return corrupt();
                }
            }
        }
    }
    Ok(())
}

macro_rules! validate_v5_table {
    ($connection:expr, $limits:expr, $sql:literal, $rows:expr, |$row:ident| $cells:expr) => {{
        let mut statement = $connection
            .prepare($sql)
            .map_err(|_| IndexError::CorruptIndex)?;
        let mut actual = statement.query([]).map_err(|_| IndexError::CorruptIndex)?;
        for $row in $rows {
            let Some(found) = actual.next().map_err(|_| IndexError::CorruptIndex)? else {
                return corrupt();
            };
            compare_v5_row(found, &$cells, $limits)?;
        }
        if actual
            .next()
            .map_err(|_| IndexError::CorruptIndex)?
            .is_some()
        {
            return corrupt();
        }
    }};
}

fn compare_v5_json_row(
    row: &Row<'_>,
    expected: &[Value],
    limits: IndexLimits,
) -> Result<(), IndexError> {
    if row.as_ref().column_count() != expected.len() {
        return corrupt();
    }
    for (index, expected) in expected.iter().enumerate() {
        let actual = row.get_ref(index).map_err(|_| IndexError::CorruptIndex)?;
        match expected {
            Value::Null if matches!(actual, ValueRef::Null) => {}
            Value::Null => return corrupt(),
            Value::Bool(expected) => {
                let ValueRef::Integer(actual) = actual else {
                    return corrupt();
                };
                if actual != i64::from(*expected) {
                    return corrupt();
                }
            }
            Value::Number(expected) => {
                let expected = expected.as_u64().ok_or(IndexError::CorruptIndex)?;
                let ValueRef::Integer(actual) = actual else {
                    return corrupt();
                };
                if actual != super::to_i64(expected)? {
                    return corrupt();
                }
            }
            Value::String(expected) => compare_text(actual, expected.as_bytes(), limits)?,
            Value::Array(_) | Value::Object(_) => {
                let canonical = canonical_json(expected).map_err(|_| IndexError::CorruptIndex)?;
                compare_text(actual, &canonical, limits)?;
            }
        }
    }
    Ok(())
}

fn m5_item_object<T: Serialize>(value: &T) -> Result<Map<String, Value>, IndexError> {
    serde_json::to_value(value)
        .map_err(|_| IndexError::CorruptIndex)?
        .as_object()
        .cloned()
        .ok_or(IndexError::CorruptIndex)
}

fn take_m5_fields(object: &Map<String, Value>, fields: &[&str]) -> Result<Vec<Value>, IndexError> {
    fields
        .iter()
        .map(|field| object.get(*field).cloned().ok_or(IndexError::CorruptIndex))
        .collect()
}

fn validate_m5_json_table<T>(
    connection: &rusqlite::Connection,
    limits: IndexLimits,
    sql: &str,
    expected: &[T],
    values: impl Fn(&T) -> Result<Vec<Value>, IndexError>,
) -> Result<(), IndexError> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|_| IndexError::CorruptIndex)?;
    let mut rows = statement.query([]).map_err(|_| IndexError::CorruptIndex)?;
    for expected in expected {
        let Some(row) = rows.next().map_err(|_| IndexError::CorruptIndex)? else {
            return corrupt();
        };
        let values = values(expected)?;
        compare_v5_json_row(row, &values, limits)?;
    }
    if rows.next().map_err(|_| IndexError::CorruptIndex)?.is_some() {
        return corrupt();
    }
    Ok(())
}

fn m5_nested_expected<T: Serialize>(
    row: &T,
    nested_field: &str,
    nested_fields: &[&str],
    owner_fields: &[&str],
) -> Result<Vec<Value>, IndexError> {
    let item = m5_item_object(row)?;
    let nested = item
        .get(nested_field)
        .and_then(Value::as_object)
        .ok_or(IndexError::CorruptIndex)?;
    let mut values = take_m5_fields(&item, &["event_sequence", "event_id"])?;
    values.extend(take_m5_fields(&item, owner_fields)?);
    values.extend(take_m5_fields(nested, nested_fields)?);
    values.extend(take_m5_fields(&item, &["body_hash"])?);
    Ok(values)
}

fn validate_m5_snapshot_closure(snapshot: &IndexSnapshotV5) -> Result<(), IndexError> {
    if snapshot.artifact_registrations_v4.len() != snapshot.gluing_input_descriptors.len()
        || snapshot.artifact_registrations_v4.len() > 2
    {
        return corrupt();
    }
    for (index, (registration, descriptor)) in snapshot
        .artifact_registrations_v4
        .iter()
        .zip(&snapshot.gluing_input_descriptors)
        .enumerate()
    {
        let descriptor_object =
            canonical_object(&descriptor.descriptor).map_err(|_| IndexError::CorruptIndex)?;
        let descriptor_id = id(&descriptor_object, "id").map_err(|_| IndexError::CorruptIndex)?;
        let descriptor_run_id =
            id(&descriptor_object, "run_id").map_err(|_| IndexError::CorruptIndex)?;
        let descriptor_context_id =
            id(&descriptor_object, "context_id").map_err(|_| IndexError::CorruptIndex)?;
        if registration.event_sequence != descriptor.event_sequence
            || registration.event_id != descriptor.event_id
            || registration.registration_id != descriptor.registration_id
            || registration.descriptor_id != descriptor_id
            || registration.cas_hash != descriptor.descriptor_hash
            || registration.size != descriptor.descriptor_size
            || registration.run_id != descriptor_run_id
        {
            return corrupt();
        }
        let event = snapshot
            .events
            .iter()
            .find(|event| event.sequence == registration.event_sequence)
            .ok_or(IndexError::CorruptIndex)?;
        if event.event_id != registration.event_id
            || event.actor != registration.event_actor
            || event.payload_kind != "artifact_registered_v4"
        {
            return corrupt();
        }
        let expected_context = if index == 0 {
            reviewgraphen_core::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID
        } else {
            reviewgraphen_core::DOUBLE_SUBMIT_UI_CONTEXT_ID
        };
        if descriptor_context_id.as_str() != expected_context {
            return corrupt();
        }
        let source = registration
            .source
            .as_object()
            .ok_or(IndexError::CorruptIndex)?;
        if source.get("kind").and_then(Value::as_str) != Some("gluing_input") {
            return corrupt();
        }
        let source_descriptor_id =
            id(source, "descriptor_id").map_err(|_| IndexError::CorruptIndex)?;
        let source_descriptor_hash =
            hash(source, "descriptor_hash").map_err(|_| IndexError::CorruptIndex)?;
        let source_descriptor_size = source
            .get("descriptor_size")
            .and_then(Value::as_u64)
            .ok_or(IndexError::CorruptIndex)?;
        let source_context_id = id(source, "context_id").map_err(|_| IndexError::CorruptIndex)?;
        if source_descriptor_id != registration.descriptor_id
            || source_descriptor_hash != registration.cas_hash
            || source_descriptor_size != registration.size
            || source_context_id != descriptor_context_id
        {
            return corrupt();
        }
    }

    let bundle_present = !snapshot.context_covers.is_empty();
    if !bundle_present {
        if !snapshot.sections.is_empty()
            || !snapshot.restrictions.is_empty()
            || !snapshot.gluing_attempts.is_empty()
            || !snapshot.global_candidates.is_empty()
            || !snapshot.gluing_obstructions.is_empty()
        {
            return corrupt();
        }
        return Ok(());
    }
    if snapshot.context_covers.len() != 1
        || snapshot.sections.is_empty()
        || snapshot.sections.len() > 2
        || snapshot.gluing_attempts.len() != 1
        || snapshot.global_candidates.len() + snapshot.gluing_obstructions.len() != 1
        || snapshot.artifact_registrations_v4.len() != 2
    {
        return corrupt();
    }
    let event_sequence = snapshot.context_covers[0].event_sequence;
    let event_id = &snapshot.context_covers[0].event_id;
    let same_event = snapshot
        .sections
        .iter()
        .all(|row| row.event_sequence == event_sequence && &row.event_id == event_id)
        && snapshot
            .restrictions
            .iter()
            .all(|row| row.event_sequence == event_sequence && &row.event_id == event_id)
        && snapshot
            .gluing_attempts
            .iter()
            .all(|row| row.event_sequence == event_sequence && &row.event_id == event_id)
        && snapshot
            .global_candidates
            .iter()
            .all(|row| row.event_sequence == event_sequence && &row.event_id == event_id)
        && snapshot
            .gluing_obstructions
            .iter()
            .all(|row| row.event_sequence == event_sequence && &row.event_id == event_id);
    if !same_event {
        return corrupt();
    }
    let event = snapshot
        .events
        .iter()
        .find(|event| event.sequence == event_sequence)
        .ok_or(IndexError::CorruptIndex)?;
    if &event.event_id != event_id || event.payload_kind != "gluing_bundle_recorded_v4" {
        return corrupt();
    }
    let contexts = snapshot
        .sections
        .iter()
        .map(|row| {
            let object = canonical_object(&row.section).map_err(|_| IndexError::CorruptIndex)?;
            id(&object, "context_id").map_err(|_| IndexError::CorruptIndex)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let required_contexts = [
        StableId::parse(reviewgraphen_core::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID)
            .map_err(|_| IndexError::CorruptIndex)?,
        StableId::parse(reviewgraphen_core::DOUBLE_SUBMIT_UI_CONTEXT_ID)
            .map_err(|_| IndexError::CorruptIndex)?,
    ];
    if contexts != required_contexts[..contexts.len()] {
        return corrupt();
    }
    let attempt_object = canonical_object(&snapshot.gluing_attempts[0].attempt)
        .map_err(|_| IndexError::CorruptIndex)?;
    let attempt_id = id(&attempt_object, "id").map_err(|_| IndexError::CorruptIndex)?;
    if snapshot
        .restrictions
        .iter()
        .any(|row| row.attempt_id != attempt_id)
        || snapshot
            .global_candidates
            .iter()
            .any(|row| row.attempt_id != attempt_id)
        || snapshot.gluing_obstructions.iter().any(|row| {
            row.obstruction
                .as_object()
                .and_then(|object| object.get("attempt_id"))
                .and_then(Value::as_str)
                != Some(attempt_id.as_str())
        })
    {
        return corrupt();
    }
    Ok(())
}

/// Reads every v5 cell through borrowed SQLite values. Candidate-controlled
/// text is length checked and byte-compared before JSON decoding allocates;
/// every canonical JSON cell is decoded and re-canonicalized, and artifact
/// provenance additionally proves its source-kind and same-run closure.
fn validate_v5_rows(
    connection: &rusqlite::Connection,
    s: &IndexSnapshotV5,
    limits: IndexLimits,
) -> Result<(), IndexError> {
    validate_v5_table!(
        connection,
        limits,
        "SELECT singleton,index_schema_version,projection_contract_version,event_contract_version,projection_mode,run_id,genesis_hash,confirmed_offset,tail_hash,event_count,policy_revision_hash,authority_replay_basis_digest FROM index_meta ORDER BY singleton",
        std::iter::once(&s.marker),
        |r| [
            ExpectedCellV5::U64(1),
            ExpectedCellV5::U64(r.index_schema_version),
            ExpectedCellV5::Text(&r.projection_contract_version),
            ExpectedCellV5::Text(&r.event_contract_version),
            ExpectedCellV5::Text(&r.projection_mode),
            ExpectedCellV5::Id(&r.run_id),
            ExpectedCellV5::Hash(&r.genesis_hash),
            ExpectedCellV5::U64(r.confirmed_offset),
            ExpectedCellV5::Hash(&r.tail_hash),
            ExpectedCellV5::U64(r.event_count),
            ExpectedCellV5::Hash(&r.policy_revision_hash),
            ExpectedCellV5::Hash(&r.authority_replay_basis_digest)
        ]
    );
    validate_v5_table!(
        connection,
        limits,
        "SELECT sequence,event_id,schema,event_hash,payload_hash,payload_kind,actor,logical_time FROM events ORDER BY sequence",
        &s.events,
        |r| [
            ExpectedCellV5::U64(r.sequence),
            ExpectedCellV5::Id(&r.event_id),
            ExpectedCellV5::Text(&r.schema),
            ExpectedCellV5::Hash(&r.event_hash),
            ExpectedCellV5::Hash(&r.payload_hash),
            ExpectedCellV5::Text(&r.payload_kind),
            ExpectedCellV5::Text(&r.actor),
            ExpectedCellV5::U64(r.logical_time)
        ]
    );
    validate_v5_table!(
        connection,
        limits,
        "SELECT object_id,object_kind,body_hash FROM program_objects ORDER BY object_id",
        &s.program_objects,
        |r| [
            ExpectedCellV5::Id(&r.object_id),
            ExpectedCellV5::Text(&r.object_kind),
            ExpectedCellV5::Hash(&r.body_hash)
        ]
    );
    validate_v5_table!(
        connection,
        limits,
        "SELECT relation_id,relation_kind,source_id,target_ids_canonical_json,body_hash FROM program_relations ORDER BY relation_id",
        &s.program_relations,
        |r| [
            ExpectedCellV5::Id(&r.relation_id),
            ExpectedCellV5::Text(&r.relation_kind),
            ExpectedCellV5::Id(&r.source_id),
            ExpectedCellV5::CanonicalJson(&r.target_ids_canonical_json),
            ExpectedCellV5::Hash(&r.body_hash)
        ]
    );
    validate_v5_table!(
        connection,
        limits,
        "SELECT singleton,universe_id,snapshot_id,profile_id,rule_set_hash,extractor_set_hash,policy_version,rule_pack_version,body_hash FROM universe ORDER BY singleton",
        s.universe.iter(),
        |r| [
            ExpectedCellV5::U64(1),
            ExpectedCellV5::Id(&r.universe_id),
            ExpectedCellV5::Id(&r.snapshot_id),
            ExpectedCellV5::Text(&r.profile_id),
            ExpectedCellV5::Hash(&r.rule_set_hash),
            ExpectedCellV5::Hash(&r.extractor_set_hash),
            ExpectedCellV5::Text(&r.policy_version),
            ExpectedCellV5::Text(&r.rule_pack_version),
            ExpectedCellV5::Hash(&r.body_hash)
        ]
    );
    validate_v5_table!(
        connection,
        limits,
        "SELECT obligation_id,target_kind,target_ids_canonical_json,property_id,lifecycle,body_hash FROM obligations ORDER BY obligation_id",
        &s.obligations,
        |r| [
            ExpectedCellV5::Id(&r.obligation_id),
            ExpectedCellV5::Text(&r.target_kind),
            ExpectedCellV5::CanonicalJson(&r.target_ids_canonical_json),
            ExpectedCellV5::Text(&r.property_id),
            ExpectedCellV5::Text(&r.lifecycle),
            ExpectedCellV5::Hash(&r.body_hash)
        ]
    );
    validate_v5_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,obligation_id,next_lifecycle FROM obligation_lifecycle ORDER BY event_sequence,obligation_id",
        &s.obligation_lifecycle,
        |r| [
            ExpectedCellV5::U64(r.event_sequence),
            ExpectedCellV5::Id(&r.event_id),
            ExpectedCellV5::Id(&r.obligation_id),
            ExpectedCellV5::Text(&r.next_lifecycle)
        ]
    );
    validate_v5_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,registration_id,run_id,cas_hash,media_type,size,sensitivity,source_kind,source_canonical_json,body_hash FROM artifact_registrations ORDER BY event_sequence,registration_id",
        &s.artifact_registrations,
        |r| [
            ExpectedCellV5::U64(r.event_sequence),
            ExpectedCellV5::Id(&r.event_id),
            ExpectedCellV5::Id(&r.registration_id),
            ExpectedCellV5::Id(&r.run_id),
            ExpectedCellV5::Hash(&r.cas_hash),
            ExpectedCellV5::Text(&r.media_type),
            ExpectedCellV5::U64(r.size),
            ExpectedCellV5::Text(&r.sensitivity),
            ExpectedCellV5::Text(&r.source_kind),
            ExpectedCellV5::ArtifactSource {
                canonical: &r.source_canonical_json,
                source: &r.source,
                source_kind: &r.source_kind,
                run_id: &r.run_id
            },
            ExpectedCellV5::Hash(&r.body_hash)
        ]
    );
    validate_v5_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,snapshot_id,artifact_id,registration_id,path,content_hash,cas_hash,line_count FROM snapshot_source_index ORDER BY event_sequence,artifact_id",
        &s.snapshot_sources,
        |r| [
            ExpectedCellV5::U64(r.event_sequence),
            ExpectedCellV5::Id(&r.event_id),
            ExpectedCellV5::Id(&r.snapshot_id),
            ExpectedCellV5::Id(&r.artifact_id),
            ExpectedCellV5::Id(&r.registration_id),
            ExpectedCellV5::Text(&r.path),
            ExpectedCellV5::Hash(&r.content_hash),
            ExpectedCellV5::Hash(&r.cas_hash),
            ExpectedCellV5::U64(r.line_count)
        ]
    );
    validate_v5_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,plan_id,universe_id,snapshot_id,planner_input_hash,planner_policy_version,planner_policy_hash,budget_canonical_json,budget_hash,risk_breakdown_canonical_json,waves_canonical_json,deferred_canonical_json,identity_body_hash,body_hash FROM review_plans ORDER BY event_sequence,plan_id",
        &s.review_plans,
        |r| [
            ExpectedCellV5::U64(r.event_sequence),
            ExpectedCellV5::Id(&r.event_id),
            ExpectedCellV5::Id(&r.plan_id),
            ExpectedCellV5::Id(&r.universe_id),
            ExpectedCellV5::Id(&r.snapshot_id),
            ExpectedCellV5::Hash(&r.planner_input_hash),
            ExpectedCellV5::Text(&r.planner_policy_version),
            ExpectedCellV5::Hash(&r.planner_policy_hash),
            ExpectedCellV5::CanonicalJson(&r.budget_canonical_json),
            ExpectedCellV5::Hash(&r.budget_hash),
            ExpectedCellV5::CanonicalJson(&r.risk_breakdown_canonical_json),
            ExpectedCellV5::CanonicalJson(&r.waves_canonical_json),
            ExpectedCellV5::CanonicalJson(&r.deferred_canonical_json),
            ExpectedCellV5::Hash(&r.identity_body_hash),
            ExpectedCellV5::Hash(&r.body_hash)
        ]
    );
    validate_v5_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,envelope_id,snapshot_id,context_policy_version,context_policy_hash,candidate_ids_canonical_json,obligation_ids_canonical_json,context_policy_canonical_json,included_sources_canonical_json,excluded_sources_canonical_json,unknowns_canonical_json,assumptions_canonical_json,losses_canonical_json,projection_hash,body_hash FROM context_envelopes ORDER BY event_sequence,envelope_id",
        &s.context_envelopes,
        |r| [
            ExpectedCellV5::U64(r.event_sequence),
            ExpectedCellV5::Id(&r.event_id),
            ExpectedCellV5::Id(&r.envelope_id),
            ExpectedCellV5::Id(&r.snapshot_id),
            ExpectedCellV5::Text(&r.context_policy_version),
            ExpectedCellV5::Hash(&r.context_policy_hash),
            ExpectedCellV5::CanonicalJson(&r.candidate_ids_canonical_json),
            ExpectedCellV5::CanonicalJson(&r.obligation_ids_canonical_json),
            ExpectedCellV5::ContextPolicy(&r.context_policy_canonical_json),
            ExpectedCellV5::CanonicalJson(&r.included_sources_canonical_json),
            ExpectedCellV5::CanonicalJson(&r.excluded_sources_canonical_json),
            ExpectedCellV5::CanonicalJson(&r.unknowns_canonical_json),
            ExpectedCellV5::CanonicalJson(&r.assumptions_canonical_json),
            ExpectedCellV5::CanonicalJson(&r.losses_canonical_json),
            ExpectedCellV5::Hash(&r.projection_hash),
            ExpectedCellV5::Hash(&r.body_hash)
        ]
    );
    validate_v5_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,execution_id,plan_id,wave_id,snapshot_id,envelope_id,obligation_ids_canonical_json,reviewer_kind,reviewer_id,provider,model,model_revision,system_prompt_version,prompt_template_version,inference_settings_canonical_json,tool_policy_version,tool_calls_canonical_json,attempt,raw_registration_id,raw_hash,parsed_claim_ids_canonical_json,outcome_kind,outcome_canonical_json,identity_body_hash,body_hash FROM executions ORDER BY event_sequence,execution_id",
        &s.executions,
        |r| [
            ExpectedCellV5::U64(r.event_sequence),
            ExpectedCellV5::Id(&r.event_id),
            ExpectedCellV5::Id(&r.execution_id),
            ExpectedCellV5::Id(&r.plan_id),
            ExpectedCellV5::Id(&r.wave_id),
            ExpectedCellV5::Id(&r.snapshot_id),
            ExpectedCellV5::Id(&r.envelope_id),
            ExpectedCellV5::CanonicalJson(&r.obligation_ids_canonical_json),
            ExpectedCellV5::Text(&r.reviewer_kind),
            ExpectedCellV5::Text(&r.reviewer_id),
            ExpectedCellV5::OptionalText(r.provider.as_deref()),
            ExpectedCellV5::OptionalText(r.model.as_deref()),
            ExpectedCellV5::OptionalText(r.model_revision.as_deref()),
            ExpectedCellV5::Text(&r.system_prompt_version),
            ExpectedCellV5::Text(&r.prompt_template_version),
            ExpectedCellV5::CanonicalJson(&r.inference_settings_canonical_json),
            ExpectedCellV5::Text(&r.tool_policy_version),
            ExpectedCellV5::CanonicalJson(&r.tool_calls_canonical_json),
            ExpectedCellV5::U64(u64::from(r.attempt)),
            ExpectedCellV5::Id(&r.raw_registration_id),
            ExpectedCellV5::Hash(&r.raw_hash),
            ExpectedCellV5::CanonicalJson(&r.parsed_claim_ids_canonical_json),
            ExpectedCellV5::Text(&r.outcome_kind),
            ExpectedCellV5::CanonicalJson(&r.outcome_canonical_json),
            ExpectedCellV5::Hash(&r.identity_body_hash),
            ExpectedCellV5::Hash(&r.body_hash)
        ]
    );
    validate_v5_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,claim_id,execution_id,obligation_ids_canonical_json,property_id,target_refs_canonical_json,polarity,disposition,summary,source_ids_canonical_json,assumptions_canonical_json,requested_evidence_canonical_json,candidate_confidence_canonical_json,author_kind,review_status,identity_body_hash,body_hash FROM claims ORDER BY event_sequence,claim_id",
        &s.claims,
        |r| [
            ExpectedCellV5::U64(r.event_sequence),
            ExpectedCellV5::Id(&r.event_id),
            ExpectedCellV5::Id(&r.claim_id),
            ExpectedCellV5::Id(&r.execution_id),
            ExpectedCellV5::CanonicalJson(&r.obligation_ids_canonical_json),
            ExpectedCellV5::Text(&r.property_id),
            ExpectedCellV5::CanonicalJson(&r.target_refs_canonical_json),
            ExpectedCellV5::Text(&r.polarity),
            ExpectedCellV5::Text(&r.disposition),
            ExpectedCellV5::Text(&r.summary),
            ExpectedCellV5::CanonicalJson(&r.source_ids_canonical_json),
            ExpectedCellV5::CanonicalJson(&r.assumptions_canonical_json),
            ExpectedCellV5::CanonicalJson(&r.requested_evidence_canonical_json),
            ExpectedCellV5::CanonicalJson(&r.candidate_confidence_canonical_json),
            ExpectedCellV5::Text(&r.author_kind),
            ExpectedCellV5::Text(&r.review_status),
            ExpectedCellV5::Hash(&r.identity_body_hash),
            ExpectedCellV5::Hash(&r.body_hash)
        ]
    );
    validate_v5_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,record_id,kind,body_hash,authority_reconciled FROM unreconciled_authority_records ORDER BY event_sequence,record_id",
        &s.shadows,
        |r| [
            ExpectedCellV5::U64(r.event_sequence),
            ExpectedCellV5::Id(&r.event_id),
            ExpectedCellV5::Id(&r.record_id),
            ExpectedCellV5::Text(&r.kind),
            ExpectedCellV5::Hash(&r.body_hash),
            ExpectedCellV5::Bool(false)
        ]
    );
    validate_v5_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,finding_id,body_hash,projection_status FROM projected_findings ORDER BY event_sequence,finding_id",
        &s.projected_findings,
        |r| [
            ExpectedCellV5::U64(r.event_sequence),
            ExpectedCellV5::Id(&r.event_id),
            ExpectedCellV5::Id(&r.finding_id),
            ExpectedCellV5::Hash(&r.body_hash),
            ExpectedCellV5::Text(&r.projection_status)
        ]
    );
    validate_v5_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,evidence_id,schema,kind,snapshot_id,subject_ids_canonical_json,descriptor_id,procedure_version,input_registration_id,output_registration_id,observation,body_hash FROM evidence_v3 ORDER BY event_sequence,evidence_id",
        &s.evidence,
        |r| [
            ExpectedCellV5::U64(r.event_sequence),
            ExpectedCellV5::Id(&r.event_id),
            ExpectedCellV5::Id(&r.evidence_id),
            ExpectedCellV5::Text(&r.schema),
            ExpectedCellV5::Text(&r.kind),
            ExpectedCellV5::Id(&r.snapshot_id),
            ExpectedCellV5::CanonicalJson(&r.subject_ids_canonical_json),
            ExpectedCellV5::Text(&r.descriptor_id),
            ExpectedCellV5::Text(&r.procedure_version),
            ExpectedCellV5::Id(&r.input_registration_id),
            ExpectedCellV5::Id(&r.output_registration_id),
            ExpectedCellV5::Text(&r.observation),
            ExpectedCellV5::Hash(&r.body_hash)
        ]
    );
    validate_v5_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,binding_id,schema,claim_id,evidence_id,relation,property_id,body_hash FROM evidence_bindings_v3 ORDER BY event_sequence,binding_id",
        &s.evidence_bindings,
        |r| [
            ExpectedCellV5::U64(r.event_sequence),
            ExpectedCellV5::Id(&r.event_id),
            ExpectedCellV5::Id(&r.binding_id),
            ExpectedCellV5::Text(&r.schema),
            ExpectedCellV5::Id(&r.claim_id),
            ExpectedCellV5::Id(&r.evidence_id),
            ExpectedCellV5::Text(&r.relation),
            ExpectedCellV5::Text(&r.property_id),
            ExpectedCellV5::Hash(&r.body_hash)
        ]
    );
    validate_v5_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,verification_id,schema,claim_id,descriptor_id,procedure_version,input_registration_id,output_registration_id,evidence_ids_canonical_json,outcome,limitations_canonical_json,body_hash FROM verifications_v3 ORDER BY event_sequence,verification_id",
        &s.verifications,
        |r| [
            ExpectedCellV5::U64(r.event_sequence),
            ExpectedCellV5::Id(&r.event_id),
            ExpectedCellV5::Id(&r.verification_id),
            ExpectedCellV5::Text(&r.schema),
            ExpectedCellV5::Id(&r.claim_id),
            ExpectedCellV5::Text(&r.descriptor_id),
            ExpectedCellV5::Text(&r.procedure_version),
            ExpectedCellV5::Id(&r.input_registration_id),
            ExpectedCellV5::Id(&r.output_registration_id),
            ExpectedCellV5::CanonicalJson(&r.evidence_ids_canonical_json),
            ExpectedCellV5::Text(&r.outcome),
            ExpectedCellV5::CanonicalJson(&r.limitations_canonical_json),
            ExpectedCellV5::Hash(&r.body_hash)
        ]
    );
    validate_v5_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,decision_id,schema,policy_revision_hash,run_id,universe_id,claim_id,property_id,outcome,actor,authority_id,snapshot_id,source_ids_canonical_json,rationale,issued_at,expires_at,body_hash FROM decisions_v3 ORDER BY event_sequence,decision_id",
        &s.decisions,
        |r| [
            ExpectedCellV5::U64(r.event_sequence),
            ExpectedCellV5::Id(&r.event_id),
            ExpectedCellV5::Id(&r.decision_id),
            ExpectedCellV5::Text(&r.schema),
            ExpectedCellV5::Hash(&r.policy_revision_hash),
            ExpectedCellV5::Id(&r.run_id),
            ExpectedCellV5::Id(&r.universe_id),
            ExpectedCellV5::Id(&r.claim_id),
            ExpectedCellV5::Text(&r.property_id),
            ExpectedCellV5::Text(&r.outcome),
            ExpectedCellV5::Text(&r.actor),
            ExpectedCellV5::Text(&r.authority_id),
            ExpectedCellV5::Id(&r.snapshot_id),
            ExpectedCellV5::CanonicalJson(&r.source_ids_canonical_json),
            ExpectedCellV5::Text(&r.rationale),
            ExpectedCellV5::Text(&r.issued_at),
            ExpectedCellV5::OptionalText(r.expires_at.as_deref()),
            ExpectedCellV5::Hash(&r.body_hash)
        ]
    );
    validate_v5_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,finding_id,schema,projection_descriptor_id,claim_id,status,evidence_ids_canonical_json,verification_ids_canonical_json,decision_id,supersedes_finding_id,body_hash FROM findings_v3 ORDER BY event_sequence,finding_id",
        &s.findings,
        |r| [
            ExpectedCellV5::U64(r.event_sequence),
            ExpectedCellV5::Id(&r.event_id),
            ExpectedCellV5::Id(&r.finding_id),
            ExpectedCellV5::Text(&r.schema),
            ExpectedCellV5::Text(&r.projection_descriptor_id),
            ExpectedCellV5::Id(&r.claim_id),
            ExpectedCellV5::Text(&r.status),
            ExpectedCellV5::CanonicalJson(&r.evidence_ids_canonical_json),
            ExpectedCellV5::CanonicalJson(&r.verification_ids_canonical_json),
            ExpectedCellV5::OptionalId(r.decision_id.as_ref()),
            ExpectedCellV5::OptionalId(r.supersedes_finding_id.as_ref()),
            ExpectedCellV5::Hash(&r.body_hash)
        ]
    );
    validate_v5_table!(
        connection,
        limits,
        "SELECT claim_id,disposition,review_status,binding_ids_canonical_json,evidence_ids_canonical_json,verification_ids_canonical_json,decision_ids_canonical_json,finding_ids_canonical_json,active_decision_id,current_finding_id,decision_conflict,confirmed_event_sequence FROM claim_assessments_v3 ORDER BY claim_id",
        &s.claim_assessments,
        |r| [
            ExpectedCellV5::Id(&r.claim_id),
            ExpectedCellV5::Text(&r.disposition),
            ExpectedCellV5::Text(&r.review_status),
            ExpectedCellV5::CanonicalJson(&r.binding_ids_canonical_json),
            ExpectedCellV5::CanonicalJson(&r.evidence_ids_canonical_json),
            ExpectedCellV5::CanonicalJson(&r.verification_ids_canonical_json),
            ExpectedCellV5::CanonicalJson(&r.decision_ids_canonical_json),
            ExpectedCellV5::CanonicalJson(&r.finding_ids_canonical_json),
            ExpectedCellV5::OptionalId(r.active_decision_id.as_ref()),
            ExpectedCellV5::OptionalId(r.current_finding_id.as_ref()),
            ExpectedCellV5::Bool(r.decision_conflict),
            ExpectedCellV5::U64(r.confirmed_event_sequence)
        ]
    );
    validate_m5_json_table(
        connection,
        limits,
        "SELECT event_sequence,event_id,registration_id,schema,run_id,cas_hash,media_type,size,sensitivity,source_kind,source_canonical_json,descriptor_id,body_hash FROM artifact_registrations_v4 ORDER BY event_sequence,registration_id",
        &s.artifact_registrations_v4,
        |row| {
            let object = m5_item_object(row)?;
            take_m5_fields(
                &object,
                &[
                    "event_sequence",
                    "event_id",
                    "registration_id",
                    "schema",
                    "run_id",
                    "cas_hash",
                    "media_type",
                    "size",
                    "sensitivity",
                    "source_kind",
                    "source",
                    "descriptor_id",
                    "body_hash",
                ],
            )
        },
    )?;
    validate_m5_json_table(
        connection,
        limits,
        "SELECT event_sequence,event_id,descriptor_id,registration_id,schema,run_id,snapshot_id,universe_id,plan_id,profile_descriptor_id,context_id,assignment_key,assignment_value,qualification_source_ids_canonical_json,descriptor_hash,descriptor_size,body_hash FROM gluing_input_descriptors_v4 ORDER BY event_sequence,descriptor_id",
        &s.gluing_input_descriptors,
        |row| {
            let item = m5_item_object(row)?;
            let descriptor = item
                .get("descriptor")
                .and_then(Value::as_object)
                .ok_or(IndexError::CorruptIndex)?;
            let mut values = take_m5_fields(&item, &["event_sequence", "event_id"])?;
            values.extend(take_m5_fields(descriptor, &["id"])?);
            values.extend(take_m5_fields(&item, &["registration_id"])?);
            values.extend(take_m5_fields(
                descriptor,
                &[
                    "schema",
                    "run_id",
                    "snapshot_id",
                    "universe_id",
                    "plan_id",
                    "profile_descriptor_id",
                    "context_id",
                    "assignment_key",
                    "assignment_value",
                    "qualification_source_ids",
                ],
            )?);
            values.extend(take_m5_fields(
                &item,
                &["descriptor_hash", "descriptor_size", "body_hash"],
            )?);
            Ok(values)
        },
    )?;
    validate_m5_json_table(
        connection,
        limits,
        "SELECT * FROM context_covers_v4 ORDER BY event_sequence,cover_id",
        &s.context_covers,
        |row| {
            m5_nested_expected(
                row,
                "cover",
                &[
                    "id",
                    "schema",
                    "run_id",
                    "snapshot_id",
                    "universe_id",
                    "plan_id",
                    "profile_descriptor_id",
                    "selected_obligation_ids",
                    "required_context_ids",
                    "cover_domain_ids",
                    "covered_domain_ids",
                    "uncovered_domain_ids",
                    "source_ids",
                ],
                &[],
            )
        },
    )?;
    validate_m5_json_table(
        connection,
        limits,
        V5_SECTIONS_CONTEXT_ORDER_SQL,
        &s.sections,
        |row| {
            m5_nested_expected(
                row,
                "section",
                &[
                    "id",
                    "schema",
                    "cover_id",
                    "context_id",
                    "snapshot_id",
                    "property_id",
                    "invariant_id",
                    "obligation_id",
                    "claim_id",
                    "claim_assessment_id",
                    "input_descriptor_id",
                    "input_registration_id",
                    "assignment_key",
                    "assignment_value",
                    "passed_current_verification",
                    "source_ids",
                    "qualification_source_ids",
                    "binding_ids",
                    "evidence_ids",
                    "verification_ids",
                    "decision_ids",
                    "finding_ids",
                ],
                &[],
            )
        },
    )?;
    validate_m5_json_table(
        connection,
        limits,
        V5_RESTRICTIONS_CONTEXT_ORDER_SQL,
        &s.restrictions,
        |row| {
            m5_nested_expected(
                row,
                "restriction",
                &[
                    "id",
                    "schema",
                    "section_id",
                    "context_pair",
                    "overlap_member_ids",
                    "assignment_key",
                    "assignment_value",
                    "source_ids",
                    "qualification_source_ids",
                    "claim_ids",
                    "evidence_ids",
                    "verification_ids",
                    "decision_ids",
                    "finding_ids",
                ],
                &["attempt_id"],
            )
        },
    )?;
    validate_m5_json_table(
        connection,
        limits,
        "SELECT * FROM gluing_attempts_v4 ORDER BY event_sequence,attempt_id",
        &s.gluing_attempts,
        |row| {
            m5_nested_expected(
                row,
                "attempt",
                &[
                    "id",
                    "schema",
                    "cover_id",
                    "snapshot_id",
                    "property_id",
                    "invariant_id",
                    "input_descriptor_ids",
                    "section_ids",
                    "restriction_ids",
                    "result",
                    "global_candidate_id",
                    "obstruction_id",
                    "source_ids",
                    "claim_ids",
                    "evidence_ids",
                    "verification_ids",
                    "decision_ids",
                    "finding_ids",
                ],
                &[],
            )
        },
    )?;
    validate_m5_json_table(
        connection,
        limits,
        "SELECT * FROM global_candidates_v4 ORDER BY event_sequence,global_candidate_id",
        &s.global_candidates,
        |row| {
            m5_nested_expected(
                row,
                "candidate",
                &[
                    "id",
                    "schema",
                    "cover_id",
                    "invariant_id",
                    "property_id",
                    "required_section_ids",
                    "restriction_ids",
                    "qualification_source_ids",
                    "source_ids",
                    "claim_ids",
                    "evidence_ids",
                    "verification_ids",
                    "decision_ids",
                    "finding_ids",
                ],
                &["attempt_id"],
            )
        },
    )?;
    validate_m5_json_table(
        connection,
        limits,
        "SELECT * FROM gluing_obstructions_v4 ORDER BY event_sequence,obstruction_id",
        &s.gluing_obstructions,
        |row| {
            m5_nested_expected(
                row,
                "obstruction",
                &[
                    "attempt_id",
                    "id",
                    "schema",
                    "kind",
                    "conflicting_context_ids",
                    "section_ids",
                    "overlap_member_ids",
                    "assignment_key",
                    "left_assignment_value",
                    "right_assignment_value",
                    "source_ids",
                    "claim_ids",
                    "evidence_ids",
                    "verification_ids",
                    "decision_ids",
                    "finding_ids",
                    "affected_invariant_id",
                    "severity",
                    "required_resolution",
                    "human_decision_required",
                    "blocks",
                ],
                &[],
            )
        },
    )?;

    validate_m5_snapshot_closure(s)?;
    Ok(())
}

const V5_SCAN_TABLES: [(&str, &[usize]); 29] = [
    ("SELECT * FROM index_meta", &[]),
    ("SELECT * FROM events", &[]),
    ("SELECT * FROM program_objects", &[]),
    ("SELECT * FROM program_relations", &[3]),
    ("SELECT * FROM universe", &[]),
    ("SELECT * FROM obligations", &[2]),
    ("SELECT * FROM obligation_lifecycle", &[]),
    ("SELECT * FROM executions", &[7, 15, 17, 21, 23]),
    ("SELECT * FROM claims", &[4, 6, 10, 11, 12, 13]),
    ("SELECT * FROM artifact_registrations", &[]),
    ("SELECT * FROM snapshot_source_index", &[]),
    ("SELECT * FROM review_plans", &[8, 10, 11, 12]),
    (
        "SELECT * FROM context_envelopes",
        &[6, 7, 9, 10, 11, 12, 13],
    ),
    ("SELECT * FROM unreconciled_authority_records", &[]),
    ("SELECT * FROM projected_findings", &[]),
    ("SELECT * FROM evidence_v3", &[6]),
    ("SELECT * FROM evidence_bindings_v3", &[]),
    ("SELECT * FROM verifications_v3", &[9, 11]),
    ("SELECT * FROM decisions_v3", &[13]),
    ("SELECT * FROM findings_v3", &[7, 8]),
    ("SELECT * FROM claim_assessments_v3", &[3, 4, 5, 6, 7]),
    ("SELECT * FROM artifact_registrations_v4", &[10]),
    ("SELECT * FROM gluing_input_descriptors_v4", &[13]),
    ("SELECT * FROM context_covers_v4", &[9, 10, 11, 12, 13, 14]),
    ("SELECT * FROM sections_v4", &[17, 18, 19, 20, 21, 22, 23]),
    (
        "SELECT * FROM restrictions_v4",
        &[6, 7, 10, 11, 12, 13, 14, 15, 16],
    ),
    (
        "SELECT * FROM gluing_attempts_v4",
        &[8, 9, 10, 14, 15, 16, 17, 18, 19],
    ),
    (
        "SELECT * FROM global_candidates_v4",
        &[8, 9, 10, 11, 12, 13, 14, 15, 16],
    ),
    (
        "SELECT * FROM gluing_obstructions_v4",
        &[6, 7, 8, 12, 13, 14, 15, 16, 17, 22],
    ),
];

fn preflight_json_cell(bytes: &[u8]) -> Result<u64, IndexError> {
    let mut scanner = JsonOwnershipScanner { bytes, cursor: 0 };
    scanner.whitespace();
    let scanned = scanner.value(0)?;
    scanner.whitespace();
    if scanner.cursor != bytes.len() {
        return Err(IndexError::CorruptIndex);
    }
    let input = u64::try_from(bytes.len()).map_err(|_| IndexError::IntegerOutOfRange)?;
    input
        .checked_add(scanned.canonical_bytes)
        .and_then(|value| value.checked_add(scanned.owned_bytes))
        .ok_or(IndexError::IntegerOutOfRange)
}

#[cfg(test)]
fn preflight_json_cell_with_limit(bytes: &[u8], limit: u64) -> Result<u64, IndexError> {
    let observed = preflight_json_cell(bytes)?;
    if observed > limit {
        return Err(IndexError::Incomplete { limit, observed });
    }
    Ok(observed)
}

#[derive(Clone, Copy)]
struct JsonCellScan {
    owned_bytes: u64,
    canonical_bytes: u64,
}

struct JsonOwnershipScanner<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl JsonOwnershipScanner<'_> {
    fn whitespace(&mut self) {
        while matches!(
            self.bytes.get(self.cursor),
            Some(b' ' | b'\n' | b'\r' | b'\t')
        ) {
            self.cursor += 1;
        }
    }

    fn value(&mut self, depth: usize) -> Result<JsonCellScan, IndexError> {
        if depth > 128 {
            return Err(IndexError::CorruptIndex);
        }
        self.whitespace();
        match self.bytes.get(self.cursor).copied() {
            Some(b'n') => self.literal(b"null", 0),
            Some(b't') => self.literal(b"true", 1),
            Some(b'f') => self.literal(b"false", 1),
            Some(b'"') => self.string(),
            Some(b'[') => self.array(depth + 1),
            Some(b'{') => self.object(depth + 1),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(IndexError::CorruptIndex),
        }
    }

    fn literal(&mut self, literal: &[u8], owned: u64) -> Result<JsonCellScan, IndexError> {
        if self.bytes.get(self.cursor..self.cursor + literal.len()) != Some(literal) {
            return Err(IndexError::CorruptIndex);
        }
        self.cursor += literal.len();
        Ok(JsonCellScan {
            owned_bytes: owned,
            canonical_bytes: u64::try_from(literal.len())
                .map_err(|_| IndexError::IntegerOutOfRange)?,
        })
    }

    fn array(&mut self, depth: usize) -> Result<JsonCellScan, IndexError> {
        self.cursor += 1;
        self.whitespace();
        if self.bytes.get(self.cursor) == Some(&b']') {
            self.cursor += 1;
            return Ok(JsonCellScan {
                owned_bytes: 0,
                canonical_bytes: 2,
            });
        }
        let mut owned = 0_u64;
        let mut canonical = 2_u64;
        let mut items = 0_u64;
        loop {
            let item = self.value(depth)?;
            owned = owned
                .checked_add(8)
                .and_then(|value| value.checked_add(item.owned_bytes))
                .ok_or(IndexError::IntegerOutOfRange)?;
            canonical = canonical
                .checked_add(item.canonical_bytes)
                .ok_or(IndexError::IntegerOutOfRange)?;
            items = items.checked_add(1).ok_or(IndexError::IntegerOutOfRange)?;
            self.whitespace();
            match self.bytes.get(self.cursor) {
                Some(b',') => self.cursor += 1,
                Some(b']') => {
                    self.cursor += 1;
                    canonical = canonical
                        .checked_add(items.saturating_sub(1))
                        .ok_or(IndexError::IntegerOutOfRange)?;
                    return Ok(JsonCellScan {
                        owned_bytes: owned,
                        canonical_bytes: canonical,
                    });
                }
                _ => return Err(IndexError::CorruptIndex),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<JsonCellScan, IndexError> {
        self.cursor += 1;
        self.whitespace();
        if self.bytes.get(self.cursor) == Some(&b'}') {
            self.cursor += 1;
            return Ok(JsonCellScan {
                owned_bytes: 0,
                canonical_bytes: 2,
            });
        }
        let mut owned = 0_u64;
        let mut canonical = 2_u64;
        let mut entries = 0_u64;
        loop {
            self.whitespace();
            if self.bytes.get(self.cursor) != Some(&b'"') {
                return Err(IndexError::CorruptIndex);
            }
            let key = self.string()?;
            self.whitespace();
            if self.bytes.get(self.cursor) != Some(&b':') {
                return Err(IndexError::CorruptIndex);
            }
            self.cursor += 1;
            let value = self.value(depth)?;
            owned = owned
                .checked_add(16)
                .and_then(|current| current.checked_add(key.owned_bytes))
                .and_then(|current| current.checked_add(value.owned_bytes))
                .ok_or(IndexError::IntegerOutOfRange)?;
            canonical = canonical
                .checked_add(key.canonical_bytes)
                .and_then(|current| current.checked_add(1))
                .and_then(|current| current.checked_add(value.canonical_bytes))
                .ok_or(IndexError::IntegerOutOfRange)?;
            entries = entries
                .checked_add(1)
                .ok_or(IndexError::IntegerOutOfRange)?;
            self.whitespace();
            match self.bytes.get(self.cursor) {
                Some(b',') => self.cursor += 1,
                Some(b'}') => {
                    self.cursor += 1;
                    canonical = canonical
                        .checked_add(entries.saturating_sub(1))
                        .ok_or(IndexError::IntegerOutOfRange)?;
                    return Ok(JsonCellScan {
                        owned_bytes: owned,
                        canonical_bytes: canonical,
                    });
                }
                _ => return Err(IndexError::CorruptIndex),
            }
        }
    }

    fn string(&mut self) -> Result<JsonCellScan, IndexError> {
        self.cursor += 1;
        let mut decoded = 0_u64;
        let mut canonical = 2_u64;
        loop {
            let byte = *self
                .bytes
                .get(self.cursor)
                .ok_or(IndexError::CorruptIndex)?;
            match byte {
                b'"' => {
                    self.cursor += 1;
                    return Ok(JsonCellScan {
                        owned_bytes: decoded,
                        canonical_bytes: canonical,
                    });
                }
                b'\\' => {
                    self.cursor += 1;
                    match self.bytes.get(self.cursor).copied() {
                        Some(escape @ (b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't')) => {
                            self.cursor += 1;
                            decoded = decoded
                                .checked_add(1)
                                .ok_or(IndexError::IntegerOutOfRange)?;
                            canonical = canonical
                                .checked_add(if escape == b'/' { 1 } else { 2 })
                                .ok_or(IndexError::IntegerOutOfRange)?;
                        }
                        Some(b'u') => {
                            self.cursor += 1;
                            let first = self.hex_quad()?;
                            let scalar = if (0xD800..=0xDBFF).contains(&first) {
                                if self.bytes.get(self.cursor..self.cursor + 2) != Some(b"\\u") {
                                    return Err(IndexError::CorruptIndex);
                                }
                                self.cursor += 2;
                                let second = self.hex_quad()?;
                                if !(0xDC00..=0xDFFF).contains(&second) {
                                    return Err(IndexError::CorruptIndex);
                                }
                                0x1_0000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                            } else if (0xDC00..=0xDFFF).contains(&first) {
                                return Err(IndexError::CorruptIndex);
                            } else {
                                first
                            };
                            let character =
                                char::from_u32(scalar).ok_or(IndexError::CorruptIndex)?;
                            decoded = decoded
                                .checked_add(
                                    u64::try_from(character.len_utf8())
                                        .map_err(|_| IndexError::IntegerOutOfRange)?,
                                )
                                .ok_or(IndexError::IntegerOutOfRange)?;
                            canonical = canonical
                                .checked_add(canonical_json_char_len(character)?)
                                .ok_or(IndexError::IntegerOutOfRange)?;
                        }
                        _ => return Err(IndexError::CorruptIndex),
                    }
                }
                0x00..=0x1f => return Err(IndexError::CorruptIndex),
                _ => {
                    let tail = std::str::from_utf8(&self.bytes[self.cursor..])
                        .map_err(|_| IndexError::CorruptIndex)?;
                    let character = tail.chars().next().ok_or(IndexError::CorruptIndex)?;
                    let length = character.len_utf8();
                    self.cursor += length;
                    decoded = decoded
                        .checked_add(
                            u64::try_from(length).map_err(|_| IndexError::IntegerOutOfRange)?,
                        )
                        .ok_or(IndexError::IntegerOutOfRange)?;
                    canonical = canonical
                        .checked_add(
                            u64::try_from(length).map_err(|_| IndexError::IntegerOutOfRange)?,
                        )
                        .ok_or(IndexError::IntegerOutOfRange)?;
                }
            }
        }
    }

    fn hex_quad(&mut self) -> Result<u32, IndexError> {
        let bytes = self
            .bytes
            .get(self.cursor..self.cursor + 4)
            .ok_or(IndexError::CorruptIndex)?;
        let mut value = 0_u32;
        for byte in bytes {
            let digit = match byte {
                b'0'..=b'9' => u32::from(byte - b'0'),
                b'a'..=b'f' => u32::from(byte - b'a' + 10),
                b'A'..=b'F' => u32::from(byte - b'A' + 10),
                _ => return Err(IndexError::CorruptIndex),
            };
            value = value * 16 + digit;
        }
        self.cursor += 4;
        Ok(value)
    }

    fn number(&mut self) -> Result<JsonCellScan, IndexError> {
        let number_start = self.cursor;
        if self.bytes.get(self.cursor) == Some(&b'-') {
            self.cursor += 1;
        }
        match self.bytes.get(self.cursor).copied() {
            Some(b'0') => self.cursor += 1,
            Some(b'1'..=b'9') => {
                self.cursor += 1;
                while matches!(self.bytes.get(self.cursor), Some(b'0'..=b'9')) {
                    self.cursor += 1;
                }
            }
            _ => return Err(IndexError::CorruptIndex),
        }
        if self.bytes.get(self.cursor) == Some(&b'.') {
            self.cursor += 1;
            let start = self.cursor;
            while matches!(self.bytes.get(self.cursor), Some(b'0'..=b'9')) {
                self.cursor += 1;
            }
            if self.cursor == start {
                return Err(IndexError::CorruptIndex);
            }
        }
        if matches!(self.bytes.get(self.cursor), Some(b'e' | b'E')) {
            self.cursor += 1;
            if matches!(self.bytes.get(self.cursor), Some(b'+' | b'-')) {
                self.cursor += 1;
            }
            let start = self.cursor;
            while matches!(self.bytes.get(self.cursor), Some(b'0'..=b'9')) {
                self.cursor += 1;
            }
            if self.cursor == start {
                return Err(IndexError::CorruptIndex);
            }
        }
        let number: serde_json::Number =
            serde_json::from_slice(&self.bytes[number_start..self.cursor])
                .map_err(|_| IndexError::CorruptIndex)?;
        Ok(JsonCellScan {
            owned_bytes: 8,
            canonical_bytes: json_length(&number)?,
        })
    }
}

fn canonical_json_char_len(character: char) -> Result<u64, IndexError> {
    let bytes = match character {
        '"' | '\\' | '\u{0008}' | '\t' | '\n' | '\u{000c}' | '\r' => 2,
        '\u{0000}'..='\u{001f}' => 6,
        _ => character.len_utf8(),
    };
    u64::try_from(bytes).map_err(|_| IndexError::IntegerOutOfRange)
}

/// Structural readback used before classifying an otherwise well-formed image
/// as stale. It scans every cell without owning candidate text, admits the
/// complete text/row budget, then decodes every canonical JSON value. Thus a
/// stale marker cannot hide malformed or allocation-hostile table contents.
fn preflight_v5_structure(
    connection: &rusqlite::Connection,
    limits: IndexLimits,
) -> Result<u64, IndexError> {
    let mut rows_seen = 0_u64;
    let mut text_seen = 0_u64;
    let mut integer_cells = 0_u64;
    let mut max_json_staging = 0_u64;
    for (sql, canonical_columns) in V5_SCAN_TABLES {
        let mut statement = connection
            .prepare(sql)
            .map_err(|_| IndexError::CorruptIndex)?;
        let column_count = statement.column_count();
        let mut rows = statement.query([]).map_err(|_| IndexError::CorruptIndex)?;
        while let Some(row) = rows.next().map_err(|_| IndexError::CorruptIndex)? {
            rows_seen = rows_seen
                .checked_add(1)
                .ok_or(IndexError::IntegerOutOfRange)?;
            if rows_seen > limits.max_rows {
                return Err(IndexError::Incomplete {
                    limit: limits.max_rows,
                    observed: rows_seen,
                });
            }
            for index in 0..column_count {
                match row.get_ref(index).map_err(|_| IndexError::CorruptIndex)? {
                    ValueRef::Null => {}
                    ValueRef::Integer(_) => {
                        integer_cells = integer_cells
                            .checked_add(1)
                            .ok_or(IndexError::IntegerOutOfRange)?;
                    }
                    ValueRef::Text(bytes) => {
                        std::str::from_utf8(bytes).map_err(|_| IndexError::CorruptIndex)?;
                        text_seen = text_seen
                            .checked_add(
                                u64::try_from(bytes.len())
                                    .map_err(|_| IndexError::IntegerOutOfRange)?,
                            )
                            .ok_or(IndexError::IntegerOutOfRange)?;
                        let typed_source_json =
                            sql == "SELECT * FROM artifact_registrations" && index == 9;
                        if canonical_columns.contains(&index) || typed_source_json {
                            max_json_staging = max_json_staging.max(preflight_json_cell(bytes)?);
                        }
                    }
                    ValueRef::Real(_) | ValueRef::Blob(_) => return corrupt(),
                }
            }
        }
    }
    let sql_bytes = text_seen
        .checked_add(
            integer_cells
                .checked_mul(8)
                .ok_or(IndexError::IntegerOutOfRange)?,
        )
        .and_then(|value| value.checked_add(rows_seen))
        .ok_or(IndexError::IntegerOutOfRange)?;
    if sql_bytes > limits.max_working_bytes {
        return Err(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: sql_bytes,
        });
    }
    Ok(max_json_staging)
}

#[cfg(test)]
pub(crate) fn v5_json_staging_for_connection_for_test(
    connection: &rusqlite::Connection,
    limits: IndexLimits,
) -> Result<u64, IndexError> {
    preflight_v5_structure(connection, limits)
}

fn validate_v5_structure_after_preflight(
    connection: &rusqlite::Connection,
    limits: IndexLimits,
) -> Result<(), IndexError> {
    for (sql, canonical_columns) in V5_SCAN_TABLES {
        if canonical_columns.is_empty() {
            continue;
        }
        let mut statement = connection
            .prepare(sql)
            .map_err(|_| IndexError::CorruptIndex)?;
        let mut rows = statement.query([]).map_err(|_| IndexError::CorruptIndex)?;
        while let Some(row) = rows.next().map_err(|_| IndexError::CorruptIndex)? {
            for &index in canonical_columns {
                let ValueRef::Text(bytes) =
                    row.get_ref(index).map_err(|_| IndexError::CorruptIndex)?
                else {
                    return corrupt();
                };
                record_v5_json_decode_allocation();
                let value: Value =
                    serde_json::from_slice(bytes).map_err(|_| IndexError::CorruptIndex)?;
                if canonical_json(&value).map_err(|_| IndexError::CorruptIndex)? != bytes {
                    return corrupt();
                }
            }
        }
    }
    let mut statement = connection
        .prepare("SELECT run_id,source_kind,source_canonical_json FROM artifact_registrations")
        .map_err(|_| IndexError::CorruptIndex)?;
    let mut rows = statement.query([]).map_err(|_| IndexError::CorruptIndex)?;
    while let Some(row) = rows.next().map_err(|_| IndexError::CorruptIndex)? {
        let ValueRef::Text(run_id) = row.get_ref(0).map_err(|_| IndexError::CorruptIndex)? else {
            return corrupt();
        };
        let ValueRef::Text(source_kind) = row.get_ref(1).map_err(|_| IndexError::CorruptIndex)?
        else {
            return corrupt();
        };
        let ValueRef::Text(source) = row.get_ref(2).map_err(|_| IndexError::CorruptIndex)? else {
            return corrupt();
        };
        let run_id = StableId::parse(
            std::str::from_utf8(run_id)
                .map_err(|_| IndexError::CorruptIndex)?
                .to_owned(),
        )
        .map_err(|_| IndexError::CorruptIndex)?;
        let source_kind = std::str::from_utf8(source_kind).map_err(|_| IndexError::CorruptIndex)?;
        record_v5_json_decode_allocation();
        let decoded: ArtifactSourceV3 =
            serde_json::from_slice(source).map_err(|_| IndexError::CorruptIndex)?;
        if canonical_json(&decoded).map_err(|_| IndexError::CorruptIndex)? != source {
            return corrupt();
        }
        let (decoded_kind, decoded_run_id) = source_kind_and_run_id(&decoded);
        if decoded_kind != source_kind || decoded_run_id != &run_id {
            return corrupt();
        }
    }
    let mut statement = connection
        .prepare(
            "SELECT run_id,source_kind,source_canonical_json,descriptor_id,cas_hash,size \
             FROM artifact_registrations_v4 ORDER BY event_sequence,registration_id",
        )
        .map_err(|_| IndexError::CorruptIndex)?;
    let mut rows = statement.query([]).map_err(|_| IndexError::CorruptIndex)?;
    while let Some(row) = rows.next().map_err(|_| IndexError::CorruptIndex)? {
        let run_id: String = row.get(0).map_err(|_| IndexError::CorruptIndex)?;
        let source_kind: String = row.get(1).map_err(|_| IndexError::CorruptIndex)?;
        let source: String = row.get(2).map_err(|_| IndexError::CorruptIndex)?;
        let descriptor_id: String = row.get(3).map_err(|_| IndexError::CorruptIndex)?;
        let cas_hash: String = row.get(4).map_err(|_| IndexError::CorruptIndex)?;
        let size: i64 = row.get(5).map_err(|_| IndexError::CorruptIndex)?;
        let run_id = StableId::parse(run_id).map_err(|_| IndexError::CorruptIndex)?;
        let descriptor_id = StableId::parse(descriptor_id).map_err(|_| IndexError::CorruptIndex)?;
        let cas_hash = ContentHash::parse(cas_hash).map_err(|_| IndexError::CorruptIndex)?;
        let size = u64::try_from(size).map_err(|_| IndexError::CorruptIndex)?;
        record_v5_json_decode_allocation();
        let decoded: ArtifactSourceV4 =
            serde_json::from_str(&source).map_err(|_| IndexError::CorruptIndex)?;
        if canonical_json(&decoded).map_err(|_| IndexError::CorruptIndex)? != source.as_bytes() {
            return corrupt();
        }
        let (decoded_kind, decoded_run_id) = source_v4_kind_and_run_id(&decoded);
        if decoded_kind != source_kind || decoded_run_id != &run_id {
            return corrupt();
        }
        let ArtifactSourceV4::GluingInput {
            descriptor_id: source_descriptor_id,
            descriptor_hash,
            descriptor_size,
            ..
        } = decoded
        else {
            return corrupt();
        };
        if source_descriptor_id != descriptor_id
            || descriptor_hash != cas_hash
            || descriptor_size != size
        {
            return corrupt();
        }
    }
    let baseline = reviewgraphen_core::ContextPolicyV1::baseline()
        .canonical_bytes()
        .map_err(|_| IndexError::CorruptIndex)?;
    let baseline_hash = ContentHash::sha256(&baseline).to_string();
    let mut statement = connection
        .prepare(
            "SELECT context_policy_version,context_policy_hash,context_policy_canonical_json FROM context_envelopes",
        )
        .map_err(|_| IndexError::CorruptIndex)?;
    let mut rows = statement.query([]).map_err(|_| IndexError::CorruptIndex)?;
    while let Some(row) = rows.next().map_err(|_| IndexError::CorruptIndex)? {
        compare_text(
            row.get_ref(0).map_err(|_| IndexError::CorruptIndex)?,
            reviewgraphen_core::ContextPolicyV1::VERSION.as_bytes(),
            limits,
        )?;
        compare_text(
            row.get_ref(1).map_err(|_| IndexError::CorruptIndex)?,
            baseline_hash.as_bytes(),
            limits,
        )?;
        compare_text(
            row.get_ref(2).map_err(|_| IndexError::CorruptIndex)?,
            &baseline,
            limits,
        )?;
    }
    Ok(())
}

fn validate_v5_structure(
    connection: &rusqlite::Connection,
    limits: IndexLimits,
) -> Result<(), IndexError> {
    let max_json_staging = preflight_v5_structure(connection, limits)?;
    admit_v5_json_staging(max_json_staging)?;
    validate_v5_structure_after_preflight(connection, limits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn candidate_json_staging_scanner_is_exact_without_decoding_allocation() {
        for bytes in [
            br#"null"#.as_slice(),
            br#"[true,12,"x",null]"#.as_slice(),
            br#"{"a":[1,2],"emoji":"\ud83d\ude00"}"#.as_slice(),
            br#"{"kind":"run_genesis","run_id":"run:test"}"#.as_slice(),
        ] {
            let decoded: Value = serde_json::from_slice(bytes).unwrap();
            let expected = u64::try_from(bytes.len()).unwrap()
                + u64::try_from(canonical_json(&decoded).unwrap().len()).unwrap()
                + oracle_recursive_ownership(&decoded).unwrap();
            assert_eq!(preflight_json_cell(bytes).unwrap(), expected);
        }
        let exponent = format!(
            "[{}]",
            std::iter::repeat_n("1e20", 512)
                .collect::<Vec<_>>()
                .join(",")
        );
        let decoded: Value = serde_json::from_str(&exponent).unwrap();
        let expected = u64::try_from(exponent.len()).unwrap()
            + u64::try_from(canonical_json(&decoded).unwrap().len()).unwrap()
            + oracle_recursive_ownership(&decoded).unwrap();
        assert_eq!(
            preflight_json_cell_with_limit(exponent.as_bytes(), expected).unwrap(),
            expected
        );
        assert!(matches!(
            preflight_json_cell_with_limit(exponent.as_bytes(), expected - 1),
            Err(IndexError::Incomplete { limit, observed })
                if limit + 1 == observed && observed == expected
        ));
        assert!(matches!(
            preflight_json_cell(br#"{"a":[1,]}"#),
            Err(IndexError::CorruptIndex)
        ));
    }

    #[test]
    fn borrowed_canonical_json_text_metrics_match_the_materialized_canonical_string() {
        #[derive(Serialize)]
        struct ValueWithEscapes<'a> {
            quote: &'a str,
            nested: [&'a str; 2],
        }

        let value = ValueWithEscapes {
            quote: "a\\\"b",
            nested: ["line\nfeed", "plain"],
        };
        let bytes = canonical_json(&value).unwrap();
        let encoded = serde_json::to_vec(&String::from_utf8(bytes.clone()).unwrap()).unwrap();
        let (raw, escaped, ownership) = borrowed_canonical_json_text_metrics(&value).unwrap();
        assert_eq!(raw, bytes.len() as u64);
        assert_eq!(escaped, encoded.len() as u64);
        assert_eq!(ownership, recursive_ownership_charge(&value).unwrap());
    }

    #[test]
    fn selection_marker_fingerprint_detects_tail_drift() {
        let mut first = empty_snapshot("selection-fingerprint").marker;
        let original = v5_marker_fingerprint(&first).unwrap();
        first.tail_hash = ContentHash::sha256(b"different confirmed tail");
        assert_ne!(v5_marker_fingerprint(&first).unwrap(), original);
    }

    #[test]
    fn normative_accounting_overflow_seams_are_typed_incomplete() {
        let marker = empty_snapshot("overflow").marker;
        let limits = IndexLimits::try_from(crate::StoreLimits {
            max_index_rows: u64::MAX,
            max_index_query_bytes: u64::MAX,
            max_index_working_bytes: u64::MAX,
            ..crate::StoreLimits::default()
        })
        .unwrap();
        let assert_overflow = |charge: ProjectionChargeV5| {
            reset_v5_full_snapshot_construction_count_for_test();
            let rows = charge.rows;
            assert!(matches!(
                charge.finish(&marker, rows, limits),
                Err(IndexError::Incomplete {
                    observed: u64::MAX,
                    ..
                })
            ));
            assert_eq!(v5_post_phase0_counts_for_test(), (0, 0, 0, 0, 0));
        };

        let mut sql = ProjectionChargeV5::new(&marker, limits).unwrap();
        sql.integer_cells = u64::MAX;
        assert_overflow(sql);

        let mut query = ProjectionChargeV5::new(&marker, limits).unwrap();
        query.arrays[EVENTS].json_items = u64::MAX;
        assert_overflow(query);

        let mut owned = ProjectionChargeV5::new(&marker, limits).unwrap();
        owned.marker_owned = u64::MAX;
        assert_overflow(owned);

        let mut working = ProjectionChargeV5::new(&marker, limits).unwrap();
        working.max_cas_bytes = u64::MAX;
        assert_overflow(working);
    }

    fn empty_snapshot(tag: &str) -> IndexSnapshotV5 {
        let policy = ContentHash::sha256(format!("policy:{tag}").as_bytes());
        let basis = ContentHash::sha256(format!("basis:{tag}").as_bytes());
        IndexSnapshotV5 {
            marker: IndexMarkerV5 {
                index_schema_version: 5,
                sqlite_user_version: 5,
                projection_contract_version: PROJECTION_CONTRACT_VERSION_V5.to_owned(),
                event_contract_version: EVENT_CONTRACT_VERSION_V4.to_owned(),
                projection_mode: PROJECTION_MODE_V4.to_owned(),
                run_id: StableId::parse(format!("run:v5-{tag}")).unwrap(),
                genesis_hash: ContentHash::sha256(format!("genesis:{tag}").as_bytes()),
                confirmed_offset: 0,
                tail_hash: ContentHash::sha256(format!("tail:{tag}").as_bytes()),
                event_count: 0,
                policy_revision_hash: policy.clone(),
                authority_replay_basis_digest: basis.clone(),
            },
            events: Vec::new(),
            shadows: Vec::new(),
            projected_findings: Vec::new(),
            program_objects: Vec::new(),
            program_relations: Vec::new(),
            universe: None,
            obligations: Vec::new(),
            obligation_lifecycle: Vec::new(),
            executions: Vec::new(),
            claims: Vec::new(),
            artifact_registrations: Vec::new(),
            snapshot_sources: Vec::new(),
            context_envelopes: Vec::new(),
            review_plans: Vec::new(),
            evidence: Vec::new(),
            evidence_bindings: Vec::new(),
            verifications: Vec::new(),
            decisions: Vec::new(),
            findings: Vec::new(),
            claim_assessments: Vec::new(),
            artifact_registrations_v4: Vec::new(),
            gluing_input_descriptors: Vec::new(),
            context_covers: Vec::new(),
            sections: Vec::new(),
            restrictions: Vec::new(),
            gluing_attempts: Vec::new(),
            global_candidates: Vec::new(),
            gluing_obstructions: Vec::new(),
            policy_revision_hash: policy,
            authority_replay_basis_digest: basis,
        }
    }

    #[test]
    fn schema_v5_literal_is_complete_strict_and_versioned() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection.execute_batch(SCHEMA_V5).unwrap();
        connection
            .pragma_update(None, "user_version", INDEX_SCHEMA_VERSION_V5)
            .unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 5);
        let mut statement = connection
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        let tables = statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<BTreeSet<_>>>()
            .unwrap();
        assert_eq!(
            tables,
            BTreeSet::from([
                "artifact_registrations".to_owned(),
                "artifact_registrations_v4".to_owned(),
                "claim_assessments_v3".to_owned(),
                "claims".to_owned(),
                "context_envelopes".to_owned(),
                "context_covers_v4".to_owned(),
                "decisions_v3".to_owned(),
                "events".to_owned(),
                "evidence_bindings_v3".to_owned(),
                "evidence_v3".to_owned(),
                "executions".to_owned(),
                "findings_v3".to_owned(),
                "global_candidates_v4".to_owned(),
                "gluing_attempts_v4".to_owned(),
                "gluing_input_descriptors_v4".to_owned(),
                "gluing_obstructions_v4".to_owned(),
                "index_meta".to_owned(),
                "obligation_lifecycle".to_owned(),
                "obligations".to_owned(),
                "program_objects".to_owned(),
                "program_relations".to_owned(),
                "projected_findings".to_owned(),
                "review_plans".to_owned(),
                "restrictions_v4".to_owned(),
                "sections_v4".to_owned(),
                "snapshot_source_index".to_owned(),
                "universe".to_owned(),
                "unreconciled_authority_records".to_owned(),
                "verifications_v3".to_owned(),
            ])
        );
        let registration_sql: String = connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name='artifact_registrations'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(registration_sql.contains("source_canonical_json TEXT NOT NULL"));
        assert!(!registration_sql.contains("source_id TEXT"));
        for variant in [
            "run_genesis",
            "snapshot_ingest",
            "reviewer_execution",
            "verifier_artifact",
            "external_harness_witness",
        ] {
            assert!(registration_sql.contains(variant));
        }
        assert!(SCHEMA_V5.contains("reviewgraphen.index_projection.v5"));
        assert!(SCHEMA_V5.contains("reviewgraphen.review_event.v4"));
        assert!(SCHEMA_V5.contains("v4_gluing"));
    }

    #[test]
    fn m5_readback_uses_context_order_not_record_id_order() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection.execute_batch(SCHEMA_V5).unwrap();
        connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
        // Deliberately reverse lexical record IDs.  The two M5 arrays are
        // positional payment/UI arrays, so readback must retain context order.
        connection
            .execute_batch(
                "INSERT INTO sections_v4 VALUES
                 (9,'event:bundle','section:z-payment','reviewgraphen.section.v4','cover:test','context:payment','snapshot:test','payment.at_most_once','invariant:payment-at-most-once','obligation:payment','claim:payment','claim:payment','descriptor:payment','registration:payment','caller_duplicate_protection','satisfied',1,'[]','[]','[]','[]','[]','[]','[]','hash:payment'),
                 (9,'event:bundle','section:a-ui','reviewgraphen.section.v4','cover:test','context:ui-event','snapshot:test','payment.at_most_once','invariant:payment-at-most-once','obligation:ui','claim:ui','claim:ui','descriptor:ui','registration:ui','caller_duplicate_protection','required',1,'[]','[]','[]','[]','[]','[]','[]','hash:ui');
                 INSERT INTO restrictions_v4 VALUES
                 (9,'event:bundle','attempt:test','restriction:z-payment','reviewgraphen.restriction.v4','section:z-payment','[]','[]','caller_duplicate_protection','satisfied','[]','[]','[]','[]','[]','[]','[]','hash:payment'),
                 (9,'event:bundle','attempt:test','restriction:a-ui','reviewgraphen.restriction.v4','section:a-ui','[]','[]','caller_duplicate_protection','required','[]','[]','[]','[]','[]','[]','[]','hash:ui');",
            )
            .unwrap();

        let section_ids = connection
            .prepare(V5_SECTIONS_CONTEXT_ORDER_SQL)
            .unwrap()
            .query_map([], |row| row.get::<_, String>(2))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(section_ids, ["section:z-payment", "section:a-ui"]);

        let restriction_ids = connection
            .prepare(V5_RESTRICTIONS_CONTEXT_ORDER_SQL)
            .unwrap()
            .query_map([], |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(
            restriction_ids,
            ["restriction:z-payment", "restriction:a-ui"]
        );
    }

    #[test]
    fn schema_v1_through_v4_images_are_rebuild_required_by_v5_reader() {
        let limits = IndexLimits::try_from(crate::StoreLimits::default()).unwrap();
        for found in [1_u32, 2, 3, 4] {
            let connection = rusqlite::Connection::open_in_memory().unwrap();
            super::super::configure_connection(&connection, limits, 0).unwrap();
            super::super::create_schema(&connection, limits).unwrap();
            connection
                .pragma_update(None, "user_version", found)
                .unwrap();
            let image = super::super::serialize_connection(&connection, limits).unwrap();
            assert!(matches!(
                super::super::deserialize_read_only_for_schema(image, limits, 0, 5),
                Err(IndexError::RebuildRequired {
                    found: actual,
                    required: 5
                }) if actual == found
            ));
        }
    }

    #[test]
    fn v5_full_readback_refuses_scalar_and_canonical_json_tamper() {
        let limits = IndexLimits::try_from(crate::StoreLimits::default()).unwrap();
        let mut snapshot = empty_snapshot("tamper");
        snapshot.program_relations.push(IndexProgramRelation {
            relation_id: StableId::parse("relation:v5-tamper").unwrap(),
            relation_kind: "calls".to_owned(),
            source_id: StableId::parse("node:v5-source").unwrap(),
            target_ids_canonical_json: "[]".to_owned(),
            body_hash: ContentHash::sha256(b"relation:v5-tamper"),
        });
        let connection = build_connection(&snapshot, limits, 0).unwrap();
        validate_v5_rows(&connection, &snapshot, limits).unwrap();
        connection.pragma_update(None, "query_only", false).unwrap();
        connection
            .execute("UPDATE index_meta SET run_id='run:v5-other'", [])
            .unwrap();
        assert!(matches!(
            validate_v5_rows(&connection, &snapshot, limits),
            Err(IndexError::CorruptIndex)
        ));
        connection
            .execute(
                "UPDATE index_meta SET run_id=?1",
                [snapshot.marker.run_id.to_string()],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE program_relations SET target_ids_canonical_json='[] '",
                [],
            )
            .unwrap();
        assert!(matches!(
            validate_v5_structure(&connection, limits),
            Err(IndexError::CorruptIndex)
        ));
    }

    #[test]
    fn v5_publication_failpoints_preserve_or_uncertain_at_the_rename_boundary() {
        for fault in [
            super::super::PublishFault::AfterWrite,
            super::super::PublishFault::BeforeCandidateSync,
            super::super::PublishFault::AfterCandidateLinkBeforeDirectorySync,
            super::super::PublishFault::AfterCandidateSync,
            super::super::PublishFault::AfterImageDropBeforeCandidateRead,
            super::super::PublishFault::AfterCandidateInodeCheck,
            super::super::PublishFault::AfterCandidateHashCheck,
            super::super::PublishFault::AfterValidation,
            super::super::PublishFault::AfterRename,
            super::super::PublishFault::AfterActiveInodeCheck,
            super::super::PublishFault::AfterActiveHashCheck,
            super::super::PublishFault::BeforeFinalDirectorySync,
            super::super::PublishFault::AfterActiveVerify,
        ] {
            let workspace = tempfile::tempdir().unwrap();
            let root = StoreRoot::open(workspace.path(), crate::StoreLimits::default()).unwrap();
            let index = DerivedIndexV5::open(&root).unwrap();
            let old_snapshot = empty_snapshot("old");
            let old_accounting = account_snapshot(&old_snapshot, 0, index.limits()).unwrap();
            let old_connection =
                build_connection(&old_snapshot, index.limits(), old_accounting.owned_bytes)
                    .unwrap();
            let old_image = super::super::serialize_connection_with_retained(
                &old_connection,
                index.limits(),
                old_accounting.owned_bytes,
            )
            .unwrap();
            let replacement_snapshot = empty_snapshot("replacement");
            let replacement_accounting =
                account_snapshot(&replacement_snapshot, 0, index.limits()).unwrap();
            let replacement_connection = build_connection(
                &replacement_snapshot,
                index.limits(),
                replacement_accounting.owned_bytes,
            )
            .unwrap();
            let replacement_image = super::super::serialize_connection_with_retained(
                &replacement_connection,
                index.limits(),
                replacement_accounting.owned_bytes,
            )
            .unwrap();
            let lock = index.inner.lock_exclusive().unwrap();
            index
                .publish_v5_image_locked(
                    old_image.clone(),
                    &old_snapshot,
                    &old_accounting,
                    v5_sqlite_limits(index.inner.limits).unwrap(),
                )
                .unwrap();
            index.inject_publish_fault(fault);
            let result = index.publish_v5_image_locked(
                replacement_image.clone(),
                &replacement_snapshot,
                &replacement_accounting,
                v5_sqlite_limits(index.inner.limits).unwrap(),
            );
            let before_rename = matches!(
                fault,
                super::super::PublishFault::AfterWrite
                    | super::super::PublishFault::BeforeCandidateSync
                    | super::super::PublishFault::AfterCandidateLinkBeforeDirectorySync
                    | super::super::PublishFault::AfterCandidateSync
                    | super::super::PublishFault::AfterImageDropBeforeCandidateRead
                    | super::super::PublishFault::AfterCandidateInodeCheck
                    | super::super::PublishFault::AfterCandidateHashCheck
                    | super::super::PublishFault::AfterValidation
            );
            lock.verify_unchanged().unwrap();
            drop(lock);
            if before_rename {
                assert!(matches!(result, Err(IndexError::Io(_))), "{fault:?}");
                assert_eq!(index.inner.read_active_image().unwrap(), old_image);
            } else {
                assert!(
                    matches!(
                        result,
                        Err(IndexError::PublicationDurabilityUncertain { .. })
                    ),
                    "{fault:?}"
                );
                assert_eq!(index.inner.read_active_image().unwrap(), replacement_image);
            }
        }
    }
}
