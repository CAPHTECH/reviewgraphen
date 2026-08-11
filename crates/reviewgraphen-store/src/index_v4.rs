//! Authority-bound event-v3 SQLite projection.
//!
//! This module is a child of `index` so it can reuse the descriptor-anchored
//! image transport without making those security-sensitive primitives public.

use super::{
    IndexClaim, IndexContextEnvelope, IndexError, IndexEvent, IndexExecution, IndexFinding,
    IndexLimits, IndexObligation, IndexObligationLifecycle, IndexProgramObject,
    IndexProgramRelation, IndexReviewPlan, IndexShadow, IndexSnapshotSource, IndexUniverse,
};
use crate::{EventJournal, StoreRoot, StoreRootIdentity, journal::IndexV4ReplayedPrefix};
use reviewgraphen_core::{
    ArtifactSourceV3, AuthorityReplayBasisV3, AuthorityTrustRootsV3, ContentHash, DecodedPayload,
    StableId, canonical_json,
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
use std::{collections::BTreeSet, fs::File, io::Write};

pub const INDEX_SCHEMA_VERSION_V4: u32 = 4;
pub const PROJECTION_CONTRACT_VERSION_V4: &str = "reviewgraphen.index_projection.v4";
const EVENT_CONTRACT_VERSION_V3: &str = "reviewgraphen.review_event.v3";
const PROJECTION_MODE_V3: &str = "v3_authority";
// Separate from ADR 0021 Working4. This is the Store's process-local hard
// ceiling for the non-normative typed-snapshot selector and deliberately
// never scales up from caller input.
const V4_SELECTION_OPERATIONAL_LIMIT: u64 = 256 * 1024 * 1024;
const V4_JSON_STAGING_OPERATIONAL_LIMIT: u64 = 64 * 1024 * 1024;

#[cfg(test)]
thread_local! {
    static V4_PROJECTION_DECODE_COUNT: Cell<u64> = const { Cell::new(0) };
    static V4_PROJECTION_OBLIGATION_PROBE_COUNT: Cell<u64> = const { Cell::new(0) };
    static V4_SELECTION_OPERATIONAL_LIMIT_OVERRIDE: Cell<Option<u64>> = const { Cell::new(None) };
    static V4_JSON_STAGING_LIMIT_OVERRIDE: Cell<Option<u64>> = const { Cell::new(None) };
    static V4_JSON_DECODE_ALLOCATION_COUNT: Cell<u64> = const { Cell::new(0) };
    static V4_FULL_SNAPSHOT_CONSTRUCTION_COUNT: Cell<u64> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_projection_decode_count_for_test() {
    V4_PROJECTION_DECODE_COUNT.with(|count| count.set(0));
    V4_PROJECTION_OBLIGATION_PROBE_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn projection_decode_count_for_test() -> u64 {
    V4_PROJECTION_DECODE_COUNT.with(Cell::get)
}

#[cfg(test)]
pub(crate) fn projection_obligation_probe_count_for_test() -> u64 {
    V4_PROJECTION_OBLIGATION_PROBE_COUNT.with(Cell::get)
}

#[cfg(test)]
fn record_projection_decode() {
    V4_PROJECTION_DECODE_COUNT.with(|count| count.set(count.get().saturating_add(1)));
}

#[cfg(not(test))]
fn record_projection_decode() {}

#[cfg(test)]
fn record_projection_obligation_probe() {
    V4_PROJECTION_OBLIGATION_PROBE_COUNT.with(|count| count.set(count.get().saturating_add(1)));
}

#[cfg(not(test))]
fn record_projection_obligation_probe() {}

fn v4_selection_operational_limit() -> u64 {
    #[cfg(test)]
    if let Some(limit) = V4_SELECTION_OPERATIONAL_LIMIT_OVERRIDE.with(Cell::get) {
        return limit;
    }
    V4_SELECTION_OPERATIONAL_LIMIT
}

#[cfg(test)]
pub(crate) fn set_v4_selection_operational_limit_for_test(limit: Option<u64>) {
    V4_SELECTION_OPERATIONAL_LIMIT_OVERRIDE.with(|value| value.set(limit));
}

fn v4_json_staging_limit() -> u64 {
    #[cfg(test)]
    if let Some(limit) = V4_JSON_STAGING_LIMIT_OVERRIDE.with(Cell::get) {
        return limit;
    }
    V4_JSON_STAGING_OPERATIONAL_LIMIT
}

fn admit_v4_json_staging(observed: u64) -> Result<(), IndexError> {
    let limit = v4_json_staging_limit();
    if observed > limit {
        return Err(IndexError::Incomplete { limit, observed });
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn set_v4_json_staging_limit_for_test(limit: Option<u64>) {
    V4_JSON_STAGING_LIMIT_OVERRIDE.with(|value| value.set(limit));
}

#[cfg(test)]
pub(crate) fn reset_v4_json_decode_allocation_count_for_test() {
    V4_JSON_DECODE_ALLOCATION_COUNT.with(|value| value.set(0));
}

#[cfg(test)]
pub(crate) fn v4_json_decode_allocation_count_for_test() -> u64 {
    V4_JSON_DECODE_ALLOCATION_COUNT.with(Cell::get)
}

#[cfg(test)]
pub(crate) fn reset_v4_full_snapshot_construction_count_for_test() {
    V4_FULL_SNAPSHOT_CONSTRUCTION_COUNT.with(|value| value.set(0));
}

#[cfg(test)]
pub(crate) fn v4_full_snapshot_construction_count_for_test() -> u64 {
    V4_FULL_SNAPSHOT_CONSTRUCTION_COUNT.with(Cell::get)
}

#[cfg(test)]
fn record_v4_full_snapshot_construction() {
    V4_FULL_SNAPSHOT_CONSTRUCTION_COUNT.with(|value| value.set(value.get().saturating_add(1)));
}

#[cfg(not(test))]
fn record_v4_full_snapshot_construction() {}

#[cfg(test)]
fn record_v4_json_decode_allocation() {
    V4_JSON_DECODE_ALLOCATION_COUNT.with(|value| value.set(value.get().saturating_add(1)));
}

#[cfg(not(test))]
fn record_v4_json_decode_allocation() {}

/// Complete schema-v4 literal.  In particular, this is deliberately not a
/// schema-v3 migration or a runtime concatenation of older DDL.
pub(crate) const SCHEMA_V4: &str = r#"
CREATE TABLE index_meta (
 singleton INTEGER PRIMARY KEY CHECK (singleton=1),
 index_schema_version INTEGER NOT NULL CHECK (index_schema_version=4),
 projection_contract_version TEXT NOT NULL CHECK (projection_contract_version='reviewgraphen.index_projection.v4'),
 event_contract_version TEXT NOT NULL CHECK (event_contract_version='reviewgraphen.review_event.v3'),
 projection_mode TEXT NOT NULL CHECK (projection_mode='v3_authority'),
 run_id TEXT NOT NULL, genesis_hash TEXT NOT NULL,
 confirmed_offset INTEGER NOT NULL CHECK (confirmed_offset>=0), tail_hash TEXT NOT NULL,
 event_count INTEGER NOT NULL CHECK (event_count>=0),
 policy_revision_hash TEXT NOT NULL,
 authority_replay_basis_digest TEXT NOT NULL
) STRICT;
CREATE TABLE events (
 sequence INTEGER PRIMARY KEY CHECK (sequence>0), event_id TEXT NOT NULL UNIQUE,
 schema TEXT NOT NULL CHECK (schema='reviewgraphen.review_event.v3'),
 event_hash TEXT NOT NULL, payload_hash TEXT NOT NULL,
 payload_kind TEXT NOT NULL CHECK (payload_kind IN (
  'obligation_transition','run_genesis_manifest','artifact_registered',
  'snapshot_sources_recorded','review_plan_recorded','context_envelope_projected',
  'review_execution_recorded','evidence_recorded_v3','evidence_bound_v3',
  'verification_recorded_v3','decision_recorded_v3','finding_recorded_v3'
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
"#;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IndexMarkerV4 {
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IndexArtifactRegistrationV4 {
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
    pub source: ArtifactSourceV3,
    pub body_hash: ContentHash,
}

macro_rules! m4_row {
    ($name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Clone, Debug, Eq, PartialEq, Serialize)]
        pub struct $name { $(pub $field: $ty),* }
    };
}

m4_row!(IndexEvidenceV3 {
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
m4_row!(IndexEvidenceBindingV3 {
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
m4_row!(IndexVerificationV3 {
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
m4_row!(IndexDecisionV3 {
    event_sequence: u64, event_id: StableId, decision_id: StableId, schema: String,
    policy_revision_hash: ContentHash, run_id: StableId, universe_id: StableId,
    claim_id: StableId, property_id: String, outcome: String, actor: String,
    authority_id: String, snapshot_id: StableId, source_ids_canonical_json: String,
    rationale: String, issued_at: String, expires_at: Option<String>, body_hash: ContentHash,
});
m4_row!(IndexFindingV3 {
    event_sequence: u64, event_id: StableId, finding_id: StableId, schema: String,
    projection_descriptor_id: String, claim_id: StableId, status: String,
    evidence_ids_canonical_json: String, verification_ids_canonical_json: String,
    decision_id: Option<StableId>, supersedes_finding_id: Option<StableId>, body_hash: ContentHash,
});
m4_row!(IndexClaimAssessmentV3 {
    claim_id: StableId, disposition: String, review_status: String,
    binding_ids_canonical_json: String, evidence_ids_canonical_json: String,
    verification_ids_canonical_json: String, decision_ids_canonical_json: String,
    finding_ids_canonical_json: String, active_decision_id: Option<StableId>,
    current_finding_id: Option<StableId>, decision_conflict: bool,
    confirmed_event_sequence: u64,
});

/// Complete read-only schema-v4 projection.  The two legacy arrays remain
/// explicit even though an event-v3 authority replay normally leaves them
/// empty.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IndexSnapshotV4 {
    pub marker: IndexMarkerV4,
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
    pub artifact_registrations: Vec<IndexArtifactRegistrationV4>,
    pub snapshot_sources: Vec<IndexSnapshotSource>,
    pub context_envelopes: Vec<IndexContextEnvelope>,
    pub review_plans: Vec<IndexReviewPlan>,
    pub evidence: Vec<IndexEvidenceV3>,
    pub evidence_bindings: Vec<IndexEvidenceBindingV3>,
    pub verifications: Vec<IndexVerificationV3>,
    pub decisions: Vec<IndexDecisionV3>,
    pub findings: Vec<IndexFindingV3>,
    pub claim_assessments: Vec<IndexClaimAssessmentV3>,
    pub policy_revision_hash: ContentHash,
    pub authority_replay_basis_digest: ContentHash,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexAccountingV4 {
    pub rows: u64,
    pub integer_cells: u64,
    pub text_bytes: u64,
    pub sql_bytes: u64,
    pub query_bytes: u64,
    pub owned_bytes: u64,
    pub working_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexRebuildReceiptV4 {
    pub run_id: StableId,
    pub genesis_hash: ContentHash,
    pub confirmed_offset: u64,
    pub tail_hash: ContentHash,
    pub event_count: u64,
    pub policy_revision_hash: ContentHash,
    pub authority_replay_basis_digest: ContentHash,
    pub serialized_bytes: u64,
    pub image_hash: ContentHash,
    pub accounting: IndexAccountingV4,
}

/// Caller-owned selection key for the read-only report-v3 projection seam.
/// The selected IDs are borrowed; Store never turns them into report-owned
/// wire data.
#[derive(Clone, Copy)]
pub struct V4SelectionRequest<'a> {
    pub plan_id: &'a StableId,
    pub selected_obligation_ids: &'a BTreeSet<StableId>,
    pub expected_confirmed_offset: u64,
    pub expected_event_count: u64,
    pub expected_tail_hash: &'a ContentHash,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum V4CoverageAxis {
    Denominator,
    Visited,
    Completed,
    EvidenceSupported,
    Verified,
    Accepted,
}

/// One borrowed row or semantic coverage ID in deterministic report order.
pub enum V4SelectionItem<'a> {
    ArtifactRegistration(&'a IndexArtifactRegistrationV4),
    Execution(&'a IndexExecution),
    Claim(&'a IndexClaim),
    Evidence(&'a IndexEvidenceV3),
    EvidenceBinding(&'a IndexEvidenceBindingV3),
    Verification(&'a IndexVerificationV3),
    Decision(&'a IndexDecisionV3),
    Finding(&'a IndexFindingV3),
    ClaimAssessment(&'a IndexClaimAssessmentV3),
    Obstruction(&'a IndexExecution),
    CoverageId {
        axis: V4CoverageAxis,
        id: &'a StableId,
    },
}

pub trait V4SelectionVisitor {
    type Error;

    fn visit(&mut self, item: V4SelectionItem<'_>) -> Result<(), Self::Error>;
}

#[derive(Debug)]
pub enum V4SelectionVisitError<E> {
    Index(IndexError),
    Visitor(E),
}

impl<E> From<IndexError> for V4SelectionVisitError<E> {
    fn from(value: IndexError) -> Self {
        Self::Index(value)
    }
}

impl<E> From<rusqlite::Error> for V4SelectionVisitError<E> {
    fn from(value: rusqlite::Error) -> Self {
        Self::Index(IndexError::from(value))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct V4SelectionCounts {
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
pub struct V4SelectionSummary {
    pub counts: V4SelectionCounts,
    pub max_cas_bytes: u64,
    pub confirmed_offset: u64,
    pub event_count: u64,
    pub marker_fingerprint: [u8; 32],
}

/// Schema-v4 handle.  The descriptor-safe directory admission is shared with
/// the frozen v3 implementation, while all schema and projection semantics
/// remain version-specific.
pub struct DerivedIndexV4<'a> {
    inner: super::DerivedIndex<'a>,
}

/// Opaque authority- and image-bound owner of one validated schema-v4
/// snapshot. It is intentionally neither `Clone` nor serializable: report
/// passes borrow the same typed image through this capability.
pub struct ValidatedIndexSnapshotV4<'index, 'root, 'roots> {
    index: &'index DerivedIndexV4<'root>,
    roots: &'roots AuthorityTrustRootsV3,
    snapshot: IndexSnapshotV4,
    store_root_identity: StoreRootIdentity,
    image_hash: ContentHash,
    marker_fingerprint: [u8; 32],
}

impl ValidatedIndexSnapshotV4<'_, '_, '_> {
    #[must_use]
    pub const fn snapshot(&self) -> &IndexSnapshotV4 {
        &self.snapshot
    }

    fn into_snapshot(self) -> IndexSnapshotV4 {
        self.snapshot
    }
}

impl<'a> DerivedIndexV4<'a> {
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

    pub fn rebuild_v4(
        &self,
        journal: &EventJournal<'_>,
        roots: &AuthorityTrustRootsV3,
    ) -> Result<IndexRebuildReceiptV4, IndexError> {
        let lock = self.inner.lock_exclusive()?;
        self.inner.audit_candidates(true)?;
        let (session, basis) = journal.replayed_v3_session(roots)?;
        if !session.matches_store_root(self.inner.root)? {
            return Err(IndexError::ProjectionContractViolation);
        }
        let projected = project_verified_source(&session, &basis, self.inner.limits)?;
        let snapshot = projected.snapshot;
        let accounting = projected.accounting;
        let sqlite_limits = v4_sqlite_limits(self.inner.limits)?;
        let connection = build_connection(&snapshot, sqlite_limits, accounting.owned_bytes)?;
        let image = super::serialize_connection_with_retained(
            &connection,
            sqlite_limits,
            accounting.owned_bytes,
        )?;
        drop(connection);
        let serialized_bytes =
            u64::try_from(image.len()).map_err(|_| IndexError::IntegerOutOfRange)?;
        let image_hash =
            self.publish_v4_image_locked(image, &snapshot, &accounting, sqlite_limits)?;
        lock.verify_unchanged()?;
        Ok(IndexRebuildReceiptV4 {
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

    pub fn validated_snapshot_current_v4<'index, 'roots>(
        &'index self,
        journal: &EventJournal<'_>,
        roots: &'roots AuthorityTrustRootsV3,
    ) -> Result<ValidatedIndexSnapshotV4<'index, 'a, 'roots>, IndexError> {
        let lock = self.inner.lock_shared()?;
        self.inner.audit_candidates(false)?;
        let (session, basis) = journal.replayed_v3_session(roots)?;
        if !session.matches_store_root(self.inner.root)? {
            return Err(IndexError::ProjectionContractViolation);
        }
        let projected = project_verified_source(&session, &basis, self.inner.limits)?;
        let expected = projected.snapshot;
        let retained = projected.accounting.owned_bytes;
        let sqlite_limits = v4_sqlite_limits(self.inner.limits)?;
        let image = self
            .inner
            .read_active_image_locked_with_limits_and_retained(sqlite_limits, retained)?;
        let image_hash = ContentHash::sha256(&image);
        let connection = super::deserialize_read_only_for_schema(
            image,
            sqlite_limits,
            retained,
            INDEX_SCHEMA_VERSION_V4,
        )
        .map_err(super::normalize_external_image_error)?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version == 3 {
            return Err(IndexError::RebuildRequired {
                found: 3,
                required: 4,
            });
        }
        if version != 4 {
            return Err(IndexError::CorruptIndex);
        }
        let max_json_staging = preflight_v4_structure(&connection, self.inner.limits)
            .map_err(super::normalize_external_image_error)?;
        admit_v4_json_staging(max_json_staging)?;
        validate_v4_structure_after_preflight(&connection, self.inner.limits)
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
        let committed_marker = marker_v4_from_connection(&connection)?;
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
                .index_v4_replay_prefix(
                    committed_marker.event_count,
                    committed_marker.confirmed_offset,
                )
                .map_err(|_| IndexError::CorruptIndex)?;
            let indexed = project_verified_source(&prefix, prefix.basis(), self.inner.limits)?;
            if indexed.snapshot.marker != committed_marker {
                return Err(IndexError::CorruptIndex);
            }
            validate_v4_rows(&connection, &indexed.snapshot, self.inner.limits)
                .map_err(super::normalize_external_image_error)?;
            return Err(IndexError::CommittedIndexStale {
                indexed_offset: committed_marker.confirmed_offset,
                indexed_tail: committed_marker.tail_hash,
                committed_offset,
                committed_tail,
            });
        }
        validate_v4_rows(&connection, &expected, self.inner.limits)
            .map_err(super::normalize_external_image_error)?;
        drop(connection);
        lock.verify_unchanged()?;
        let marker_fingerprint = v4_marker_fingerprint(&expected.marker)?;
        Ok(ValidatedIndexSnapshotV4 {
            index: self,
            roots,
            snapshot: expected,
            store_root_identity: self.inner.root.identity().clone(),
            image_hash,
            marker_fingerprint,
        })
    }

    pub fn snapshot_current_v4(
        &self,
        journal: &EventJournal<'_>,
        roots: &AuthorityTrustRootsV3,
    ) -> Result<IndexSnapshotV4, IndexError> {
        Ok(self
            .validated_snapshot_current_v4(journal, roots)?
            .into_snapshot())
    }

    /// Visits the exact report-v3 selection from one roots-bound current-v4
    /// view. Callers may run this twice (measure, then materialize) and compare
    /// the fixed summary to detect any source drift without Store returning an
    /// ID vector or report wire representation.
    pub fn visit_current_v4_selection<V: V4SelectionVisitor>(
        &self,
        journal: &EventJournal<'_>,
        roots: &AuthorityTrustRootsV3,
        request: V4SelectionRequest<'_>,
        visitor: &mut V,
    ) -> Result<V4SelectionSummary, V4SelectionVisitError<V::Error>> {
        self.validated_snapshot_current_v4(journal, roots)?
            .visit_selection(journal, request, visitor)
    }
}

impl ValidatedIndexSnapshotV4<'_, '_, '_> {
    /// Visits a selection against this exact retained snapshot. The active
    /// image and roots-bound journal marker are revalidated under their locks,
    /// but no second `IndexSnapshotV4` is projected or allocated.
    pub fn visit_selection<V: V4SelectionVisitor>(
        &self,
        journal: &EventJournal<'_>,
        request: V4SelectionRequest<'_>,
        visitor: &mut V,
    ) -> Result<V4SelectionSummary, V4SelectionVisitError<V::Error>> {
        let snapshot = &self.snapshot;
        if snapshot.marker.confirmed_offset != request.expected_confirmed_offset
            || snapshot.marker.event_count != request.expected_event_count
            || &snapshot.marker.tail_hash != request.expected_tail_hash
            || v4_marker_fingerprint(&snapshot.marker)? != self.marker_fingerprint
        {
            return Err(IndexError::ProjectionContractViolation.into());
        }
        #[cfg(test)]
        let oracle_summary = {
            struct OracleNoop;
            impl V4SelectionVisitor for OracleNoop {
                type Error = ();
                fn visit(&mut self, _item: V4SelectionItem<'_>) -> Result<(), Self::Error> {
                    Ok(())
                }
            }
            visit_v4_snapshot_selection(snapshot, request, &mut OracleNoop).map_err(|error| {
                match error {
                    V4SelectionVisitError::Index(error) => V4SelectionVisitError::Index(error),
                    V4SelectionVisitError::Visitor(()) => {
                        V4SelectionVisitError::Index(IndexError::ProjectionContractViolation)
                    }
                }
            })?
        };
        let operational = account_snapshot(snapshot, 0, self.index.inner.limits)?.sql_bytes;
        let operational_limit = v4_selection_operational_limit();
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
            .replayed_v3_session(self.roots)
            .map_err(IndexError::from)?;
        let session_matches_root = session
            .matches_store_root(self.index.inner.root)
            .map_err(IndexError::from)?;
        let session_offset = session
            .index_v4_confirmed_offset()
            .map_err(IndexError::from)?;
        let session_event_count = u64::try_from(session.event_count().map_err(IndexError::from)?)
            .map_err(|_| IndexError::IntegerOutOfRange)?;
        let session_tail = session.tail_hash().map_err(IndexError::from)?;
        let session_run = session.run_id().map_err(IndexError::from)?;
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
        let sqlite_limits = v4_sqlite_limits(self.index.inner.limits)?;
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
            INDEX_SCHEMA_VERSION_V4,
        )
        .map_err(super::normalize_external_image_error)?;
        let max_json_staging = preflight_v4_structure(&connection, self.index.inner.limits)
            .map_err(super::normalize_external_image_error)?;
        admit_v4_json_staging(max_json_staging)?;
        validate_v4_structure_after_preflight(&connection, self.index.inner.limits)
            .map_err(super::normalize_external_image_error)?;
        let active_marker = marker_v4_from_connection(&connection)?;
        if active_marker != snapshot.marker
            || v4_marker_fingerprint(&active_marker)? != self.marker_fingerprint
        {
            return Err(IndexError::CorruptIndex.into());
        }
        validate_v4_rows(&connection, snapshot, self.index.inner.limits)
            .map_err(super::normalize_external_image_error)?;
        install_v4_selected_obligations(&connection, request.selected_obligation_ids)?;
        let summary = visit_v4_sql_selection(&connection, snapshot, request, visitor)?;
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

fn install_v4_selected_obligations(
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

fn visit_v4_sql_selection<V: V4SelectionVisitor>(
    connection: &rusqlite::Connection,
    snapshot: &IndexSnapshotV4,
    request: V4SelectionRequest<'_>,
    visitor: &mut V,
) -> Result<V4SelectionSummary, V4SelectionVisitError<V::Error>> {
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
    let mut counts = V4SelectionCounts::default();
    let selector = V4SqlSelector {
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
                |row| V4SelectionItem::$variant(row),
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
        V4SelectionItem::Evidence,
        visitor,
    )?;
    let registration_sql = format!(
        "{SX}SELECT DISTINCT ar.event_sequence,ar.registration_id FROM artifact_registrations ar JOIN (SELECT raw_registration_id id FROM sx UNION SELECT ev.input_registration_id FROM evidence_v3 ev JOIN evidence_bindings_v3 b ON b.evidence_id=ev.evidence_id JOIN claims c ON c.claim_id=b.claim_id JOIN sx ON sx.execution_id=c.execution_id UNION SELECT ev.output_registration_id FROM evidence_v3 ev JOIN evidence_bindings_v3 b ON b.evidence_id=ev.evidence_id JOIN claims c ON c.claim_id=b.claim_id JOIN sx ON sx.execution_id=c.execution_id UNION SELECT v.input_registration_id FROM verifications_v3 v JOIN claims c ON c.claim_id=v.claim_id JOIN sx ON sx.execution_id=c.execution_id UNION SELECT v.output_registration_id FROM verifications_v3 v JOIN claims c ON c.claim_id=v.claim_id JOIN sx ON sx.execution_id=c.execution_id) wanted ON wanted.id=ar.registration_id ORDER BY ar.event_sequence,ar.registration_id"
    );
    counts.artifact_registrations = selector.emit_keyed_rows(
        &registration_sql,
        &snapshot.artifact_registrations,
        |row| (&row.registration_id, row.event_sequence),
        V4SelectionItem::ArtifactRegistration,
        visitor,
    )?;
    let assessment_sql = format!(
        "{SX}SELECT 0,a.claim_id FROM claim_assessments_v3 a JOIN claims c ON c.claim_id=a.claim_id JOIN sx ON sx.execution_id=c.execution_id ORDER BY a.claim_id"
    );
    counts.claim_assessments = selector.emit_keyed_rows(
        &assessment_sql,
        &snapshot.claim_assessments,
        |row| (&row.claim_id, 0),
        V4SelectionItem::ClaimAssessment,
        visitor,
    )?;
    let obstruction_sql = format!(
        "{SX}SELECT event_sequence,execution_id FROM sx WHERE outcome_kind<>'structured' ORDER BY event_sequence,execution_id"
    );
    counts.obstructions = selector.emit_keyed_rows(
        &obstruction_sql,
        &snapshot.executions,
        |row| (&row.execution_id, row.event_sequence),
        V4SelectionItem::Obstruction,
        visitor,
    )?;

    counts.denominator_ids = emit_v4_coverage_ids(
        connection,
        "SELECT obligation_id FROM obligations ORDER BY obligation_id",
        request.plan_id,
        &snapshot.obligations,
        V4CoverageAxis::Denominator,
        visitor,
    )?;
    counts.visited_ids = emit_v4_coverage_ids(
        connection,
        &format!(
            "{SX}SELECT DISTINCT json_extract(obligation_ids_canonical_json,'$[0]') FROM sx ORDER BY 1"
        ),
        request.plan_id,
        &snapshot.obligations,
        V4CoverageAxis::Visited,
        visitor,
    )?;
    counts.completed_ids = emit_v4_coverage_ids(
        connection,
        &format!(
            "{SX}SELECT DISTINCT o.obligation_id FROM obligations o JOIN sx ON json_extract(sx.obligation_ids_canonical_json,'$[0]')=o.obligation_id WHERE o.lifecycle='completed' AND sx.outcome_kind='structured' ORDER BY o.obligation_id"
        ),
        request.plan_id,
        &snapshot.obligations,
        V4CoverageAxis::Completed,
        visitor,
    )?;
    counts.evidence_supported_ids = emit_v4_coverage_ids(
        connection,
        &format!(
            "{SX}SELECT DISTINCT json_extract(c.obligation_ids_canonical_json,'$[0]') FROM claims c JOIN sx ON sx.execution_id=c.execution_id JOIN evidence_bindings_v3 b ON b.claim_id=c.claim_id WHERE b.relation='reproduces' ORDER BY 1"
        ),
        request.plan_id,
        &snapshot.obligations,
        V4CoverageAxis::EvidenceSupported,
        visitor,
    )?;
    let verified_sql = format!(
        "{SX}SELECT DISTINCT json_extract(c.obligation_ids_canonical_json,'$[0]') FROM claims c JOIN sx ON sx.execution_id=c.execution_id JOIN verifications_v3 v ON v.claim_id=c.claim_id WHERE v.outcome='passed' AND json_array_length(v.evidence_ids_canonical_json)>0 AND NOT EXISTS (SELECT 1 FROM json_each(v.evidence_ids_canonical_json) cited WHERE NOT EXISTS (SELECT 1 FROM evidence_bindings_v3 b WHERE b.claim_id=c.claim_id AND b.evidence_id=cited.value AND b.relation='reproduces')) ORDER BY 1"
    );
    counts.verified_ids = emit_v4_coverage_ids(
        connection,
        &verified_sql,
        request.plan_id,
        &snapshot.obligations,
        V4CoverageAxis::Verified,
        visitor,
    )?;
    let accepted_sql = "WITH sx AS (SELECT e.* FROM executions e JOIN temp.selected_obligations s ON s.id=json_extract(e.obligation_ids_canonical_json,'$[0]') WHERE e.plan_id=?1 AND json_array_length(e.obligation_ids_canonical_json)=1), qualifying AS (SELECT v.claim_id,v.verification_id FROM verifications_v3 v WHERE v.outcome='passed' AND json_array_length(v.evidence_ids_canonical_json)>0 AND NOT EXISTS (SELECT 1 FROM json_each(v.evidence_ids_canonical_json) cited WHERE NOT EXISTS (SELECT 1 FROM evidence_bindings_v3 b WHERE b.claim_id=v.claim_id AND b.evidence_id=cited.value AND b.relation='reproduces'))) SELECT DISTINCT json_extract(c.obligation_ids_canonical_json,'$[0]') FROM claims c JOIN sx ON sx.execution_id=c.execution_id JOIN claim_assessments_v3 a ON a.claim_id=c.claim_id JOIN findings_v3 f ON f.finding_id=a.current_finding_id AND f.claim_id=c.claim_id JOIN decisions_v3 d ON d.decision_id=a.active_decision_id AND d.claim_id=c.claim_id JOIN qualifying q ON q.claim_id=c.claim_id WHERE a.decision_conflict=0 AND f.status='accepted' AND f.decision_id=d.decision_id AND d.outcome='accept' AND NOT EXISTS (SELECT 1 FROM json_each(f.evidence_ids_canonical_json) fe WHERE NOT EXISTS (SELECT 1 FROM evidence_bindings_v3 b WHERE b.claim_id=c.claim_id AND b.evidence_id=fe.value AND b.relation='reproduces')) AND NOT EXISTS (SELECT 1 FROM evidence_bindings_v3 b WHERE b.claim_id=c.claim_id AND b.relation='reproduces' AND NOT EXISTS (SELECT 1 FROM json_each(f.evidence_ids_canonical_json) fe WHERE fe.value=b.evidence_id)) AND NOT EXISTS (SELECT 1 FROM json_each(f.verification_ids_canonical_json) fv WHERE NOT EXISTS (SELECT 1 FROM qualifying q2 WHERE q2.claim_id=c.claim_id AND q2.verification_id=fv.value)) AND NOT EXISTS (SELECT 1 FROM qualifying q2 WHERE q2.claim_id=c.claim_id AND NOT EXISTS (SELECT 1 FROM json_each(f.verification_ids_canonical_json) fv WHERE fv.value=q2.verification_id)) ORDER BY 1";
    counts.accepted_ids = emit_v4_coverage_ids(
        connection,
        accepted_sql,
        request.plan_id,
        &snapshot.obligations,
        V4CoverageAxis::Accepted,
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
    Ok(V4SelectionSummary {
        counts,
        max_cas_bytes,
        confirmed_offset: snapshot.marker.confirmed_offset,
        event_count: snapshot.marker.event_count,
        marker_fingerprint: v4_marker_fingerprint(&snapshot.marker)?,
    })
}

struct V4SqlSelector<'a> {
    connection: &'a rusqlite::Connection,
    plan_id: &'a StableId,
}

impl V4SqlSelector<'_> {
    fn emit_keyed_rows<'a, T, V, K, I>(
        &self,
        sql: &str,
        source: &'a [T],
        key: K,
        item: I,
        visitor: &mut V,
    ) -> Result<u64, V4SelectionVisitError<V::Error>>
    where
        V: V4SelectionVisitor,
        K: Fn(&T) -> (&StableId, u64),
        I: Fn(&'a T) -> V4SelectionItem<'a>,
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
                .map_err(V4SelectionVisitError::Visitor)?;
            cursor += 1;
            count = count.checked_add(1).ok_or(IndexError::IntegerOutOfRange)?;
        }
        Ok(count)
    }
}

fn emit_v4_coverage_ids<V: V4SelectionVisitor>(
    connection: &rusqlite::Connection,
    sql: &str,
    plan_id: &StableId,
    denominator: &[IndexObligation],
    axis: V4CoverageAxis,
    visitor: &mut V,
) -> Result<u64, V4SelectionVisitError<V::Error>> {
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
            .visit(V4SelectionItem::CoverageId {
                axis,
                id: &obligation.obligation_id,
            })
            .map_err(V4SelectionVisitError::Visitor)?;
        cursor += 1;
        count = count.checked_add(1).ok_or(IndexError::IntegerOutOfRange)?;
    }
    Ok(count)
}

#[cfg(test)]
fn visit_v4_snapshot_selection<V: V4SelectionVisitor>(
    snapshot: &IndexSnapshotV4,
    request: V4SelectionRequest<'_>,
    visitor: &mut V,
) -> Result<V4SelectionSummary, V4SelectionVisitError<V::Error>> {
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
        v4_selection_operational_upper_bound(snapshot, request.selected_obligation_ids)?;
    let operational_limit = V4_SELECTION_OPERATIONAL_LIMIT;
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
        || !v4_plan_contains_all(&plan.waves_canonical_json, request.selected_obligation_ids)?
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
        let scope = v4_strict_ids(&row.obligation_ids_canonical_json)?;
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
        let scope = v4_strict_ids(&row.obligation_ids_canonical_json)?;
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
        let expected = v4_strict_ids(&execution.parsed_claim_ids_canonical_json)?;
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
        if !v4_strict_ids(&row.evidence_ids_canonical_json)?.is_subset(&evidence_ids) {
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
        let cited = v4_strict_ids(&row.evidence_ids_canonical_json)?;
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
            && &v4_strict_ids(&finding.evidence_ids_canonical_json)? == all_reproduces
            && v4_strict_ids(&finding.verification_ids_canonical_json)?
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

    let mut counts = V4SelectionCounts::default();
    macro_rules! emit_rows {
        ($rows:expr, $predicate:expr, $variant:ident, $count:ident) => {
            for row in $rows.iter().filter($predicate) {
                visitor
                    .visit(V4SelectionItem::$variant(row))
                    .map_err(V4SelectionVisitError::Visitor)?;
                counts.$count = counts
                    .$count
                    .checked_add(1)
                    .ok_or(V4SelectionVisitError::Index(IndexError::IntegerOutOfRange))?;
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
            V4CoverageAxis::Denominator,
            &denominator,
            &mut counts.denominator_ids,
        ),
        (V4CoverageAxis::Visited, &visited, &mut counts.visited_ids),
        (
            V4CoverageAxis::Completed,
            &completed,
            &mut counts.completed_ids,
        ),
        (
            V4CoverageAxis::EvidenceSupported,
            &evidence_supported,
            &mut counts.evidence_supported_ids,
        ),
        (
            V4CoverageAxis::Verified,
            &verified,
            &mut counts.verified_ids,
        ),
        (
            V4CoverageAxis::Accepted,
            &accepted,
            &mut counts.accepted_ids,
        ),
    ] {
        for id in ids {
            visitor
                .visit(V4SelectionItem::CoverageId { axis, id })
                .map_err(V4SelectionVisitError::Visitor)?;
            *count = count
                .checked_add(1)
                .ok_or(V4SelectionVisitError::Index(IndexError::IntegerOutOfRange))?;
        }
    }
    let max_cas_bytes = snapshot
        .artifact_registrations
        .iter()
        .filter(|row| registration_ids.contains(&row.registration_id))
        .map(|row| row.size)
        .max()
        .unwrap_or(0);
    Ok(V4SelectionSummary {
        counts,
        max_cas_bytes,
        confirmed_offset: snapshot.marker.confirmed_offset,
        event_count: snapshot.marker.event_count,
        marker_fingerprint: v4_marker_fingerprint(&snapshot.marker)?,
    })
}

#[cfg(test)]
fn v4_selection_operational_upper_bound(
    snapshot: &IndexSnapshotV4,
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
pub(crate) fn v4_selection_sql_operational_charge_for_test(
    snapshot: &IndexSnapshotV4,
    _selected: &BTreeSet<StableId>,
    limits: IndexLimits,
) -> Result<u64, IndexError> {
    Ok(account_snapshot(snapshot, 0, limits)?.sql_bytes)
}

#[cfg(test)]
fn v4_strict_ids(input: &str) -> Result<BTreeSet<StableId>, IndexError> {
    let values: Vec<StableId> =
        serde_json::from_str(input).map_err(|_| IndexError::ProjectionContractViolation)?;
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(IndexError::ProjectionContractViolation);
    }
    Ok(values.into_iter().collect())
}

#[cfg(test)]
#[derive(Deserialize)]
struct V4PlanWave {
    obligation_ids: Vec<StableId>,
    wave_index: u32,
}

#[cfg(test)]
fn v4_plan_contains_all(input: &str, selected: &BTreeSet<StableId>) -> Result<bool, IndexError> {
    let waves: Vec<V4PlanWave> =
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

fn v4_marker_fingerprint(marker: &IndexMarkerV4) -> Result<[u8; 32], IndexError> {
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

fn v4_sqlite_limits(mut limits: IndexLimits) -> Result<IndexLimits, IndexError> {
    limits.max_working_bytes = limits
        .max_serialized_bytes
        .checked_mul(3)
        .and_then(|value| value.checked_add(limits.max_working_bytes))
        .and_then(|value| value.checked_add(1024))
        .ok_or(IndexError::InvalidLimits)?;
    Ok(limits)
}

struct ProjectedV4 {
    snapshot: IndexSnapshotV4,
    accounting: IndexAccountingV4,
}

trait ProjectionSourceV4 {
    fn initial(&self) -> Result<&reviewgraphen_core::ReviewAggregate, IndexError>;
    fn current(&self) -> Result<&reviewgraphen_core::ReviewAggregate, IndexError>;
    fn envelopes(
        &self,
    ) -> Result<impl ExactSizeIterator<Item = &reviewgraphen_core::EventEnvelope>, IndexError>;
    fn claim_assessments(
        &self,
    ) -> Result<impl Iterator<Item = &reviewgraphen_core::ClaimAssessmentV3>, IndexError>;
    fn confirmed_offset(&self) -> Result<u64, IndexError>;
}

impl ProjectionSourceV4 for crate::ReplayedV3RunSession<'_, '_> {
    fn initial(&self) -> Result<&reviewgraphen_core::ReviewAggregate, IndexError> {
        Ok(self.index_v4_initial()?)
    }

    fn current(&self) -> Result<&reviewgraphen_core::ReviewAggregate, IndexError> {
        Ok(self.index_v4_current()?)
    }

    fn envelopes(
        &self,
    ) -> Result<impl ExactSizeIterator<Item = &reviewgraphen_core::EventEnvelope>, IndexError> {
        Ok(self.index_v4_envelopes()?)
    }

    fn claim_assessments(
        &self,
    ) -> Result<impl Iterator<Item = &reviewgraphen_core::ClaimAssessmentV3>, IndexError> {
        Ok(self.index_v4_claim_assessments()?)
    }

    fn confirmed_offset(&self) -> Result<u64, IndexError> {
        Ok(self.index_v4_confirmed_offset()?)
    }
}

impl ProjectionSourceV4 for IndexV4ReplayedPrefix {
    fn initial(&self) -> Result<&reviewgraphen_core::ReviewAggregate, IndexError> {
        Ok(self.initial())
    }

    fn current(&self) -> Result<&reviewgraphen_core::ReviewAggregate, IndexError> {
        Ok(self.current())
    }

    fn envelopes(
        &self,
    ) -> Result<impl ExactSizeIterator<Item = &reviewgraphen_core::EventEnvelope>, IndexError> {
        Ok(self.envelopes())
    }

    fn claim_assessments(
        &self,
    ) -> Result<impl Iterator<Item = &reviewgraphen_core::ClaimAssessmentV3>, IndexError> {
        Ok(self.claim_assessments())
    }

    fn confirmed_offset(&self) -> Result<u64, IndexError> {
        Ok(self.confirmed_offset())
    }
}

fn project_verified_source<S: ProjectionSourceV4>(
    session: &S,
    basis: &AuthorityReplayBasisV3,
    limits: IndexLimits,
) -> Result<ProjectedV4, IndexError> {
    let initial = session.initial()?;
    let current = session.current()?;
    let event_count =
        u64::try_from(session.envelopes()?.len()).map_err(|_| IndexError::IntegerOutOfRange)?;
    if event_count != basis.confirmed_event_count() {
        return Err(IndexError::ProjectionContractViolation);
    }
    // Count every physical result row from the verified typed source before
    // reserving any public snapshot vector. This pass intentionally decodes
    // no raw JSON outside Core's bounded event projection API.
    let mut rows = 1_u64;
    for count in [
        initial.program().artifacts().len(),
        initial.program().relations().len(),
        1,
        initial.obligations().count(),
    ] {
        rows = rows
            .checked_add(u64::try_from(count).map_err(|_| IndexError::IntegerOutOfRange)?)
            .ok_or(IndexError::IntegerOutOfRange)?;
    }
    for envelope in session.envelopes()? {
        record_projection_decode();
        rows = rows.checked_add(1).ok_or(IndexError::IntegerOutOfRange)?;
        let decoded = envelope
            .decode_for_streaming_projection()
            .map_err(|_| IndexError::ProjectionContractViolation)?;
        let added = match decoded.payload() {
            DecodedPayload::RunGenesisManifestV3(_)
            | DecodedPayload::ArtifactRegisteredV3(_)
            | DecodedPayload::ObligationTransition { .. }
            | DecodedPayload::ReviewPlanRecorded(_)
            | DecodedPayload::ContextEnvelopeProjected(_)
            | DecodedPayload::EvidenceRecordedV3(_)
            | DecodedPayload::EvidenceBoundV3(_)
            | DecodedPayload::VerificationRecordedV3(_)
            | DecodedPayload::DecisionRecordedV3(_)
            | DecodedPayload::FindingRecordedV3(_) => 1_u64,
            DecodedPayload::SnapshotSourcesRecorded(value) => {
                u64::try_from(value.entries().len()).map_err(|_| IndexError::IntegerOutOfRange)?
            }
            DecodedPayload::ReviewExecutionRecorded { claims, .. } => u64::try_from(claims.len())
                .map_err(|_| IndexError::IntegerOutOfRange)?
                .checked_add(1)
                .ok_or(IndexError::IntegerOutOfRange)?,
            _ => return Err(IndexError::ProjectionContractViolation),
        };
        rows = rows
            .checked_add(added)
            .ok_or(IndexError::IntegerOutOfRange)?;
    }
    rows = rows
        .checked_add(
            u64::try_from(session.claim_assessments()?.count())
                .map_err(|_| IndexError::IntegerOutOfRange)?,
        )
        .ok_or(IndexError::IntegerOutOfRange)?;
    if rows > limits.max_rows {
        return Err(IndexError::Incomplete {
            limit: limits.max_rows,
            observed: rows,
        });
    }
    let marker = IndexMarkerV4 {
        index_schema_version: 4,
        sqlite_user_version: 4,
        projection_contract_version: PROJECTION_CONTRACT_VERSION_V4.to_owned(),
        event_contract_version: EVENT_CONTRACT_VERSION_V3.to_owned(),
        projection_mode: PROJECTION_MODE_V3.to_owned(),
        run_id: basis.run_id().clone(),
        genesis_hash: basis.genesis_hash().clone(),
        confirmed_offset: session.confirmed_offset()?,
        tail_hash: basis.confirmed_tail_hash().clone(),
        event_count,
        policy_revision_hash: basis.policy_revision_hash().clone(),
        authority_replay_basis_digest: basis.basis_digest().clone(),
    };
    // Run the complete zero-based accounting pass before any public result
    // vector is created. The pass materializes at most one projected row at a
    // time and retains no complete snapshot or canonical snapshot buffer.
    let preflight =
        preflight_verified_projection(session, initial, current, &marker, rows, limits)?;
    let accounting = preflight.accounting;
    let mut snapshot = IndexSnapshotV4 {
        marker,
        events: reserved_vec(preflight.array_counts[EVENTS])?,
        shadows: reserved_vec(preflight.array_counts[1])?,
        projected_findings: reserved_vec(preflight.array_counts[2])?,
        program_objects: reserved_vec(preflight.array_counts[PROGRAM_OBJECTS])?,
        program_relations: reserved_vec(preflight.array_counts[PROGRAM_RELATIONS])?,
        universe: None,
        obligations: reserved_vec(preflight.array_counts[OBLIGATIONS])?,
        obligation_lifecycle: reserved_vec(preflight.array_counts[OBLIGATION_LIFECYCLE])?,
        executions: reserved_vec(preflight.array_counts[EXECUTIONS])?,
        claims: reserved_vec(preflight.array_counts[CLAIMS])?,
        artifact_registrations: reserved_vec(preflight.array_counts[REGISTRATIONS])?,
        snapshot_sources: reserved_vec(preflight.array_counts[SNAPSHOT_SOURCES])?,
        context_envelopes: reserved_vec(preflight.array_counts[CONTEXT_ENVELOPES])?,
        review_plans: reserved_vec(preflight.array_counts[REVIEW_PLANS])?,
        evidence: reserved_vec(preflight.array_counts[EVIDENCE])?,
        evidence_bindings: reserved_vec(preflight.array_counts[EVIDENCE_BINDINGS])?,
        verifications: reserved_vec(preflight.array_counts[VERIFICATIONS])?,
        decisions: reserved_vec(preflight.array_counts[DECISIONS])?,
        findings: reserved_vec(preflight.array_counts[FINDINGS])?,
        claim_assessments: reserved_vec(preflight.array_counts[CLAIM_ASSESSMENTS])?,
        policy_revision_hash: basis.policy_revision_hash().clone(),
        authority_replay_basis_digest: basis.basis_digest().clone(),
    };
    for artifact in initial.program().artifacts() {
        snapshot.program_objects.push(IndexProgramObject {
            object_id: artifact.id.clone(),
            object_kind: artifact.kind.clone(),
            body_hash: super::body_hash(artifact)?,
        });
    }
    for relation in initial.program().relations() {
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
    for obligation in current.obligations() {
        record_projection_obligation_probe();
        snapshot.obligations.push(IndexObligation {
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
    for envelope in session.envelopes()? {
        record_projection_decode();
        let decoded = envelope
            .decode_for_streaming_projection()
            .map_err(|_| IndexError::ProjectionContractViolation)?;
        let kind = payload_kind_v4(decoded.payload())?;
        snapshot.events.push(IndexEvent {
            sequence: envelope.sequence(),
            event_id: envelope.id().clone(),
            schema: EVENT_CONTRACT_VERSION_V3.to_owned(),
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
    for assessment in session.claim_assessments()? {
        let object = canonical_object(assessment)?;
        let claim_id = id(&object, "claim_id")?;
        let confirmed_event_sequence = event_count;
        snapshot.claim_assessments.push(IndexClaimAssessmentV3 {
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
            confirmed_event_sequence,
        });
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
    record_v4_full_snapshot_construction();
    Ok(ProjectedV4 {
        snapshot,
        accounting,
    })
}

#[cfg(test)]
pub(crate) fn preflight_accounting_limits_for_test(
    session: &crate::ReplayedV3RunSession<'_, '_>,
    basis: &AuthorityReplayBasisV3,
    max_query_bytes: u64,
    max_working_bytes: u64,
) -> Result<(IndexAccountingV4, IndexAccountingV4), IndexError> {
    let limits = IndexLimits {
        max_rows: u64::MAX,
        max_serialized_bytes: u64::MAX,
        max_working_bytes,
        max_query_bytes,
        max_statement_bytes: u64::MAX,
    };
    let projected = project_verified_source(session, basis, limits)?;
    let max_event_line_bytes = session
        .index_v4_envelopes()?
        .map(|envelope| {
            usize::try_from(json_length(envelope)?)
                .map_err(|_| IndexError::IntegerOutOfRange)?
                .checked_add(1)
                .and_then(|value| u64::try_from(value).ok())
                .ok_or(IndexError::IntegerOutOfRange)
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .max()
        .unwrap_or(0);
    let oracle = account_snapshot(&projected.snapshot, max_event_line_bytes, limits)?;
    Ok((projected.accounting, oracle))
}

const V4_ARRAY_FIELDS: [&str; 19] = [
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
];
const EVENTS: usize = 0;
const PROGRAM_OBJECTS: usize = 3;
const PROGRAM_RELATIONS: usize = 4;
const OBLIGATIONS: usize = 5;
const OBLIGATION_LIFECYCLE: usize = 6;
const EXECUTIONS: usize = 7;
const CLAIMS: usize = 8;
const REGISTRATIONS: usize = 9;
const SNAPSHOT_SOURCES: usize = 10;
const CONTEXT_ENVELOPES: usize = 11;
const REVIEW_PLANS: usize = 12;
const EVIDENCE: usize = 13;
const EVIDENCE_BINDINGS: usize = 14;
const VERIFICATIONS: usize = 15;
const DECISIONS: usize = 16;
const FINDINGS: usize = 17;
const CLAIM_ASSESSMENTS: usize = 18;

#[derive(Clone, Copy, Default)]
struct ArrayChargeV4 {
    items: u64,
    json_items: u64,
    owned_items: u64,
}

struct ProjectionChargeV4 {
    arrays: [ArrayChargeV4; V4_ARRAY_FIELDS.len()],
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

struct ProjectionPreflightV4 {
    accounting: IndexAccountingV4,
    array_counts: [u64; V4_ARRAY_FIELDS.len()],
}

fn reserved_vec<T>(count: u64) -> Result<Vec<T>, IndexError> {
    let count = usize::try_from(count).map_err(|_| IndexError::IntegerOutOfRange)?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| IndexError::IntegerOutOfRange)?;
    Ok(result)
}

impl ProjectionChargeV4 {
    fn new(marker: &IndexMarkerV4, limits: IndexLimits) -> Result<Self, IndexError> {
        let marker_json = json_length(marker)?;
        let marker_owned = recursive_ownership_charge(marker)?;
        if marker_json > limits.max_query_bytes {
            return Err(IndexError::Incomplete {
                limit: limits.max_query_bytes,
                observed: marker_json,
            });
        }
        let mut result = Self {
            arrays: [ArrayChargeV4::default(); V4_ARRAY_FIELDS.len()],
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
        marker: &IndexMarkerV4,
        expected_rows: u64,
        limits: IndexLimits,
    ) -> Result<ProjectionPreflightV4, IndexError> {
        if self.rows != expected_rows {
            return Err(IndexError::ProjectionContractViolation);
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

        let policy = json_length(&marker.policy_revision_hash)?;
        let basis = json_length(&marker.authority_replay_basis_digest)?;
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
        let field_count = u64::try_from(V4_ARRAY_FIELDS.len() + SCALAR_FIELDS.len())
            .map_err(|_| IndexError::IntegerOutOfRange)?;
        let mut key_bytes = 0_u64;
        for key in V4_ARRAY_FIELDS.iter().chain(SCALAR_FIELDS.iter()) {
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
            marker.policy_revision_hash.as_str().len(),
            marker.authority_replay_basis_digest.as_str().len(),
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
        Ok(ProjectionPreflightV4 {
            accounting: IndexAccountingV4 {
                rows: self.rows,
                integer_cells: self.integer_cells,
                text_bytes: self.text_bytes,
                sql_bytes,
                query_bytes,
                owned_bytes,
                working_bytes,
            },
            array_counts,
        })
    }
}

fn recursive_ownership_charge<T: Serialize>(value: &T) -> Result<u64, IndexError> {
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

fn json_length<T: Serialize>(value: &T) -> Result<u64, IndexError> {
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

fn preflight_verified_projection<S: ProjectionSourceV4>(
    session: &S,
    initial: &reviewgraphen_core::ReviewAggregate,
    current: &reviewgraphen_core::ReviewAggregate,
    marker: &IndexMarkerV4,
    expected_rows: u64,
    limits: IndexLimits,
) -> Result<ProjectionPreflightV4, IndexError> {
    let mut charge = ProjectionChargeV4::new(marker, limits)?;
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
                schema: EVENT_CONTRACT_VERSION_V3.to_owned(),
                event_hash: envelope.event_hash().clone(),
                payload_hash: envelope.payload_hash().clone(),
                payload_kind: payload_kind_v4(decoded.payload())?.to_owned(),
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
            &IndexClaimAssessmentV3 {
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

fn payload_kind_v4(payload: &DecodedPayload) -> Result<&'static str, IndexError> {
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
fn body_hash<T: Serialize>(value: &T) -> Result<ContentHash, IndexError> {
    super::body_hash(value)
}

fn project_registration(
    envelope: &reviewgraphen_core::EventEnvelope,
    value: &reviewgraphen_core::ArtifactRegisteredV3,
) -> Result<IndexArtifactRegistrationV4, IndexError> {
    let source_kind = source_kind_and_run_id(value.source()).0.to_owned();
    let source_canonical_json = String::from_utf8(
        canonical_json(value.source()).map_err(|_| IndexError::ProjectionContractViolation)?,
    )
    .map_err(|_| IndexError::ProjectionContractViolation)?;
    Ok(IndexArtifactRegistrationV4 {
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
        source: value.source().clone(),
        body_hash: body_hash(value)?,
    })
}

fn project_evidence(
    envelope: &reviewgraphen_core::EventEnvelope,
    value: &reviewgraphen_core::EvidenceV3,
) -> Result<IndexEvidenceV3, IndexError> {
    let o = canonical_object(value)?;
    Ok(IndexEvidenceV3 {
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
fn project_binding(
    envelope: &reviewgraphen_core::EventEnvelope,
    value: &reviewgraphen_core::EvidenceBindingV3,
) -> Result<IndexEvidenceBindingV3, IndexError> {
    let o = canonical_object(value)?;
    Ok(IndexEvidenceBindingV3 {
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
fn project_verification(
    envelope: &reviewgraphen_core::EventEnvelope,
    value: &reviewgraphen_core::VerificationV3,
) -> Result<IndexVerificationV3, IndexError> {
    let o = canonical_object(value)?;
    Ok(IndexVerificationV3 {
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
fn project_decision(
    envelope: &reviewgraphen_core::EventEnvelope,
    value: &reviewgraphen_core::DecisionV3,
) -> Result<IndexDecisionV3, IndexError> {
    let o = canonical_object(value)?;
    Ok(IndexDecisionV3 {
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
fn project_finding(
    envelope: &reviewgraphen_core::EventEnvelope,
    value: &reviewgraphen_core::FindingV3,
) -> Result<IndexFindingV3, IndexError> {
    let o = canonical_object(value)?;
    Ok(IndexFindingV3 {
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

fn account_snapshot(
    snapshot: &IndexSnapshotV4,
    max_event_line_bytes: u64,
    limits: IndexLimits,
) -> Result<IndexAccountingV4, IndexError> {
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
    Ok(IndexAccountingV4 {
        rows,
        integer_cells,
        text_bytes,
        sql_bytes,
        query_bytes,
        owned_bytes,
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
    snapshot: &IndexSnapshotV4,
    limits: IndexLimits,
    retained: u64,
) -> Result<rusqlite::Connection, IndexError> {
    super::preflight_build_connection(limits, retained)?;
    let connection = rusqlite::Connection::open_in_memory_with_flags(
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
            | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
            | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    super::configure_connection(&connection, limits, retained)?;
    super::checked_batch(&connection, SCHEMA_V4, limits)?;
    connection.pragma_update(None, "user_version", INDEX_SCHEMA_VERSION_V4)?;
    let tx = connection.unchecked_transaction()?;
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

fn insert_snapshot(tx: &rusqlite::Transaction<'_>, s: &IndexSnapshotV4) -> Result<(), IndexError> {
    use rusqlite::params;
    let m = &s.marker;
    tx.execute("INSERT INTO index_meta VALUES(1,4,?1,'reviewgraphen.review_event.v3','v3_authority',?2,?3,?4,?5,?6,?7,?8)", params![m.projection_contract_version,m.run_id.to_string(),m.genesis_hash.to_string(),super::to_i64(m.confirmed_offset)?,m.tail_hash.to_string(),super::to_i64(m.event_count)?,m.policy_revision_hash.to_string(),m.authority_replay_basis_digest.to_string()]).map_err(super::map_sql)?;
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
    Ok(())
}

impl DerivedIndexV4<'_> {
    fn publish_v4_image_locked(
        &self,
        image: Vec<u8>,
        snapshot: &IndexSnapshotV4,
        accounting: &IndexAccountingV4,
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
            return Err(super::test_publish_failure("v4 after candidate write"));
        }
        file.sync_all()?;
        #[cfg(test)]
        if self
            .inner
            .take_publish_fault(super::PublishFault::BeforeCandidateSync)
        {
            return Err(super::test_publish_failure(
                "v4 after candidate file sync before link",
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
                "v4 after candidate link before directory sync",
            ));
        }
        fs::fsync(&self.inner.indexes)?;
        #[cfg(test)]
        if self
            .inner
            .take_publish_fault(super::PublishFault::AfterCandidateSync)
        {
            return Err(super::test_publish_failure(
                "v4 after candidate directory sync",
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
                "v4 before candidate read allocation",
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
                "v4 after candidate inode check",
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
            return Err(super::test_publish_failure("v4 after candidate hash check"));
        }
        validate_v4_image(
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
            return Err(super::test_publish_failure("v4 after candidate validation"));
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
                return Err(super::test_publish_failure("v4 after active inode check"));
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
                return Err(super::test_publish_failure("v4 after active hash check"));
            }
            #[cfg(test)]
            if self
                .inner
                .take_publish_fault(super::PublishFault::AfterActiveVerify)
            {
                return Err(super::test_publish_failure("v4 after active verification"));
            }
            #[cfg(test)]
            if self
                .inner
                .take_publish_fault(super::PublishFault::BeforeFinalDirectorySync)
            {
                return Err(super::test_publish_failure(
                    "v4 before final directory sync",
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

fn marker_v4_from_connection(
    connection: &rusqlite::Connection,
) -> Result<IndexMarkerV4, IndexError> {
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
    let marker = IndexMarkerV4 {
        index_schema_version: u64::try_from(raw.0).map_err(|_| IndexError::CorruptIndex)?,
        sqlite_user_version: INDEX_SCHEMA_VERSION_V4.into(),
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
    if marker.index_schema_version != u64::from(INDEX_SCHEMA_VERSION_V4)
        || marker.projection_contract_version != PROJECTION_CONTRACT_VERSION_V4
        || marker.event_contract_version != EVENT_CONTRACT_VERSION_V3
        || marker.projection_mode != PROJECTION_MODE_V3
    {
        return Err(IndexError::CorruptIndex);
    }
    Ok(marker)
}

fn validate_v4_image(
    bytes: Vec<u8>,
    expected: &IndexSnapshotV4,
    limits: IndexLimits,
    retained: u64,
) -> Result<(), IndexError> {
    let actual_hash = ContentHash::sha256(&bytes);
    let connection =
        super::deserialize_read_only_for_schema(bytes, limits, retained, INDEX_SCHEMA_VERSION_V4)
            .map_err(super::normalize_external_image_error)?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != 4 {
        return Err(IndexError::CorruptIndex);
    }
    validate_v4_structure(&connection, limits)?;
    let marker: (String,String,String,String) = connection.query_row(
        "SELECT projection_contract_version,event_contract_version,projection_mode,authority_replay_basis_digest FROM index_meta WHERE singleton=1",
        [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
    ).map_err(|_| IndexError::CorruptIndex)?;
    if marker
        != (
            PROJECTION_CONTRACT_VERSION_V4.to_owned(),
            EVENT_CONTRACT_VERSION_V3.to_owned(),
            PROJECTION_MODE_V3.to_owned(),
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
    validate_v4_rows(&connection, expected, limits)?;
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

enum ExpectedCellV4<'a> {
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
        source: &'a ArtifactSourceV3,
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

fn compare_v4_row(
    row: &Row<'_>,
    expected: &[ExpectedCellV4<'_>],
    limits: IndexLimits,
) -> Result<(), IndexError> {
    if row.as_ref().column_count() != expected.len() {
        return corrupt();
    }
    for (index, expected) in expected.iter().enumerate() {
        let actual = row.get_ref(index).map_err(|_| IndexError::CorruptIndex)?;
        match expected {
            ExpectedCellV4::U64(value) => {
                let ValueRef::Integer(actual) = actual else {
                    return corrupt();
                };
                if actual != super::to_i64(*value)? {
                    return corrupt();
                }
            }
            ExpectedCellV4::Bool(value) => {
                let ValueRef::Integer(actual) = actual else {
                    return corrupt();
                };
                if actual != i64::from(*value) {
                    return corrupt();
                }
            }
            ExpectedCellV4::Text(value) => compare_text(actual, value.as_bytes(), limits)?,
            ExpectedCellV4::OptionalText(value) => match value {
                Some(value) => compare_text(actual, value.as_bytes(), limits)?,
                None if matches!(actual, ValueRef::Null) => {}
                None => return corrupt(),
            },
            ExpectedCellV4::Id(value) => {
                let value = value.to_string();
                compare_text(actual, value.as_bytes(), limits)?;
            }
            ExpectedCellV4::OptionalId(value) => match value {
                Some(value) => {
                    let value = value.to_string();
                    compare_text(actual, value.as_bytes(), limits)?;
                }
                None if matches!(actual, ValueRef::Null) => {}
                None => return corrupt(),
            },
            ExpectedCellV4::Hash(value) => {
                let value = value.to_string();
                compare_text(actual, value.as_bytes(), limits)?;
            }
            ExpectedCellV4::CanonicalJson(value) => {
                compare_text(actual, value.as_bytes(), limits)?;
                let parsed: Value =
                    serde_json::from_str(value).map_err(|_| IndexError::CorruptIndex)?;
                if canonical_json(&parsed).map_err(|_| IndexError::CorruptIndex)?
                    != value.as_bytes()
                {
                    return corrupt();
                }
            }
            ExpectedCellV4::ContextPolicy(value) => {
                compare_text(actual, value.as_bytes(), limits)?;
                let canonical = reviewgraphen_core::ContextPolicyV1::baseline()
                    .canonical_bytes()
                    .map_err(|_| IndexError::CorruptIndex)?;
                if canonical != value.as_bytes() {
                    return corrupt();
                }
            }
            ExpectedCellV4::ArtifactSource {
                canonical,
                source,
                source_kind,
                run_id,
            } => {
                compare_text(actual, canonical.as_bytes(), limits)?;
                let decoded: ArtifactSourceV3 =
                    serde_json::from_str(canonical).map_err(|_| IndexError::CorruptIndex)?;
                if &decoded != *source
                    || canonical_json(&decoded).map_err(|_| IndexError::CorruptIndex)?
                        != canonical.as_bytes()
                {
                    return corrupt();
                }
                let (decoded_kind, decoded_run_id) = source_kind_and_run_id(&decoded);
                if decoded_kind != *source_kind || decoded_run_id != *run_id {
                    return corrupt();
                }
            }
        }
    }
    Ok(())
}

macro_rules! validate_v4_table {
    ($connection:expr, $limits:expr, $sql:literal, $rows:expr, |$row:ident| $cells:expr) => {{
        let mut statement = $connection
            .prepare($sql)
            .map_err(|_| IndexError::CorruptIndex)?;
        let mut actual = statement.query([]).map_err(|_| IndexError::CorruptIndex)?;
        for $row in $rows {
            let Some(found) = actual.next().map_err(|_| IndexError::CorruptIndex)? else {
                return corrupt();
            };
            compare_v4_row(found, &$cells, $limits)?;
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

/// Reads every v4 cell through borrowed SQLite values. Candidate-controlled
/// text is length checked and byte-compared before JSON decoding allocates;
/// every canonical JSON cell is decoded and re-canonicalized, and artifact
/// provenance additionally proves its source-kind and same-run closure.
fn validate_v4_rows(
    connection: &rusqlite::Connection,
    s: &IndexSnapshotV4,
    limits: IndexLimits,
) -> Result<(), IndexError> {
    validate_v4_table!(
        connection,
        limits,
        "SELECT singleton,index_schema_version,projection_contract_version,event_contract_version,projection_mode,run_id,genesis_hash,confirmed_offset,tail_hash,event_count,policy_revision_hash,authority_replay_basis_digest FROM index_meta ORDER BY singleton",
        std::iter::once(&s.marker),
        |r| [
            ExpectedCellV4::U64(1),
            ExpectedCellV4::U64(r.index_schema_version),
            ExpectedCellV4::Text(&r.projection_contract_version),
            ExpectedCellV4::Text(&r.event_contract_version),
            ExpectedCellV4::Text(&r.projection_mode),
            ExpectedCellV4::Id(&r.run_id),
            ExpectedCellV4::Hash(&r.genesis_hash),
            ExpectedCellV4::U64(r.confirmed_offset),
            ExpectedCellV4::Hash(&r.tail_hash),
            ExpectedCellV4::U64(r.event_count),
            ExpectedCellV4::Hash(&r.policy_revision_hash),
            ExpectedCellV4::Hash(&r.authority_replay_basis_digest)
        ]
    );
    validate_v4_table!(
        connection,
        limits,
        "SELECT sequence,event_id,schema,event_hash,payload_hash,payload_kind,actor,logical_time FROM events ORDER BY sequence",
        &s.events,
        |r| [
            ExpectedCellV4::U64(r.sequence),
            ExpectedCellV4::Id(&r.event_id),
            ExpectedCellV4::Text(&r.schema),
            ExpectedCellV4::Hash(&r.event_hash),
            ExpectedCellV4::Hash(&r.payload_hash),
            ExpectedCellV4::Text(&r.payload_kind),
            ExpectedCellV4::Text(&r.actor),
            ExpectedCellV4::U64(r.logical_time)
        ]
    );
    validate_v4_table!(
        connection,
        limits,
        "SELECT object_id,object_kind,body_hash FROM program_objects ORDER BY object_id",
        &s.program_objects,
        |r| [
            ExpectedCellV4::Id(&r.object_id),
            ExpectedCellV4::Text(&r.object_kind),
            ExpectedCellV4::Hash(&r.body_hash)
        ]
    );
    validate_v4_table!(
        connection,
        limits,
        "SELECT relation_id,relation_kind,source_id,target_ids_canonical_json,body_hash FROM program_relations ORDER BY relation_id",
        &s.program_relations,
        |r| [
            ExpectedCellV4::Id(&r.relation_id),
            ExpectedCellV4::Text(&r.relation_kind),
            ExpectedCellV4::Id(&r.source_id),
            ExpectedCellV4::CanonicalJson(&r.target_ids_canonical_json),
            ExpectedCellV4::Hash(&r.body_hash)
        ]
    );
    validate_v4_table!(
        connection,
        limits,
        "SELECT singleton,universe_id,snapshot_id,profile_id,rule_set_hash,extractor_set_hash,policy_version,rule_pack_version,body_hash FROM universe ORDER BY singleton",
        s.universe.iter(),
        |r| [
            ExpectedCellV4::U64(1),
            ExpectedCellV4::Id(&r.universe_id),
            ExpectedCellV4::Id(&r.snapshot_id),
            ExpectedCellV4::Text(&r.profile_id),
            ExpectedCellV4::Hash(&r.rule_set_hash),
            ExpectedCellV4::Hash(&r.extractor_set_hash),
            ExpectedCellV4::Text(&r.policy_version),
            ExpectedCellV4::Text(&r.rule_pack_version),
            ExpectedCellV4::Hash(&r.body_hash)
        ]
    );
    validate_v4_table!(
        connection,
        limits,
        "SELECT obligation_id,target_kind,target_ids_canonical_json,property_id,lifecycle,body_hash FROM obligations ORDER BY obligation_id",
        &s.obligations,
        |r| [
            ExpectedCellV4::Id(&r.obligation_id),
            ExpectedCellV4::Text(&r.target_kind),
            ExpectedCellV4::CanonicalJson(&r.target_ids_canonical_json),
            ExpectedCellV4::Text(&r.property_id),
            ExpectedCellV4::Text(&r.lifecycle),
            ExpectedCellV4::Hash(&r.body_hash)
        ]
    );
    validate_v4_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,obligation_id,next_lifecycle FROM obligation_lifecycle ORDER BY event_sequence,obligation_id",
        &s.obligation_lifecycle,
        |r| [
            ExpectedCellV4::U64(r.event_sequence),
            ExpectedCellV4::Id(&r.event_id),
            ExpectedCellV4::Id(&r.obligation_id),
            ExpectedCellV4::Text(&r.next_lifecycle)
        ]
    );
    validate_v4_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,registration_id,run_id,cas_hash,media_type,size,sensitivity,source_kind,source_canonical_json,body_hash FROM artifact_registrations ORDER BY event_sequence,registration_id",
        &s.artifact_registrations,
        |r| [
            ExpectedCellV4::U64(r.event_sequence),
            ExpectedCellV4::Id(&r.event_id),
            ExpectedCellV4::Id(&r.registration_id),
            ExpectedCellV4::Id(&r.run_id),
            ExpectedCellV4::Hash(&r.cas_hash),
            ExpectedCellV4::Text(&r.media_type),
            ExpectedCellV4::U64(r.size),
            ExpectedCellV4::Text(&r.sensitivity),
            ExpectedCellV4::Text(&r.source_kind),
            ExpectedCellV4::ArtifactSource {
                canonical: &r.source_canonical_json,
                source: &r.source,
                source_kind: &r.source_kind,
                run_id: &r.run_id
            },
            ExpectedCellV4::Hash(&r.body_hash)
        ]
    );
    validate_v4_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,snapshot_id,artifact_id,registration_id,path,content_hash,cas_hash,line_count FROM snapshot_source_index ORDER BY event_sequence,artifact_id",
        &s.snapshot_sources,
        |r| [
            ExpectedCellV4::U64(r.event_sequence),
            ExpectedCellV4::Id(&r.event_id),
            ExpectedCellV4::Id(&r.snapshot_id),
            ExpectedCellV4::Id(&r.artifact_id),
            ExpectedCellV4::Id(&r.registration_id),
            ExpectedCellV4::Text(&r.path),
            ExpectedCellV4::Hash(&r.content_hash),
            ExpectedCellV4::Hash(&r.cas_hash),
            ExpectedCellV4::U64(r.line_count)
        ]
    );
    validate_v4_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,plan_id,universe_id,snapshot_id,planner_input_hash,planner_policy_version,planner_policy_hash,budget_canonical_json,budget_hash,risk_breakdown_canonical_json,waves_canonical_json,deferred_canonical_json,identity_body_hash,body_hash FROM review_plans ORDER BY event_sequence,plan_id",
        &s.review_plans,
        |r| [
            ExpectedCellV4::U64(r.event_sequence),
            ExpectedCellV4::Id(&r.event_id),
            ExpectedCellV4::Id(&r.plan_id),
            ExpectedCellV4::Id(&r.universe_id),
            ExpectedCellV4::Id(&r.snapshot_id),
            ExpectedCellV4::Hash(&r.planner_input_hash),
            ExpectedCellV4::Text(&r.planner_policy_version),
            ExpectedCellV4::Hash(&r.planner_policy_hash),
            ExpectedCellV4::CanonicalJson(&r.budget_canonical_json),
            ExpectedCellV4::Hash(&r.budget_hash),
            ExpectedCellV4::CanonicalJson(&r.risk_breakdown_canonical_json),
            ExpectedCellV4::CanonicalJson(&r.waves_canonical_json),
            ExpectedCellV4::CanonicalJson(&r.deferred_canonical_json),
            ExpectedCellV4::Hash(&r.identity_body_hash),
            ExpectedCellV4::Hash(&r.body_hash)
        ]
    );
    validate_v4_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,envelope_id,snapshot_id,context_policy_version,context_policy_hash,candidate_ids_canonical_json,obligation_ids_canonical_json,context_policy_canonical_json,included_sources_canonical_json,excluded_sources_canonical_json,unknowns_canonical_json,assumptions_canonical_json,losses_canonical_json,projection_hash,body_hash FROM context_envelopes ORDER BY event_sequence,envelope_id",
        &s.context_envelopes,
        |r| [
            ExpectedCellV4::U64(r.event_sequence),
            ExpectedCellV4::Id(&r.event_id),
            ExpectedCellV4::Id(&r.envelope_id),
            ExpectedCellV4::Id(&r.snapshot_id),
            ExpectedCellV4::Text(&r.context_policy_version),
            ExpectedCellV4::Hash(&r.context_policy_hash),
            ExpectedCellV4::CanonicalJson(&r.candidate_ids_canonical_json),
            ExpectedCellV4::CanonicalJson(&r.obligation_ids_canonical_json),
            ExpectedCellV4::ContextPolicy(&r.context_policy_canonical_json),
            ExpectedCellV4::CanonicalJson(&r.included_sources_canonical_json),
            ExpectedCellV4::CanonicalJson(&r.excluded_sources_canonical_json),
            ExpectedCellV4::CanonicalJson(&r.unknowns_canonical_json),
            ExpectedCellV4::CanonicalJson(&r.assumptions_canonical_json),
            ExpectedCellV4::CanonicalJson(&r.losses_canonical_json),
            ExpectedCellV4::Hash(&r.projection_hash),
            ExpectedCellV4::Hash(&r.body_hash)
        ]
    );
    validate_v4_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,execution_id,plan_id,wave_id,snapshot_id,envelope_id,obligation_ids_canonical_json,reviewer_kind,reviewer_id,provider,model,model_revision,system_prompt_version,prompt_template_version,inference_settings_canonical_json,tool_policy_version,tool_calls_canonical_json,attempt,raw_registration_id,raw_hash,parsed_claim_ids_canonical_json,outcome_kind,outcome_canonical_json,identity_body_hash,body_hash FROM executions ORDER BY event_sequence,execution_id",
        &s.executions,
        |r| [
            ExpectedCellV4::U64(r.event_sequence),
            ExpectedCellV4::Id(&r.event_id),
            ExpectedCellV4::Id(&r.execution_id),
            ExpectedCellV4::Id(&r.plan_id),
            ExpectedCellV4::Id(&r.wave_id),
            ExpectedCellV4::Id(&r.snapshot_id),
            ExpectedCellV4::Id(&r.envelope_id),
            ExpectedCellV4::CanonicalJson(&r.obligation_ids_canonical_json),
            ExpectedCellV4::Text(&r.reviewer_kind),
            ExpectedCellV4::Text(&r.reviewer_id),
            ExpectedCellV4::OptionalText(r.provider.as_deref()),
            ExpectedCellV4::OptionalText(r.model.as_deref()),
            ExpectedCellV4::OptionalText(r.model_revision.as_deref()),
            ExpectedCellV4::Text(&r.system_prompt_version),
            ExpectedCellV4::Text(&r.prompt_template_version),
            ExpectedCellV4::CanonicalJson(&r.inference_settings_canonical_json),
            ExpectedCellV4::Text(&r.tool_policy_version),
            ExpectedCellV4::CanonicalJson(&r.tool_calls_canonical_json),
            ExpectedCellV4::U64(u64::from(r.attempt)),
            ExpectedCellV4::Id(&r.raw_registration_id),
            ExpectedCellV4::Hash(&r.raw_hash),
            ExpectedCellV4::CanonicalJson(&r.parsed_claim_ids_canonical_json),
            ExpectedCellV4::Text(&r.outcome_kind),
            ExpectedCellV4::CanonicalJson(&r.outcome_canonical_json),
            ExpectedCellV4::Hash(&r.identity_body_hash),
            ExpectedCellV4::Hash(&r.body_hash)
        ]
    );
    validate_v4_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,claim_id,execution_id,obligation_ids_canonical_json,property_id,target_refs_canonical_json,polarity,disposition,summary,source_ids_canonical_json,assumptions_canonical_json,requested_evidence_canonical_json,candidate_confidence_canonical_json,author_kind,review_status,identity_body_hash,body_hash FROM claims ORDER BY event_sequence,claim_id",
        &s.claims,
        |r| [
            ExpectedCellV4::U64(r.event_sequence),
            ExpectedCellV4::Id(&r.event_id),
            ExpectedCellV4::Id(&r.claim_id),
            ExpectedCellV4::Id(&r.execution_id),
            ExpectedCellV4::CanonicalJson(&r.obligation_ids_canonical_json),
            ExpectedCellV4::Text(&r.property_id),
            ExpectedCellV4::CanonicalJson(&r.target_refs_canonical_json),
            ExpectedCellV4::Text(&r.polarity),
            ExpectedCellV4::Text(&r.disposition),
            ExpectedCellV4::Text(&r.summary),
            ExpectedCellV4::CanonicalJson(&r.source_ids_canonical_json),
            ExpectedCellV4::CanonicalJson(&r.assumptions_canonical_json),
            ExpectedCellV4::CanonicalJson(&r.requested_evidence_canonical_json),
            ExpectedCellV4::CanonicalJson(&r.candidate_confidence_canonical_json),
            ExpectedCellV4::Text(&r.author_kind),
            ExpectedCellV4::Text(&r.review_status),
            ExpectedCellV4::Hash(&r.identity_body_hash),
            ExpectedCellV4::Hash(&r.body_hash)
        ]
    );
    validate_v4_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,record_id,kind,body_hash,authority_reconciled FROM unreconciled_authority_records ORDER BY event_sequence,record_id",
        &s.shadows,
        |r| [
            ExpectedCellV4::U64(r.event_sequence),
            ExpectedCellV4::Id(&r.event_id),
            ExpectedCellV4::Id(&r.record_id),
            ExpectedCellV4::Text(&r.kind),
            ExpectedCellV4::Hash(&r.body_hash),
            ExpectedCellV4::Bool(false)
        ]
    );
    validate_v4_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,finding_id,body_hash,projection_status FROM projected_findings ORDER BY event_sequence,finding_id",
        &s.projected_findings,
        |r| [
            ExpectedCellV4::U64(r.event_sequence),
            ExpectedCellV4::Id(&r.event_id),
            ExpectedCellV4::Id(&r.finding_id),
            ExpectedCellV4::Hash(&r.body_hash),
            ExpectedCellV4::Text(&r.projection_status)
        ]
    );
    validate_v4_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,evidence_id,schema,kind,snapshot_id,subject_ids_canonical_json,descriptor_id,procedure_version,input_registration_id,output_registration_id,observation,body_hash FROM evidence_v3 ORDER BY event_sequence,evidence_id",
        &s.evidence,
        |r| [
            ExpectedCellV4::U64(r.event_sequence),
            ExpectedCellV4::Id(&r.event_id),
            ExpectedCellV4::Id(&r.evidence_id),
            ExpectedCellV4::Text(&r.schema),
            ExpectedCellV4::Text(&r.kind),
            ExpectedCellV4::Id(&r.snapshot_id),
            ExpectedCellV4::CanonicalJson(&r.subject_ids_canonical_json),
            ExpectedCellV4::Text(&r.descriptor_id),
            ExpectedCellV4::Text(&r.procedure_version),
            ExpectedCellV4::Id(&r.input_registration_id),
            ExpectedCellV4::Id(&r.output_registration_id),
            ExpectedCellV4::Text(&r.observation),
            ExpectedCellV4::Hash(&r.body_hash)
        ]
    );
    validate_v4_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,binding_id,schema,claim_id,evidence_id,relation,property_id,body_hash FROM evidence_bindings_v3 ORDER BY event_sequence,binding_id",
        &s.evidence_bindings,
        |r| [
            ExpectedCellV4::U64(r.event_sequence),
            ExpectedCellV4::Id(&r.event_id),
            ExpectedCellV4::Id(&r.binding_id),
            ExpectedCellV4::Text(&r.schema),
            ExpectedCellV4::Id(&r.claim_id),
            ExpectedCellV4::Id(&r.evidence_id),
            ExpectedCellV4::Text(&r.relation),
            ExpectedCellV4::Text(&r.property_id),
            ExpectedCellV4::Hash(&r.body_hash)
        ]
    );
    validate_v4_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,verification_id,schema,claim_id,descriptor_id,procedure_version,input_registration_id,output_registration_id,evidence_ids_canonical_json,outcome,limitations_canonical_json,body_hash FROM verifications_v3 ORDER BY event_sequence,verification_id",
        &s.verifications,
        |r| [
            ExpectedCellV4::U64(r.event_sequence),
            ExpectedCellV4::Id(&r.event_id),
            ExpectedCellV4::Id(&r.verification_id),
            ExpectedCellV4::Text(&r.schema),
            ExpectedCellV4::Id(&r.claim_id),
            ExpectedCellV4::Text(&r.descriptor_id),
            ExpectedCellV4::Text(&r.procedure_version),
            ExpectedCellV4::Id(&r.input_registration_id),
            ExpectedCellV4::Id(&r.output_registration_id),
            ExpectedCellV4::CanonicalJson(&r.evidence_ids_canonical_json),
            ExpectedCellV4::Text(&r.outcome),
            ExpectedCellV4::CanonicalJson(&r.limitations_canonical_json),
            ExpectedCellV4::Hash(&r.body_hash)
        ]
    );
    validate_v4_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,decision_id,schema,policy_revision_hash,run_id,universe_id,claim_id,property_id,outcome,actor,authority_id,snapshot_id,source_ids_canonical_json,rationale,issued_at,expires_at,body_hash FROM decisions_v3 ORDER BY event_sequence,decision_id",
        &s.decisions,
        |r| [
            ExpectedCellV4::U64(r.event_sequence),
            ExpectedCellV4::Id(&r.event_id),
            ExpectedCellV4::Id(&r.decision_id),
            ExpectedCellV4::Text(&r.schema),
            ExpectedCellV4::Hash(&r.policy_revision_hash),
            ExpectedCellV4::Id(&r.run_id),
            ExpectedCellV4::Id(&r.universe_id),
            ExpectedCellV4::Id(&r.claim_id),
            ExpectedCellV4::Text(&r.property_id),
            ExpectedCellV4::Text(&r.outcome),
            ExpectedCellV4::Text(&r.actor),
            ExpectedCellV4::Text(&r.authority_id),
            ExpectedCellV4::Id(&r.snapshot_id),
            ExpectedCellV4::CanonicalJson(&r.source_ids_canonical_json),
            ExpectedCellV4::Text(&r.rationale),
            ExpectedCellV4::Text(&r.issued_at),
            ExpectedCellV4::OptionalText(r.expires_at.as_deref()),
            ExpectedCellV4::Hash(&r.body_hash)
        ]
    );
    validate_v4_table!(
        connection,
        limits,
        "SELECT event_sequence,event_id,finding_id,schema,projection_descriptor_id,claim_id,status,evidence_ids_canonical_json,verification_ids_canonical_json,decision_id,supersedes_finding_id,body_hash FROM findings_v3 ORDER BY event_sequence,finding_id",
        &s.findings,
        |r| [
            ExpectedCellV4::U64(r.event_sequence),
            ExpectedCellV4::Id(&r.event_id),
            ExpectedCellV4::Id(&r.finding_id),
            ExpectedCellV4::Text(&r.schema),
            ExpectedCellV4::Text(&r.projection_descriptor_id),
            ExpectedCellV4::Id(&r.claim_id),
            ExpectedCellV4::Text(&r.status),
            ExpectedCellV4::CanonicalJson(&r.evidence_ids_canonical_json),
            ExpectedCellV4::CanonicalJson(&r.verification_ids_canonical_json),
            ExpectedCellV4::OptionalId(r.decision_id.as_ref()),
            ExpectedCellV4::OptionalId(r.supersedes_finding_id.as_ref()),
            ExpectedCellV4::Hash(&r.body_hash)
        ]
    );
    validate_v4_table!(
        connection,
        limits,
        "SELECT claim_id,disposition,review_status,binding_ids_canonical_json,evidence_ids_canonical_json,verification_ids_canonical_json,decision_ids_canonical_json,finding_ids_canonical_json,active_decision_id,current_finding_id,decision_conflict,confirmed_event_sequence FROM claim_assessments_v3 ORDER BY claim_id",
        &s.claim_assessments,
        |r| [
            ExpectedCellV4::Id(&r.claim_id),
            ExpectedCellV4::Text(&r.disposition),
            ExpectedCellV4::Text(&r.review_status),
            ExpectedCellV4::CanonicalJson(&r.binding_ids_canonical_json),
            ExpectedCellV4::CanonicalJson(&r.evidence_ids_canonical_json),
            ExpectedCellV4::CanonicalJson(&r.verification_ids_canonical_json),
            ExpectedCellV4::CanonicalJson(&r.decision_ids_canonical_json),
            ExpectedCellV4::CanonicalJson(&r.finding_ids_canonical_json),
            ExpectedCellV4::OptionalId(r.active_decision_id.as_ref()),
            ExpectedCellV4::OptionalId(r.current_finding_id.as_ref()),
            ExpectedCellV4::Bool(r.decision_conflict),
            ExpectedCellV4::U64(r.confirmed_event_sequence)
        ]
    );
    Ok(())
}

const V4_SCAN_TABLES: [(&str, &[usize]); 21] = [
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
fn preflight_v4_structure(
    connection: &rusqlite::Connection,
    limits: IndexLimits,
) -> Result<u64, IndexError> {
    let mut rows_seen = 0_u64;
    let mut text_seen = 0_u64;
    let mut integer_cells = 0_u64;
    let mut max_json_staging = 0_u64;
    for (sql, canonical_columns) in V4_SCAN_TABLES {
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
pub(crate) fn v4_json_staging_for_connection_for_test(
    connection: &rusqlite::Connection,
    limits: IndexLimits,
) -> Result<u64, IndexError> {
    preflight_v4_structure(connection, limits)
}

fn validate_v4_structure_after_preflight(
    connection: &rusqlite::Connection,
    limits: IndexLimits,
) -> Result<(), IndexError> {
    for (sql, canonical_columns) in V4_SCAN_TABLES {
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
                record_v4_json_decode_allocation();
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
        record_v4_json_decode_allocation();
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

fn validate_v4_structure(
    connection: &rusqlite::Connection,
    limits: IndexLimits,
) -> Result<(), IndexError> {
    let max_json_staging = preflight_v4_structure(connection, limits)?;
    admit_v4_json_staging(max_json_staging)?;
    validate_v4_structure_after_preflight(connection, limits)
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
    fn selection_marker_fingerprint_detects_tail_drift() {
        let mut first = empty_snapshot("selection-fingerprint").marker;
        let original = v4_marker_fingerprint(&first).unwrap();
        first.tail_hash = ContentHash::sha256(b"different confirmed tail");
        assert_ne!(v4_marker_fingerprint(&first).unwrap(), original);
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
        let assert_overflow = |charge: ProjectionChargeV4| {
            let rows = charge.rows;
            assert!(matches!(
                charge.finish(&marker, rows, limits),
                Err(IndexError::Incomplete {
                    observed: u64::MAX,
                    ..
                })
            ));
        };

        let mut sql = ProjectionChargeV4::new(&marker, limits).unwrap();
        sql.integer_cells = u64::MAX;
        assert_overflow(sql);

        let mut query = ProjectionChargeV4::new(&marker, limits).unwrap();
        query.arrays[EVENTS].json_items = u64::MAX;
        assert_overflow(query);

        let mut owned = ProjectionChargeV4::new(&marker, limits).unwrap();
        owned.marker_owned = u64::MAX;
        assert_overflow(owned);

        let mut working = ProjectionChargeV4::new(&marker, limits).unwrap();
        working.max_cas_bytes = u64::MAX;
        assert_overflow(working);
    }

    fn empty_snapshot(tag: &str) -> IndexSnapshotV4 {
        let policy = ContentHash::sha256(format!("policy:{tag}").as_bytes());
        let basis = ContentHash::sha256(format!("basis:{tag}").as_bytes());
        IndexSnapshotV4 {
            marker: IndexMarkerV4 {
                index_schema_version: 4,
                sqlite_user_version: 4,
                projection_contract_version: PROJECTION_CONTRACT_VERSION_V4.to_owned(),
                event_contract_version: EVENT_CONTRACT_VERSION_V3.to_owned(),
                projection_mode: PROJECTION_MODE_V3.to_owned(),
                run_id: StableId::parse(format!("run:v4-{tag}")).unwrap(),
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
            policy_revision_hash: policy,
            authority_replay_basis_digest: basis,
        }
    }

    #[test]
    fn schema_v4_literal_is_complete_strict_and_versioned() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection.execute_batch(SCHEMA_V4).unwrap();
        connection
            .pragma_update(None, "user_version", INDEX_SCHEMA_VERSION_V4)
            .unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 4);
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
                "claim_assessments_v3".to_owned(),
                "claims".to_owned(),
                "context_envelopes".to_owned(),
                "decisions_v3".to_owned(),
                "events".to_owned(),
                "evidence_bindings_v3".to_owned(),
                "evidence_v3".to_owned(),
                "executions".to_owned(),
                "findings_v3".to_owned(),
                "index_meta".to_owned(),
                "obligation_lifecycle".to_owned(),
                "obligations".to_owned(),
                "program_objects".to_owned(),
                "program_relations".to_owned(),
                "projected_findings".to_owned(),
                "review_plans".to_owned(),
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
        assert!(SCHEMA_V4.contains("reviewgraphen.index_projection.v4"));
        assert!(SCHEMA_V4.contains("reviewgraphen.review_event.v3"));
        assert!(SCHEMA_V4.contains("v3_authority"));
    }

    #[test]
    fn schema_v2_and_v3_images_are_rebuild_required_by_v4_reader() {
        let limits = IndexLimits::try_from(crate::StoreLimits::default()).unwrap();
        for found in [2_u32, 3] {
            let connection = rusqlite::Connection::open_in_memory().unwrap();
            super::super::configure_connection(&connection, limits, 0).unwrap();
            super::super::create_schema(&connection, limits).unwrap();
            connection
                .pragma_update(None, "user_version", found)
                .unwrap();
            let image = super::super::serialize_connection(&connection, limits).unwrap();
            assert!(matches!(
                super::super::deserialize_read_only_for_schema(image, limits, 0, 4),
                Err(IndexError::RebuildRequired {
                    found: actual,
                    required: 4
                }) if actual == found
            ));
        }
    }

    #[test]
    fn v4_full_readback_refuses_scalar_and_canonical_json_tamper() {
        let limits = IndexLimits::try_from(crate::StoreLimits::default()).unwrap();
        let mut snapshot = empty_snapshot("tamper");
        snapshot.program_relations.push(IndexProgramRelation {
            relation_id: StableId::parse("relation:v4-tamper").unwrap(),
            relation_kind: "calls".to_owned(),
            source_id: StableId::parse("node:v4-source").unwrap(),
            target_ids_canonical_json: "[]".to_owned(),
            body_hash: ContentHash::sha256(b"relation:v4-tamper"),
        });
        let connection = build_connection(&snapshot, limits, 0).unwrap();
        validate_v4_rows(&connection, &snapshot, limits).unwrap();
        connection.pragma_update(None, "query_only", false).unwrap();
        connection
            .execute("UPDATE index_meta SET run_id='run:v4-other'", [])
            .unwrap();
        assert!(matches!(
            validate_v4_rows(&connection, &snapshot, limits),
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
            validate_v4_structure(&connection, limits),
            Err(IndexError::CorruptIndex)
        ));
    }

    #[test]
    fn v4_publication_failpoints_preserve_or_uncertain_at_the_rename_boundary() {
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
            let index = DerivedIndexV4::open(&root).unwrap();
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
                .publish_v4_image_locked(
                    old_image.clone(),
                    &old_snapshot,
                    &old_accounting,
                    v4_sqlite_limits(index.inner.limits).unwrap(),
                )
                .unwrap();
            index.inject_publish_fault(fault);
            let result = index.publish_v4_image_locked(
                replacement_image.clone(),
                &replacement_snapshot,
                &replacement_accounting,
                v4_sqlite_limits(index.inner.limits).unwrap(),
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
