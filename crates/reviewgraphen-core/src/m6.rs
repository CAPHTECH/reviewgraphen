//! Pure deterministic M6 incremental-review domain contracts.
//!
//! This module owns no journal, authority capability, CAS handle, runtime
//! operation, report projection, or historical mutable state.  It constructs
//! only validated, versioned values described by ADR 0023.  In particular,
//! structural preservation is audit evidence and never becomes a native M4
//! verification, human decision, finding, gluing authority, or gate credit.

use crate::event::{
    HistoricalPrefixAdmissionV4, HistoricalPrefixProjectionV4, HistoricalSourceRecordKindV4,
    HistoricalSourceRecordProjectionV4, HistoricalSourceRecordValueV4,
    TargetActualRecordInventoryV5, TargetActualRecordProjectionV5, TargetActualRecordV5,
    TargetPredecessorSuppressionProjectionV5, TargetSuppressionRecordWitnessV5,
    V5PreIncrementalStructuralPrefixProjection,
};
use crate::{
    ArtifactSensitivity, ArtifactSourceV3, ArtifactSourceV4, AuthorityReplayBasisV4, ContentHash,
    DecisionOutcomeV3, DomainError, EventLogV4, EventLogV5, FindingStatusV3,
    M5CompletedGluingProfileV4, Obligation, ObligationLifecycle, ProgramSpace, ReviewAggregate,
    StableId, VerificationOutcomeV3,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use thiserror::Error;

pub const PROGRAM_MAPPING_POLICY_V5: &str = "reviewgraphen.program_mapping@1";
pub const RUST_SYMBOL_ANCHOR_V1: &str = "reviewgraphen.rust_symbol_anchor@1";
pub const GIT_CHANGE_PROVENANCE_V1: &str = "reviewgraphen.ingest.git.changed_structure.v1";
pub const OBLIGATION_CORRESPONDENCE_POLICY_V5: &str = "reviewgraphen.obligation_correspondence@1";
pub const MAX_M6_PROGRAM_DOMAIN_IDS: usize = 4_096;
pub const MAX_M6_MAPPINGS: usize = 8_192;
pub const MAX_M6_MAPPING_SIDE_IDS: usize = 64;
pub const MAX_M6_MAPPING_LINK_IDS: usize = 64;
pub const MAX_M6_CANONICAL_BYTES: usize = 1_048_576;
pub const MAX_M6_MAPPING_DTO_BYTES: usize = 1_048_576;
pub const MAX_M6_CLOSURE_DTO_BYTES: usize = 1_048_576;
pub const MAX_M6_MORPHISM_DTO_BYTES: usize = 1_048_576;
pub const MAX_M6_EVENT_LINE_BYTES: usize = 1_048_576;
pub const MAX_M6_MAPPING_WORKING_BYTES: usize = 536_870_912;
pub const MAX_M6_OBLIGATIONS_PER_UNIVERSE: usize = 2_048;
pub const MAX_M6_CORRESPONDENCE_ENTRIES: usize = 4_096;
pub const MAX_M6_CORRESPONDENCE_SIDE_IDS: usize = 64;
pub const MAX_M6_CORRESPONDENCE_PREDECESSOR_IDS: usize = 64;
pub const MAX_M6_CORRESPONDENCE_DTO_BYTES: usize = 1_048_576;
pub const MAX_M6_CORRESPONDENCE_WORKING_BYTES: usize = 536_870_912;
pub const MAX_M6_STALENESS_WORKING_BYTES: usize = 536_870_912;
pub const MAX_M6_RECORD_METADATA_IDS: usize = 512;
pub const MAX_M6_HISTORICAL_ASSESSMENTS: usize = 8_192;
pub const MAX_M6_GLUE_FRESHNESS_RECORDS: usize = 1;
pub const MAX_M6_RELATION_VISITS: usize = 65_536;
pub const MVP_PROPERTY_IMPACT_POLICY_V5: &str = "reviewgraphen.mvp_property_impact@1";
pub const STRUCTURAL_PRESERVATION_DESCRIPTOR_V5: &str = "reviewgraphen.structural_preservation@1";
pub const PAYMENT_PRESERVATION_PROCEDURE_V1: &str =
    "reviewgraphen.structural_preservation.payment_v1";
pub const PRESERVATION_INPUT_MEDIA_TYPE_V1: &str =
    "application/vnd.reviewgraphen.preservation-input+json;version=1";
pub const PRESERVATION_RESULT_MEDIA_TYPE_V1: &str =
    "application/vnd.reviewgraphen.preservation-result+json;version=1";
pub const MAX_M6_PRESERVATION_DEPENDENCY_MAPPINGS: usize = 512;
pub const MAX_M6_PRESERVATION_RECORDS: usize = 2_048;
pub const MAX_M6_PRESERVATION_REGISTRATIONS: usize = 4_096;
pub const MAX_M6_PRESERVATION_CAS_BYTES: usize = 1_048_576;
pub const PARTIAL_RERUN_PLANNER_DESCRIPTOR_V5: &str = "reviewgraphen.partial_rerun@1";
pub const MAX_M6_PARTIAL_RERUN_ACTIONS: usize = 4_096;
pub const MAX_M6_ACTION_METADATA_IDS: usize = 512;
pub const MAX_M6_PARTIAL_RERUN_WORKING_BYTES: usize = 536_870_912;
pub const M5_CLAIM_SELECTION_DESCRIPTOR_V5: &str = "reviewgraphen.m5_claim_selection@1";
pub const GLUING_RERUN_REASON_V5: &str = "fresh_target_gluing_required";
pub const MAX_M6_GLUING_RERUN_ACTIONS: usize = 5;
pub const MAX_M6_GLUING_PREREQUISITES: usize = 8;

pub type M6Result<T> = std::result::Result<T, M6Error>;

fn canonical_json_string_v6<T: Serialize>(value: &T) -> M6Result<String> {
    String::from_utf8(crate::canonical_json(value)?)
        .map_err(|error| M6Error::InvalidWire(error.to_string()))
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum M6Error {
    #[error("invalid M6 source/target closure: {0}")]
    InvalidSourceClosure(&'static str),
    #[error("invalid full lowercase SHA-256 in {field}: {value}")]
    InvalidFullSha256 { field: &'static str, value: String },
    #[error("invalid full lowercase Git object ID in {field}: {value}")]
    InvalidGitObjectId { field: &'static str, value: String },
    #[error("invalid M6 mapping component: {0}")]
    InvalidMapping(&'static str),
    #[error("invalid M6 obligation universe: {0}")]
    InvalidObligationUniverse(&'static str),
    #[error("invalid M6 historical staleness topology: {0}")]
    InvalidHistoricalTopology(&'static str),
    #[error("invalid M6 staleness assessment: {0}")]
    InvalidStalenessAssessment(&'static str),
    #[error("target preservation is unsupported: {0}")]
    PreservationUnsupported(&'static str),
    #[error("M6 reviewer execution {execution_id} has unsupported claim cardinality {observed}")]
    M6ClaimCardinalityUnsupported {
        execution_id: StableId,
        observed: usize,
    },
    #[error("completed target obligation {obligation_id} lacks its exact reviewer closure")]
    CompletedReviewerClosureMismatch { obligation_id: StableId },
    #[error("target D2 claim selection is ambiguous for {context_id}")]
    AmbiguousGluingClaimSelection { context_id: StableId },
    #[error("an existing target M5 bundle differs from the derived target plan")]
    ExistingTargetM5Mismatch,
    #[error("missing accepted M6 {kind} fact for {object_id}")]
    MissingAcceptedMappingFact {
        kind: &'static str,
        object_id: StableId,
    },
    #[error("{operation} exceeds limit {limit} (observed {observed})")]
    Incomplete {
        operation: &'static str,
        limit: usize,
        observed: usize,
    },
    #[error("canonical M6 construction failed: {0}")]
    Canonical(String),
    #[error("invalid M6 wire value: {0}")]
    InvalidWire(String),
}

#[cfg(test)]
mod preservation_contract_tests {
    use super::*;

    fn sid(kind: &str, value: &str) -> StableId {
        StableId::parse(format!("{kind}:{value}")).expect("test ID")
    }

    fn bundle() -> PreservationBundleV5 {
        let input = PreservationInputV1::new(PreservationInputParamsV1 {
            source_closure_id: sid("incremental-source-closure-v5", "source"),
            morphism_id: sid("change-morphism-v5", "morphism"),
            correspondence_entry_id: sid("obligation-correspondence-entry-v5", "entry"),
            source_claim_id: sid("claim", "source"),
            source_evidence_ids: BTreeSet::from([sid("evidence", "source")]),
            source_verification_id: sid("verification", "source"),
            target_snapshot_id: sid("snapshot", "target"),
            target_obligation_id: sid("obligation", "target"),
            dependency_mapping_ids: BTreeSet::from([sid("program-mapping-v5", "dependency")]),
            policy_revision_hash: ContentHash::sha256(b"policy"),
        })
        .expect("preservation input");
        PreservationBundleV5::build(sid("run", "target"), sid("run", "source"), input)
            .expect("preservation bundle")
    }

    #[test]
    fn preservation_bytes_are_deterministic_role_separated_and_strict() {
        let first = bundle();
        let second = bundle();
        assert_eq!(first.input_bytes(), second.input_bytes());
        assert_eq!(first.output_bytes(), second.output_bytes());
        assert_ne!(
            first.input_registration().cas_hash(),
            first.output_registration().cas_hash()
        );
        assert_ne!(
            first.input_registration().id(),
            first.output_registration().id()
        );
        assert_eq!(
            ContentHash::sha256(first.input_bytes()),
            *first.input_registration().cas_hash()
        );
        assert_eq!(
            ContentHash::sha256(first.output_bytes()),
            *first.output_registration().cas_hash()
        );
        assert_eq!(
            first.verification().source_ids,
            BTreeSet::from([
                first.evidence().id().clone(),
                first.evidence().source_verification_id().clone(),
            ])
        );
        let input = PreservationInputV1::from_json_bytes(first.input_bytes()).expect("input DTO");
        let result =
            PreservationResultV1::from_json_bytes(first.output_bytes()).expect("result DTO");
        for (mut value, key) in [
            (serde_json::to_value(&input).unwrap(), "input_unknown"),
            (serde_json::to_value(&result).unwrap(), "result_unknown"),
        ] {
            value
                .as_object_mut()
                .expect("DTO object")
                .insert(key.to_owned(), Value::Bool(true));
            let bytes = crate::canonical_json(&value).unwrap();
            assert!(
                PreservationInputV1::from_json_bytes(&bytes).is_err()
                    && PreservationResultV1::from_json_bytes(&bytes).is_err()
            );
        }
        let exact_bytes = vec![0_u8; MAX_M6_PRESERVATION_CAS_BYTES];
        assert!(
            ArtifactRegistrationV5::new(
                sid("run", "target"),
                &exact_bytes,
                PRESERVATION_INPUT_MEDIA_TYPE_V1,
                first.input_registration().source().clone(),
            )
            .is_ok()
        );
        assert!(
            ArtifactRegistrationV5::new(
                sid("run", "target"),
                &vec![0_u8; MAX_M6_PRESERVATION_CAS_BYTES + 1],
                PRESERVATION_INPUT_MEDIA_TYPE_V1,
                first.input_registration().source().clone(),
            )
            .is_err()
        );

        let mut registration =
            serde_json::to_value(first.input_registration()).expect("registration value");
        registration
            .as_object_mut()
            .expect("object")
            .insert("unknown".to_owned(), Value::Bool(true));
        assert!(
            ArtifactRegistrationV5::from_json_bytes(&crate::canonical_json(&registration).unwrap())
                .is_err()
        );
        let mut evidence = serde_json::to_value(first.evidence()).expect("evidence value");
        evidence
            .as_object_mut()
            .expect("object")
            .insert("unknown".to_owned(), Value::Bool(true));
        assert!(
            PreservationEvidenceV5::from_json_bytes(&crate::canonical_json(&evidence).unwrap())
                .is_err()
        );
        let mut verification =
            serde_json::to_value(first.verification()).expect("verification value");
        verification
            .as_object_mut()
            .expect("object")
            .insert("unknown".to_owned(), Value::Bool(true));
        assert!(
            PreservationVerificationV5::from_json_bytes(
                &crate::canonical_json(&verification).unwrap()
            )
            .is_err()
        );

        let mutate = |value: &serde_json::Value, key: &str, replacement: Value| {
            let mut changed = value.clone();
            changed
                .as_object_mut()
                .expect("preservation DTO object")
                .insert(key.to_owned(), replacement);
            crate::canonical_json(&changed).expect("mutated canonical JSON")
        };
        let input_value = serde_json::to_value(&input).expect("input value");
        assert!(
            PreservationInputV1::from_json_bytes(&mutate(
                &input_value,
                "schema",
                Value::String("reviewgraphen.preservation_input.v2".to_owned()),
            ))
            .is_err()
        );
        let result_value = serde_json::to_value(&result).expect("result value");
        assert!(
            PreservationResultV1::from_json_bytes(&mutate(
                &result_value,
                "schema",
                Value::String("reviewgraphen.preservation_result.v2".to_owned()),
            ))
            .is_err()
        );
        let registration =
            serde_json::to_value(first.input_registration()).expect("registration value");
        for (key, replacement) in [
            (
                "schema",
                Value::String("reviewgraphen.artifact_registration.v6".to_owned()),
            ),
            ("id", Value::String("registration-v5:wrong".to_owned())),
            (
                "cas_hash",
                Value::String(ContentHash::sha256(b"wrong").to_string()),
            ),
        ] {
            assert!(
                ArtifactRegistrationV5::from_json_bytes(&mutate(&registration, key, replacement,))
                    .is_err()
            );
        }
        let mut wrong_role = registration.clone();
        wrong_role["source"]["role"] = Value::String("output".to_owned());
        assert!(
            ArtifactRegistrationV5::from_json_bytes(&crate::canonical_json(&wrong_role).unwrap())
                .is_err()
        );
        for (field, replacement) in [
            ("kind", serde_json::json!("other_artifact")),
            ("run_id", serde_json::json!("run:wrong-target")),
            (
                "target_snapshot_id",
                serde_json::json!("snapshot:wrong-target"),
            ),
            (
                "target_obligation_id",
                serde_json::json!("obligation:wrong-target"),
            ),
            ("source_run_id", serde_json::json!("run:wrong-source")),
            (
                "source_verification_id",
                serde_json::json!("verification:wrong"),
            ),
            ("descriptor_id", serde_json::json!("reviewgraphen.other@1")),
            (
                "procedure_version",
                serde_json::json!("reviewgraphen.other.v1"),
            ),
            ("role", serde_json::json!("output")),
        ] {
            let mut changed = registration.clone();
            changed["source"][field] = replacement;
            assert!(
                ArtifactRegistrationV5::from_json_bytes(&crate::canonical_json(&changed).unwrap())
                    .is_err(),
                "nested preservation source substitution must refuse: {field}"
            );
        }

        let evidence = serde_json::to_value(first.evidence()).expect("evidence value");
        for (key, replacement) in [
            (
                "schema",
                Value::String("reviewgraphen.preservation_evidence.v6".to_owned()),
            ),
            (
                "id",
                Value::String("preservation-evidence-v5:wrong".to_owned()),
            ),
            (
                "source_verification_id",
                Value::String("verification:wrong".to_owned()),
            ),
            (
                "descriptor_id",
                Value::String("reviewgraphen.other@1".to_owned()),
            ),
            (
                "procedure_version",
                Value::String("reviewgraphen.other.v1".to_owned()),
            ),
            (
                "dependency_mapping_ids",
                serde_json::json!(["program-mapping-v5:wrong"]),
            ),
        ] {
            assert!(
                PreservationEvidenceV5::from_json_bytes(&mutate(&evidence, key, replacement,))
                    .is_err()
            );
        }
        let verification = serde_json::to_value(first.verification()).expect("verification value");
        for (key, replacement) in [
            (
                "schema",
                Value::String("reviewgraphen.preservation_verification.v6".to_owned()),
            ),
            (
                "id",
                Value::String("preservation-verification-v5:wrong".to_owned()),
            ),
            (
                "source_verification_id",
                Value::String("verification:wrong".to_owned()),
            ),
            (
                "descriptor_id",
                Value::String("reviewgraphen.other@1".to_owned()),
            ),
            (
                "procedure_version",
                Value::String("reviewgraphen.other.v1".to_owned()),
            ),
        ] {
            assert!(
                PreservationVerificationV5::from_json_bytes(&mutate(
                    &verification,
                    key,
                    replacement,
                ))
                .is_err()
            );
        }
    }
}

impl From<DomainError> for M6Error {
    fn from(error: DomainError) -> Self {
        match error {
            DomainError::Incomplete {
                operation,
                limit,
                observed,
            } => Self::Incomplete {
                operation,
                limit,
                observed,
            },
            other => Self::Canonical(other.to_string()),
        }
    }
}

fn bounded(observed: usize, limit: usize, operation: &'static str) -> M6Result<()> {
    if observed > limit {
        return Err(M6Error::Incomplete {
            operation,
            limit,
            observed,
        });
    }
    Ok(())
}

pub(crate) fn derive(kind: &str, identity: &impl Serialize) -> M6Result<StableId> {
    bounded_serialized(identity, MAX_M6_CANONICAL_BYTES, "M6 identity bytes")?;
    let hash = ContentHash::sha256(&crate::canonical_json(identity)?);
    StableId::parse(format!("{kind}:{hash}")).map_err(Into::into)
}

fn body_hash(value: &impl Serialize) -> M6Result<ContentHash> {
    bounded_serialized(value, MAX_M6_CANONICAL_BYTES, "M6 canonical body bytes")?;
    Ok(ContentHash::sha256(&crate::canonical_json(value)?))
}

fn bounded_serialized(
    value: &impl Serialize,
    limit: usize,
    operation: &'static str,
) -> M6Result<()> {
    crate::canonical::canonical_json_count_bounded(value, limit, operation)?;
    Ok(())
}

fn bounded_event_dto(
    value: &impl Serialize,
    limit: usize,
    operation: &'static str,
) -> M6Result<()> {
    let bytes = usize::try_from(crate::canonical::canonical_json_count_bounded(
        value, limit, operation,
    )?)
    .map_err(|_| M6Error::Incomplete {
        operation,
        limit,
        observed: usize::MAX,
    })?;
    preflight_event_line(bytes, 1)?;
    Ok(())
}

fn checked_working_add(total: usize, addition: usize) -> M6Result<usize> {
    let observed = total.checked_add(addition).ok_or(M6Error::Incomplete {
        operation: "M6 mapping retained working bytes",
        limit: MAX_M6_MAPPING_WORKING_BYTES,
        observed: usize::MAX,
    })?;
    bounded(
        observed,
        MAX_M6_MAPPING_WORKING_BYTES,
        "M6 mapping retained working bytes",
    )?;
    Ok(observed)
}

fn full_sha256(field: &'static str, value: &ContentHash) -> M6Result<()> {
    let Some(hex) = value.as_str().strip_prefix("sha256:") else {
        return Err(M6Error::InvalidFullSha256 {
            field,
            value: value.to_string(),
        });
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(M6Error::InvalidFullSha256 {
            field,
            value: value.to_string(),
        });
    }
    Ok(())
}

fn git_oid(field: &'static str, value: &str) -> M6Result<()> {
    if value.len() != 40
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(M6Error::InvalidGitObjectId {
            field,
            value: value.to_owned(),
        });
    }
    Ok(())
}

fn git_tree(field: &'static str, value: &ContentHash) -> M6Result<()> {
    let Some(hex) = value.as_str().strip_prefix("git:") else {
        return Err(M6Error::InvalidGitObjectId {
            field,
            value: value.to_string(),
        });
    };
    git_oid(field, hex)
}

fn require_kind(id: &StableId, kind: &'static str, field: &'static str) -> M6Result<()> {
    if id.kind() != kind {
        return Err(M6Error::Canonical(format!(
            "{field} must have kind {kind}, got {id}"
        )));
    }
    Ok(())
}

fn digest_ids(ids: &BTreeSet<StableId>) -> M6Result<ContentHash> {
    body_hash(&ids.iter().collect::<Vec<_>>())
}

/// Closed role of one structural-preservation CAS object.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreservationArtifactRoleV5 {
    Input,
    Output,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreservationArtifactV5 {
    kind: String,
    run_id: StableId,
    target_snapshot_id: StableId,
    target_obligation_id: StableId,
    source_run_id: StableId,
    source_verification_id: StableId,
    descriptor_id: String,
    procedure_version: String,
    role: PreservationArtifactRoleV5,
}

impl PreservationArtifactV5 {
    pub(crate) fn allocated_bytes(&self) -> usize {
        self.kind.capacity()
            + self.run_id.allocated_bytes()
            + self.target_snapshot_id.allocated_bytes()
            + self.target_obligation_id.allocated_bytes()
            + self.source_run_id.allocated_bytes()
            + self.source_verification_id.allocated_bytes()
            + self.descriptor_id.capacity()
            + self.procedure_version.capacity()
    }

    fn validate(&self) -> M6Result<()> {
        for (id, kind, field) in [
            (&self.run_id, "run", "run_id"),
            (&self.target_snapshot_id, "snapshot", "target_snapshot_id"),
            (
                &self.target_obligation_id,
                "obligation",
                "target_obligation_id",
            ),
            (&self.source_run_id, "run", "source_run_id"),
            (
                &self.source_verification_id,
                "verification",
                "source_verification_id",
            ),
        ] {
            require_kind(id, kind, field)?;
        }
        if self.kind != "preservation_artifact"
            || self.run_id == self.source_run_id
            || self.descriptor_id != STRUCTURAL_PRESERVATION_DESCRIPTOR_V5
            || self.procedure_version != PAYMENT_PRESERVATION_PROCEDURE_V1
        {
            return Err(M6Error::PreservationUnsupported(
                "artifact provenance is outside the sole preservation procedure",
            ));
        }
        Ok(())
    }

    #[must_use]
    pub fn role(&self) -> PreservationArtifactRoleV5 {
        self.role
    }
    #[must_use]
    pub fn target_obligation_id(&self) -> &StableId {
        &self.target_obligation_id
    }
    #[must_use]
    pub fn source_verification_id(&self) -> &StableId {
        &self.source_verification_id
    }
    #[must_use]
    pub fn run_id(&self) -> &StableId {
        &self.run_id
    }
    #[must_use]
    pub fn target_snapshot_id(&self) -> &StableId {
        &self.target_snapshot_id
    }
    #[must_use]
    pub fn source_run_id(&self) -> &StableId {
        &self.source_run_id
    }
    #[must_use]
    pub fn descriptor_id(&self) -> &str {
        &self.descriptor_id
    }
    #[must_use]
    pub fn procedure_version(&self) -> &str {
        &self.procedure_version
    }
}

#[derive(Serialize)]
struct ArtifactRegistrationIdentityV5<'a> {
    cas_hash: &'a ContentHash,
    media_type: &'a str,
    run_id: &'a StableId,
    sensitivity: ArtifactSensitivity,
    size: u64,
    source: &'a PreservationArtifactV5,
}

/// Separate V5 registration contract used only for preservation artifacts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ArtifactRegistrationV5 {
    schema: String,
    id: StableId,
    run_id: StableId,
    cas_hash: ContentHash,
    media_type: String,
    size: u64,
    sensitivity: ArtifactSensitivity,
    source: PreservationArtifactV5,
}

/// Closed, read-only SQL projection for the ADR-0023 preservation
/// registration table. The canonical source cell is produced by Core from
/// the typed source enum; Store never parses a payload or generic DTO JSON.
#[derive(Clone, Debug)]
pub struct ArtifactRegistrationV5IndexProjection {
    pub schema: String,
    pub registration_id: StableId,
    pub run_id: StableId,
    pub cas_hash: ContentHash,
    pub media_type: String,
    pub size: u64,
    pub sensitivity: &'static str,
    pub source_kind: &'static str,
    pub source_canonical_json: String,
    pub body_hash: ContentHash,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactRegistrationWireV5 {
    schema: String,
    id: StableId,
    run_id: StableId,
    cas_hash: ContentHash,
    media_type: String,
    size: u64,
    sensitivity: ArtifactSensitivity,
    source: PreservationArtifactV5,
}

impl ArtifactRegistrationV5 {
    pub fn new(
        run_id: StableId,
        bytes: &[u8],
        media_type: impl Into<String>,
        source: PreservationArtifactV5,
    ) -> M6Result<Self> {
        bounded(
            bytes.len(),
            MAX_M6_PRESERVATION_CAS_BYTES,
            "M6 preservation CAS object",
        )?;
        source.validate()?;
        let media_type = media_type.into();
        let cas_hash = ContentHash::sha256(bytes);
        let size = u64::try_from(bytes.len()).map_err(|_| M6Error::Incomplete {
            operation: "M6 preservation CAS object",
            limit: MAX_M6_PRESERVATION_CAS_BYTES,
            observed: usize::MAX,
        })?;
        let sensitivity = ArtifactSensitivity::CanonicalState;
        let identity = ArtifactRegistrationIdentityV5 {
            cas_hash: &cas_hash,
            media_type: &media_type,
            run_id: &run_id,
            sensitivity,
            size,
            source: &source,
        };
        let value = Self {
            schema: "reviewgraphen.artifact_registration.v5".to_owned(),
            id: derive("registration-v5", &identity)?,
            run_id,
            cas_hash,
            media_type,
            size,
            sensitivity,
            source,
        };
        value.validate()?;
        bounded_event_dto(&value, MAX_M6_CANONICAL_BYTES, "M6 registration DTO bytes")?;
        Ok(value)
    }

    fn validate(&self) -> M6Result<()> {
        self.source.validate()?;
        full_sha256("cas_hash", &self.cas_hash)?;
        let expected_media_type = match self.source.role {
            PreservationArtifactRoleV5::Input => PRESERVATION_INPUT_MEDIA_TYPE_V1,
            PreservationArtifactRoleV5::Output => PRESERVATION_RESULT_MEDIA_TYPE_V1,
        };
        let identity = ArtifactRegistrationIdentityV5 {
            cas_hash: &self.cas_hash,
            media_type: &self.media_type,
            run_id: &self.run_id,
            sensitivity: self.sensitivity,
            size: self.size,
            source: &self.source,
        };
        if self.schema != "reviewgraphen.artifact_registration.v5"
            || self.id != derive("registration-v5", &identity)?
            || self.run_id != self.source.run_id
            || self.sensitivity != ArtifactSensitivity::CanonicalState
            || self.media_type != expected_media_type
            || self.size > MAX_M6_PRESERVATION_CAS_BYTES as u64
        {
            return Err(M6Error::InvalidWire(
                "registration is not the exact role-bound V5 CAS tuple".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn from_json_bytes(input: &[u8]) -> M6Result<Self> {
        preflight_event_line(input.len(), 1)?;
        let wire: ArtifactRegistrationWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        let value = Self {
            schema: wire.schema,
            id: wire.id,
            run_id: wire.run_id,
            cas_hash: wire.cas_hash,
            media_type: wire.media_type,
            size: wire.size,
            sensitivity: wire.sensitivity,
            source: wire.source,
        };
        value.validate()?;
        if crate::canonical_json(&value)? != input {
            return Err(M6Error::InvalidWire(
                "registration JSON is not canonical".to_owned(),
            ));
        }
        Ok(value)
    }

    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    #[must_use]
    pub fn run_id(&self) -> &StableId {
        &self.run_id
    }
    #[must_use]
    pub fn cas_hash(&self) -> &ContentHash {
        &self.cas_hash
    }
    #[must_use]
    pub fn media_type(&self) -> &str {
        &self.media_type
    }
    #[must_use]
    pub const fn size(&self) -> u64 {
        self.size
    }
    #[must_use]
    pub fn source(&self) -> &PreservationArtifactV5 {
        &self.source
    }
    pub fn body_hash(&self) -> M6Result<ContentHash> {
        body_hash(self)
    }

    /// Produces the complete typed column projection for
    /// `artifact_registrations_v5`.
    pub fn index_projection_v6(&self) -> M6Result<ArtifactRegistrationV5IndexProjection> {
        let source_canonical_json = String::from_utf8(crate::canonical_json(&self.source)?)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        Ok(ArtifactRegistrationV5IndexProjection {
            schema: self.schema.clone(),
            registration_id: self.id.clone(),
            run_id: self.run_id.clone(),
            cas_hash: self.cas_hash.clone(),
            media_type: self.media_type.clone(),
            size: self.size,
            sensitivity: "canonical_state",
            source_kind: "preservation_artifact",
            source_canonical_json,
            body_hash: self.body_hash()?,
        })
    }
}

impl<'de> Deserialize<'de> for ArtifactRegistrationV5 {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ArtifactRegistrationWireV5::deserialize(deserializer)?;
        let value = Self {
            schema: wire.schema,
            id: wire.id,
            run_id: wire.run_id,
            cas_hash: wire.cas_hash,
            media_type: wire.media_type,
            size: wire.size,
            sensitivity: wire.sensitivity,
            source: wire.source,
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreservationInputV1 {
    schema: String,
    source_closure_id: StableId,
    morphism_id: StableId,
    correspondence_entry_id: StableId,
    source_claim_id: StableId,
    source_evidence_ids: BTreeSet<StableId>,
    source_verification_id: StableId,
    target_snapshot_id: StableId,
    target_obligation_id: StableId,
    dependency_mapping_ids: BTreeSet<StableId>,
    policy_revision_hash: ContentHash,
}

/// Complete construction tuple for the closed preservation input contract.
pub struct PreservationInputParamsV1 {
    pub source_closure_id: StableId,
    pub morphism_id: StableId,
    pub correspondence_entry_id: StableId,
    pub source_claim_id: StableId,
    pub source_evidence_ids: BTreeSet<StableId>,
    pub source_verification_id: StableId,
    pub target_snapshot_id: StableId,
    pub target_obligation_id: StableId,
    pub dependency_mapping_ids: BTreeSet<StableId>,
    pub policy_revision_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreservationResultV1 {
    schema: String,
    descriptor_id: String,
    procedure_version: String,
    input_hash: ContentHash,
    target_snapshot_id: StableId,
    target_obligation_id: StableId,
    outcome: String,
    source_verification_id: StableId,
    dependency_mapping_ids: BTreeSet<StableId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreservationEvidenceV5 {
    schema: String,
    id: StableId,
    target_snapshot_id: StableId,
    target_obligation_id: StableId,
    source_closure_id: StableId,
    morphism_id: StableId,
    correspondence_entry_id: StableId,
    source_claim_id: StableId,
    source_evidence_ids: BTreeSet<StableId>,
    source_verification_id: StableId,
    dependency_mapping_ids: BTreeSet<StableId>,
    input_registration_id: StableId,
    output_registration_id: StableId,
    descriptor_id: String,
    procedure_version: String,
    observation: String,
    source_ids: BTreeSet<StableId>,
}

#[derive(Clone, Debug)]
pub struct PreservationEvidenceV5IndexProjection {
    pub schema: String,
    pub evidence_id: StableId,
    pub target_snapshot_id: StableId,
    pub target_obligation_id: StableId,
    pub source_closure_id: StableId,
    pub morphism_id: StableId,
    pub correspondence_entry_id: StableId,
    pub source_claim_id: StableId,
    pub source_evidence_ids_canonical_json: String,
    pub source_verification_id: StableId,
    pub dependency_mapping_ids_canonical_json: String,
    pub input_registration_id: StableId,
    pub output_registration_id: StableId,
    pub descriptor_id: String,
    pub procedure_version: String,
    pub observation: String,
    pub source_ids_canonical_json: String,
    pub body_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreservationVerificationV5 {
    schema: String,
    id: StableId,
    target_snapshot_id: StableId,
    target_obligation_id: StableId,
    evidence_id: StableId,
    source_verification_id: StableId,
    descriptor_id: String,
    procedure_version: String,
    outcome: String,
    source_ids: BTreeSet<StableId>,
}

#[derive(Clone, Debug)]
pub struct PreservationVerificationV5IndexProjection {
    pub schema: String,
    pub verification_id: StableId,
    pub target_snapshot_id: StableId,
    pub target_obligation_id: StableId,
    pub evidence_id: StableId,
    pub source_verification_id: StableId,
    pub descriptor_id: String,
    pub procedure_version: String,
    pub outcome: String,
    pub source_ids_canonical_json: String,
    pub body_hash: ContentHash,
}

fn exact_preservation_source_ids(
    input: &PreservationInputV1,
    input_registration_id: &StableId,
    output_registration_id: &StableId,
) -> BTreeSet<StableId> {
    let mut ids = BTreeSet::from([
        input.source_closure_id.clone(),
        input.morphism_id.clone(),
        input.correspondence_entry_id.clone(),
        input.source_claim_id.clone(),
        input.source_verification_id.clone(),
        input_registration_id.clone(),
        output_registration_id.clone(),
    ]);
    ids.extend(input.source_evidence_ids.iter().cloned());
    ids.extend(input.dependency_mapping_ids.iter().cloned());
    ids
}

impl PreservationInputV1 {
    pub fn new(params: PreservationInputParamsV1) -> M6Result<Self> {
        let value = Self {
            schema: "reviewgraphen.preservation_input.v1".to_owned(),
            source_closure_id: params.source_closure_id,
            morphism_id: params.morphism_id,
            correspondence_entry_id: params.correspondence_entry_id,
            source_claim_id: params.source_claim_id,
            source_evidence_ids: params.source_evidence_ids,
            source_verification_id: params.source_verification_id,
            target_snapshot_id: params.target_snapshot_id,
            target_obligation_id: params.target_obligation_id,
            dependency_mapping_ids: params.dependency_mapping_ids,
            policy_revision_hash: params.policy_revision_hash,
        };
        value.validate()?;
        Ok(value)
    }
    fn validate(&self) -> M6Result<()> {
        bounded(
            self.dependency_mapping_ids.len(),
            MAX_M6_PRESERVATION_DEPENDENCY_MAPPINGS,
            "M6 preservation dependency mappings",
        )?;
        if self.schema != "reviewgraphen.preservation_input.v1"
            || self.source_closure_id.kind() != "incremental-source-closure-v5"
            || self.morphism_id.kind() != "change-morphism-v5"
            || self.correspondence_entry_id.kind() != "obligation-correspondence-entry-v5"
            || self.source_claim_id.kind() != "claim"
            || self.source_evidence_ids.is_empty()
            || self
                .source_evidence_ids
                .iter()
                .any(|id| id.kind() != "evidence")
            || self.source_verification_id.kind() != "verification"
            || self.target_snapshot_id.kind() != "snapshot"
            || self.target_obligation_id.kind() != "obligation"
            || self
                .dependency_mapping_ids
                .iter()
                .any(|id| id.kind() != "program-mapping-v5")
        {
            return Err(M6Error::PreservationUnsupported(
                "preservation input closure is incomplete",
            ));
        }
        full_sha256("policy_revision_hash", &self.policy_revision_hash)
    }
    pub fn canonical_bytes(&self) -> M6Result<Vec<u8>> {
        self.validate()?;
        Ok(crate::canonical_json(self)?)
    }
    pub fn from_json_bytes(input: &[u8]) -> M6Result<Self> {
        preflight_event_line(input.len(), 1)?;
        let value: Self = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        value.validate()?;
        if crate::canonical_json(&value)? != input {
            return Err(M6Error::InvalidWire(
                "input JSON is not canonical".to_owned(),
            ));
        }
        Ok(value)
    }
    #[must_use]
    pub fn target_obligation_id(&self) -> &StableId {
        &self.target_obligation_id
    }
    #[must_use]
    pub fn source_closure_id(&self) -> &StableId {
        &self.source_closure_id
    }
    #[must_use]
    pub fn source_verification_id(&self) -> &StableId {
        &self.source_verification_id
    }
    #[must_use]
    pub fn target_snapshot_id(&self) -> &StableId {
        &self.target_snapshot_id
    }
    #[must_use]
    pub fn policy_revision_hash(&self) -> &ContentHash {
        &self.policy_revision_hash
    }
}

impl PreservationResultV1 {
    fn from_input(input: &PreservationInputV1, input_hash: ContentHash) -> M6Result<Self> {
        let value = Self {
            schema: "reviewgraphen.preservation_result.v1".to_owned(),
            descriptor_id: STRUCTURAL_PRESERVATION_DESCRIPTOR_V5.to_owned(),
            procedure_version: PAYMENT_PRESERVATION_PROCEDURE_V1.to_owned(),
            input_hash,
            target_snapshot_id: input.target_snapshot_id.clone(),
            target_obligation_id: input.target_obligation_id.clone(),
            outcome: "passed".to_owned(),
            source_verification_id: input.source_verification_id.clone(),
            dependency_mapping_ids: input.dependency_mapping_ids.clone(),
        };
        value.validate()?;
        Ok(value)
    }
    fn validate(&self) -> M6Result<()> {
        full_sha256("input_hash", &self.input_hash)?;
        if self.schema != "reviewgraphen.preservation_result.v1"
            || self.descriptor_id != STRUCTURAL_PRESERVATION_DESCRIPTOR_V5
            || self.procedure_version != PAYMENT_PRESERVATION_PROCEDURE_V1
            || self.outcome != "passed"
        {
            return Err(M6Error::InvalidWire(
                "invalid preservation result".to_owned(),
            ));
        }
        Ok(())
    }
    pub fn canonical_bytes(&self) -> M6Result<Vec<u8>> {
        self.validate()?;
        Ok(crate::canonical_json(self)?)
    }
    pub fn from_json_bytes(input: &[u8]) -> M6Result<Self> {
        preflight_event_line(input.len(), 1)?;
        let value: Self = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        value.validate()?;
        if crate::canonical_json(&value)? != input {
            return Err(M6Error::InvalidWire(
                "result JSON is not canonical".to_owned(),
            ));
        }
        Ok(value)
    }
    #[must_use]
    pub fn input_hash(&self) -> &ContentHash {
        &self.input_hash
    }
    #[must_use]
    pub fn target_obligation_id(&self) -> &StableId {
        &self.target_obligation_id
    }
    #[must_use]
    pub fn target_snapshot_id(&self) -> &StableId {
        &self.target_snapshot_id
    }
    #[must_use]
    pub fn source_verification_id(&self) -> &StableId {
        &self.source_verification_id
    }
}

impl PreservationEvidenceV5 {
    pub fn index_projection_v6(&self) -> M6Result<PreservationEvidenceV5IndexProjection> {
        Ok(PreservationEvidenceV5IndexProjection {
            schema: self.schema.clone(),
            evidence_id: self.id.clone(),
            target_snapshot_id: self.target_snapshot_id.clone(),
            target_obligation_id: self.target_obligation_id.clone(),
            source_closure_id: self.source_closure_id.clone(),
            morphism_id: self.morphism_id.clone(),
            correspondence_entry_id: self.correspondence_entry_id.clone(),
            source_claim_id: self.source_claim_id.clone(),
            source_evidence_ids_canonical_json: canonical_json_string_v6(
                &self.source_evidence_ids,
            )?,
            source_verification_id: self.source_verification_id.clone(),
            dependency_mapping_ids_canonical_json: canonical_json_string_v6(
                &self.dependency_mapping_ids,
            )?,
            input_registration_id: self.input_registration_id.clone(),
            output_registration_id: self.output_registration_id.clone(),
            descriptor_id: self.descriptor_id.clone(),
            procedure_version: self.procedure_version.clone(),
            observation: self.observation.clone(),
            source_ids_canonical_json: canonical_json_string_v6(&self.source_ids)?,
            body_hash: self.body_hash()?,
        })
    }
    pub(crate) fn retained_bytes(&self) -> M6Result<usize> {
        [
            std::mem::size_of::<Self>(),
            self.schema.capacity(),
            self.id.allocated_bytes(),
            self.target_snapshot_id.allocated_bytes(),
            self.target_obligation_id.allocated_bytes(),
            self.source_closure_id.allocated_bytes(),
            self.morphism_id.allocated_bytes(),
            self.correspondence_entry_id.allocated_bytes(),
            self.source_claim_id.allocated_bytes(),
            conservative_id_set_heap(&self.source_evidence_ids)?,
            self.source_verification_id.allocated_bytes(),
            conservative_id_set_heap(&self.dependency_mapping_ids)?,
            self.input_registration_id.allocated_bytes(),
            self.output_registration_id.allocated_bytes(),
            self.descriptor_id.capacity(),
            self.procedure_version.capacity(),
            self.observation.capacity(),
            conservative_id_set_heap(&self.source_ids)?,
        ]
        .into_iter()
        .try_fold(0_usize, |total, value| {
            total.checked_add(value).ok_or(M6Error::Incomplete {
                operation: "M6 preservation evidence retained bytes",
                limit: MAX_M6_PARTIAL_RERUN_WORKING_BYTES,
                observed: usize::MAX,
            })
        })
    }
    fn new(
        input: &PreservationInputV1,
        input_registration_id: StableId,
        output_registration_id: StableId,
    ) -> M6Result<Self> {
        let mut value = Self {
            schema: "reviewgraphen.preservation_evidence.v5".to_owned(),
            id: StableId::parse("preservation-evidence-v5:pending")?,
            target_snapshot_id: input.target_snapshot_id.clone(),
            target_obligation_id: input.target_obligation_id.clone(),
            source_closure_id: input.source_closure_id.clone(),
            morphism_id: input.morphism_id.clone(),
            correspondence_entry_id: input.correspondence_entry_id.clone(),
            source_claim_id: input.source_claim_id.clone(),
            source_evidence_ids: input.source_evidence_ids.clone(),
            source_verification_id: input.source_verification_id.clone(),
            dependency_mapping_ids: input.dependency_mapping_ids.clone(),
            input_registration_id,
            output_registration_id,
            descriptor_id: STRUCTURAL_PRESERVATION_DESCRIPTOR_V5.to_owned(),
            procedure_version: PAYMENT_PRESERVATION_PROCEDURE_V1.to_owned(),
            observation: "structure_preserved".to_owned(),
            source_ids: BTreeSet::new(),
        };
        value.source_ids = exact_preservation_source_ids(
            input,
            &value.input_registration_id,
            &value.output_registration_id,
        );
        #[derive(Serialize)]
        struct Identity<'a> {
            target_snapshot_id: &'a StableId,
            target_obligation_id: &'a StableId,
            source_closure_id: &'a StableId,
            morphism_id: &'a StableId,
            correspondence_entry_id: &'a StableId,
            source_claim_id: &'a StableId,
            source_evidence_ids: &'a BTreeSet<StableId>,
            source_verification_id: &'a StableId,
            dependency_mapping_ids: &'a BTreeSet<StableId>,
            input_registration_id: &'a StableId,
            output_registration_id: &'a StableId,
            descriptor_id: &'a str,
            procedure_version: &'a str,
            observation: &'a str,
            source_ids: &'a BTreeSet<StableId>,
        }
        value.id = derive(
            "preservation-evidence-v5",
            &Identity {
                target_snapshot_id: &value.target_snapshot_id,
                target_obligation_id: &value.target_obligation_id,
                source_closure_id: &value.source_closure_id,
                morphism_id: &value.morphism_id,
                correspondence_entry_id: &value.correspondence_entry_id,
                source_claim_id: &value.source_claim_id,
                source_evidence_ids: &value.source_evidence_ids,
                source_verification_id: &value.source_verification_id,
                dependency_mapping_ids: &value.dependency_mapping_ids,
                input_registration_id: &value.input_registration_id,
                output_registration_id: &value.output_registration_id,
                descriptor_id: &value.descriptor_id,
                procedure_version: &value.procedure_version,
                observation: &value.observation,
                source_ids: &value.source_ids,
            },
        )?;
        value.validate()?;
        Ok(value)
    }
    fn validate(&self) -> M6Result<()> {
        let input = PreservationInputV1 {
            schema: "reviewgraphen.preservation_input.v1".to_owned(),
            source_closure_id: self.source_closure_id.clone(),
            morphism_id: self.morphism_id.clone(),
            correspondence_entry_id: self.correspondence_entry_id.clone(),
            source_claim_id: self.source_claim_id.clone(),
            source_evidence_ids: self.source_evidence_ids.clone(),
            source_verification_id: self.source_verification_id.clone(),
            target_snapshot_id: self.target_snapshot_id.clone(),
            target_obligation_id: self.target_obligation_id.clone(),
            dependency_mapping_ids: self.dependency_mapping_ids.clone(),
            policy_revision_hash: ContentHash::sha256(b"validation-only"),
        };
        let exact_sources = exact_preservation_source_ids(
            &input,
            &self.input_registration_id,
            &self.output_registration_id,
        );
        #[derive(Serialize)]
        struct Identity<'a> {
            target_snapshot_id: &'a StableId,
            target_obligation_id: &'a StableId,
            source_closure_id: &'a StableId,
            morphism_id: &'a StableId,
            correspondence_entry_id: &'a StableId,
            source_claim_id: &'a StableId,
            source_evidence_ids: &'a BTreeSet<StableId>,
            source_verification_id: &'a StableId,
            dependency_mapping_ids: &'a BTreeSet<StableId>,
            input_registration_id: &'a StableId,
            output_registration_id: &'a StableId,
            descriptor_id: &'a str,
            procedure_version: &'a str,
            observation: &'a str,
            source_ids: &'a BTreeSet<StableId>,
        }
        let expected = derive(
            "preservation-evidence-v5",
            &Identity {
                target_snapshot_id: &self.target_snapshot_id,
                target_obligation_id: &self.target_obligation_id,
                source_closure_id: &self.source_closure_id,
                morphism_id: &self.morphism_id,
                correspondence_entry_id: &self.correspondence_entry_id,
                source_claim_id: &self.source_claim_id,
                source_evidence_ids: &self.source_evidence_ids,
                source_verification_id: &self.source_verification_id,
                dependency_mapping_ids: &self.dependency_mapping_ids,
                input_registration_id: &self.input_registration_id,
                output_registration_id: &self.output_registration_id,
                descriptor_id: &self.descriptor_id,
                procedure_version: &self.procedure_version,
                observation: &self.observation,
                source_ids: &self.source_ids,
            },
        )?;
        if self.schema != "reviewgraphen.preservation_evidence.v5"
            || self.id != expected
            || self.descriptor_id != STRUCTURAL_PRESERVATION_DESCRIPTOR_V5
            || self.procedure_version != PAYMENT_PRESERVATION_PROCEDURE_V1
            || self.observation != "structure_preserved"
            || self.source_ids != exact_sources
        {
            return Err(M6Error::InvalidWire(
                "invalid preservation evidence closure".to_owned(),
            ));
        }
        Ok(())
    }
    pub fn from_json_bytes(input: &[u8]) -> M6Result<Self> {
        preflight_event_line(input.len(), 1)?;
        let value: Self = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        value.validate()?;
        if crate::canonical_json(&value)? != input {
            return Err(M6Error::InvalidWire(
                "evidence JSON is not canonical".to_owned(),
            ));
        }
        Ok(value)
    }
    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    #[must_use]
    pub fn target_obligation_id(&self) -> &StableId {
        &self.target_obligation_id
    }
    #[must_use]
    pub fn source_verification_id(&self) -> &StableId {
        &self.source_verification_id
    }
    #[must_use]
    pub fn source_claim_id(&self) -> &StableId {
        &self.source_claim_id
    }
    #[must_use]
    pub fn source_evidence_ids(&self) -> &BTreeSet<StableId> {
        &self.source_evidence_ids
    }
    #[must_use]
    pub fn input_registration_id(&self) -> &StableId {
        &self.input_registration_id
    }
    #[must_use]
    pub fn output_registration_id(&self) -> &StableId {
        &self.output_registration_id
    }
    pub fn body_hash(&self) -> M6Result<ContentHash> {
        body_hash(self)
    }
}

impl PreservationVerificationV5 {
    pub(crate) fn retained_bytes(&self) -> M6Result<usize> {
        [
            std::mem::size_of::<Self>(),
            self.schema.capacity(),
            self.id.allocated_bytes(),
            self.target_snapshot_id.allocated_bytes(),
            self.target_obligation_id.allocated_bytes(),
            self.evidence_id.allocated_bytes(),
            self.source_verification_id.allocated_bytes(),
            self.descriptor_id.capacity(),
            self.procedure_version.capacity(),
            self.outcome.capacity(),
            conservative_id_set_heap(&self.source_ids)?,
        ]
        .into_iter()
        .try_fold(0_usize, |total, value| {
            total.checked_add(value).ok_or(M6Error::Incomplete {
                operation: "M6 preservation verification retained bytes",
                limit: MAX_M6_PARTIAL_RERUN_WORKING_BYTES,
                observed: usize::MAX,
            })
        })
    }
    fn new(input: &PreservationInputV1, evidence: &PreservationEvidenceV5) -> M6Result<Self> {
        let source_ids =
            BTreeSet::from([evidence.id.clone(), input.source_verification_id.clone()]);
        #[derive(Serialize)]
        struct Identity<'a> {
            target_snapshot_id: &'a StableId,
            target_obligation_id: &'a StableId,
            evidence_id: &'a StableId,
            source_verification_id: &'a StableId,
            descriptor_id: &'a str,
            procedure_version: &'a str,
            outcome: &'a str,
            source_ids: &'a BTreeSet<StableId>,
        }
        let descriptor_id = STRUCTURAL_PRESERVATION_DESCRIPTOR_V5.to_owned();
        let procedure_version = PAYMENT_PRESERVATION_PROCEDURE_V1.to_owned();
        let outcome = "passed".to_owned();
        let id = derive(
            "preservation-verification-v5",
            &Identity {
                target_snapshot_id: &input.target_snapshot_id,
                target_obligation_id: &input.target_obligation_id,
                evidence_id: &evidence.id,
                source_verification_id: &input.source_verification_id,
                descriptor_id: &descriptor_id,
                procedure_version: &procedure_version,
                outcome: &outcome,
                source_ids: &source_ids,
            },
        )?;
        let value = Self {
            schema: "reviewgraphen.preservation_verification.v5".to_owned(),
            id,
            target_snapshot_id: input.target_snapshot_id.clone(),
            target_obligation_id: input.target_obligation_id.clone(),
            evidence_id: evidence.id.clone(),
            source_verification_id: input.source_verification_id.clone(),
            descriptor_id,
            procedure_version,
            outcome,
            source_ids,
        };
        value.validate()?;
        Ok(value)
    }
    fn validate(&self) -> M6Result<()> {
        #[derive(Serialize)]
        struct Identity<'a> {
            target_snapshot_id: &'a StableId,
            target_obligation_id: &'a StableId,
            evidence_id: &'a StableId,
            source_verification_id: &'a StableId,
            descriptor_id: &'a str,
            procedure_version: &'a str,
            outcome: &'a str,
            source_ids: &'a BTreeSet<StableId>,
        }
        let expected_sources = BTreeSet::from([
            self.evidence_id.clone(),
            self.source_verification_id.clone(),
        ]);
        let expected = derive(
            "preservation-verification-v5",
            &Identity {
                target_snapshot_id: &self.target_snapshot_id,
                target_obligation_id: &self.target_obligation_id,
                evidence_id: &self.evidence_id,
                source_verification_id: &self.source_verification_id,
                descriptor_id: &self.descriptor_id,
                procedure_version: &self.procedure_version,
                outcome: &self.outcome,
                source_ids: &self.source_ids,
            },
        )?;
        if self.schema != "reviewgraphen.preservation_verification.v5"
            || self.id != expected
            || self.evidence_id.kind() != "preservation-evidence-v5"
            || self.source_verification_id.kind() != "verification"
            || self.descriptor_id != STRUCTURAL_PRESERVATION_DESCRIPTOR_V5
            || self.procedure_version != PAYMENT_PRESERVATION_PROCEDURE_V1
            || self.outcome != "passed"
            || self.source_ids != expected_sources
        {
            return Err(M6Error::InvalidWire(
                "invalid preservation verification closure".to_owned(),
            ));
        }
        Ok(())
    }
    pub fn from_json_bytes(input: &[u8]) -> M6Result<Self> {
        preflight_event_line(input.len(), 1)?;
        let value: Self = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        value.validate()?;
        if crate::canonical_json(&value)? != input {
            return Err(M6Error::InvalidWire(
                "verification JSON is not canonical".to_owned(),
            ));
        }
        Ok(value)
    }
    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    #[must_use]
    pub fn evidence_id(&self) -> &StableId {
        &self.evidence_id
    }
    #[must_use]
    pub fn target_obligation_id(&self) -> &StableId {
        &self.target_obligation_id
    }
    #[must_use]
    pub fn source_verification_id(&self) -> &StableId {
        &self.source_verification_id
    }
    pub fn index_projection_v6(&self) -> M6Result<PreservationVerificationV5IndexProjection> {
        Ok(PreservationVerificationV5IndexProjection {
            schema: self.schema.clone(),
            verification_id: self.id.clone(),
            target_snapshot_id: self.target_snapshot_id.clone(),
            target_obligation_id: self.target_obligation_id.clone(),
            evidence_id: self.evidence_id.clone(),
            source_verification_id: self.source_verification_id.clone(),
            descriptor_id: self.descriptor_id.clone(),
            procedure_version: self.procedure_version.clone(),
            outcome: self.outcome.clone(),
            source_ids_canonical_json: canonical_json_string_v6(&self.source_ids)?,
            body_hash: self.body_hash()?,
        })
    }
    pub fn body_hash(&self) -> M6Result<ContentHash> {
        body_hash(self)
    }
}

/// Fully deterministic, process/network/workspace-write-free preservation output.
#[derive(Clone, Debug)]
pub struct PreservationBundleV5 {
    input_bytes: Vec<u8>,
    output_bytes: Vec<u8>,
    input_registration: ArtifactRegistrationV5,
    output_registration: ArtifactRegistrationV5,
    evidence: PreservationEvidenceV5,
    verification: PreservationVerificationV5,
}

impl PreservationBundleV5 {
    pub(crate) fn build(
        target_run_id: StableId,
        source_run_id: StableId,
        input: PreservationInputV1,
    ) -> M6Result<Self> {
        input.validate()?;
        let input_bytes = input.canonical_bytes()?;
        let input_hash = ContentHash::sha256(&input_bytes);
        let result = PreservationResultV1::from_input(&input, input_hash)?;
        let output_bytes = result.canonical_bytes()?;
        let source = |role| PreservationArtifactV5 {
            kind: "preservation_artifact".to_owned(),
            run_id: target_run_id.clone(),
            target_snapshot_id: input.target_snapshot_id.clone(),
            target_obligation_id: input.target_obligation_id.clone(),
            source_run_id: source_run_id.clone(),
            source_verification_id: input.source_verification_id.clone(),
            descriptor_id: STRUCTURAL_PRESERVATION_DESCRIPTOR_V5.to_owned(),
            procedure_version: PAYMENT_PRESERVATION_PROCEDURE_V1.to_owned(),
            role,
        };
        let input_registration = ArtifactRegistrationV5::new(
            target_run_id.clone(),
            &input_bytes,
            PRESERVATION_INPUT_MEDIA_TYPE_V1,
            source(PreservationArtifactRoleV5::Input),
        )?;
        let output_registration = ArtifactRegistrationV5::new(
            target_run_id.clone(),
            &output_bytes,
            PRESERVATION_RESULT_MEDIA_TYPE_V1,
            source(PreservationArtifactRoleV5::Output),
        )?;
        let evidence = PreservationEvidenceV5::new(
            &input,
            input_registration.id.clone(),
            output_registration.id.clone(),
        )?;
        let verification = PreservationVerificationV5::new(&input, &evidence)?;
        Ok(Self {
            input_bytes,
            output_bytes,
            input_registration,
            output_registration,
            evidence,
            verification,
        })
    }
    #[must_use]
    pub fn input_bytes(&self) -> &[u8] {
        &self.input_bytes
    }
    #[must_use]
    pub fn output_bytes(&self) -> &[u8] {
        &self.output_bytes
    }
    #[must_use]
    pub fn input_registration(&self) -> &ArtifactRegistrationV5 {
        &self.input_registration
    }
    #[must_use]
    pub fn output_registration(&self) -> &ArtifactRegistrationV5 {
        &self.output_registration
    }
    #[must_use]
    pub fn evidence(&self) -> &PreservationEvidenceV5 {
        &self.evidence
    }
    #[must_use]
    pub fn verification(&self) -> &PreservationVerificationV5 {
        &self.verification
    }
}

/// Opaque, sealed preservation handoff for partial-rerun planning.  Callers
/// cannot provide detached verification IDs: every member is revalidated as
/// the exact evidence/verification pair produced by an admitted bundle.
#[derive(Clone, Debug)]
pub struct M6PreservationPhaseV5 {
    source_closure_id: StableId,
    morphism_id: StableId,
    correspondence_id: StableId,
    staleness_assessment_id: StableId,
    target_snapshot_id: StableId,
    evidence: Vec<PreservationEvidenceV5>,
    verifications: Vec<PreservationVerificationV5>,
}

impl M6PreservationPhaseV5 {
    pub(crate) fn evidence(&self) -> &[PreservationEvidenceV5] {
        &self.evidence
    }

    pub(crate) fn verifications(&self) -> &[PreservationVerificationV5] {
        &self.verifications
    }

    pub(crate) fn retained_bytes(&self) -> M6Result<usize> {
        let mut total = std::mem::size_of::<Self>();
        let add = |total: &mut usize, value: usize| -> M6Result<()> {
            *total = total.checked_add(value).ok_or(M6Error::Incomplete {
                operation: "M6 preservation retained bytes",
                limit: MAX_M6_PARTIAL_RERUN_WORKING_BYTES,
                observed: usize::MAX,
            })?;
            Ok(())
        };
        for id in [
            &self.source_closure_id,
            &self.morphism_id,
            &self.correspondence_id,
            &self.staleness_assessment_id,
            &self.target_snapshot_id,
        ] {
            add(&mut total, id.allocated_bytes())?;
        }
        add(
            &mut total,
            self.evidence
                .capacity()
                .checked_mul(std::mem::size_of::<PreservationEvidenceV5>())
                .ok_or(M6Error::Incomplete {
                    operation: "M6 preservation retained bytes",
                    limit: MAX_M6_PARTIAL_RERUN_WORKING_BYTES,
                    observed: usize::MAX,
                })?,
        )?;
        for value in &self.evidence {
            for bytes in [
                value.schema.capacity(),
                value.id.allocated_bytes(),
                value.target_snapshot_id.allocated_bytes(),
                value.target_obligation_id.allocated_bytes(),
                value.source_closure_id.allocated_bytes(),
                value.morphism_id.allocated_bytes(),
                value.correspondence_entry_id.allocated_bytes(),
                value.source_claim_id.allocated_bytes(),
                value.source_verification_id.allocated_bytes(),
                value.input_registration_id.allocated_bytes(),
                value.output_registration_id.allocated_bytes(),
                value.descriptor_id.capacity(),
                value.procedure_version.capacity(),
                value.observation.capacity(),
                id_set_heap(&value.source_evidence_ids),
                id_set_heap(&value.dependency_mapping_ids),
                id_set_heap(&value.source_ids),
            ] {
                add(&mut total, bytes)?;
            }
        }
        add(
            &mut total,
            self.verifications
                .capacity()
                .checked_mul(std::mem::size_of::<PreservationVerificationV5>())
                .ok_or(M6Error::Incomplete {
                    operation: "M6 preservation retained bytes",
                    limit: MAX_M6_PARTIAL_RERUN_WORKING_BYTES,
                    observed: usize::MAX,
                })?,
        )?;
        for value in &self.verifications {
            for bytes in [
                value.schema.capacity(),
                value.id.allocated_bytes(),
                value.target_snapshot_id.allocated_bytes(),
                value.target_obligation_id.allocated_bytes(),
                value.evidence_id.allocated_bytes(),
                value.source_verification_id.allocated_bytes(),
                value.descriptor_id.capacity(),
                value.procedure_version.capacity(),
                value.outcome.capacity(),
                id_set_heap(&value.source_ids),
            ] {
                add(&mut total, bytes)?;
            }
        }
        Ok(total)
    }

    pub(crate) fn from_admitted_bundles(
        staleness: &M6StalenessPhaseV5,
        correspondence: &M6ObligationCorrespondencePhaseV5,
        bundles: &[PreservationBundleV5],
    ) -> M6Result<Self> {
        bounded(
            bundles.len(),
            MAX_M6_PRESERVATION_RECORDS,
            "M6 preservation phase members",
        )?;
        if correspondence.correspondence().id() != staleness.assessment.correspondence_id() {
            return Err(M6Error::PreservationUnsupported(
                "preservation correspondence differs from sealed staleness",
            ));
        }
        let entry_ids = correspondence
            .entries()
            .iter()
            .map(|entry| entry.id())
            .collect::<BTreeSet<_>>();
        let mut ordered = bundles.iter().collect::<Vec<_>>();
        ordered.sort_by(|left, right| {
            left.evidence
                .target_obligation_id
                .cmp(&right.evidence.target_obligation_id)
        });
        if ordered.windows(2).any(|pair| {
            pair[0].evidence.target_obligation_id >= pair[1].evidence.target_obligation_id
        }) {
            return Err(M6Error::PreservationUnsupported(
                "preservation phase targets are not unique",
            ));
        }
        let mut evidence = Vec::with_capacity(ordered.len());
        let mut verifications = Vec::with_capacity(ordered.len());
        for bundle in ordered {
            bundle.evidence.validate()?;
            bundle.verification.validate()?;
            if bundle.evidence.source_closure_id != staleness.assessment.source_closure_id
                || bundle.evidence.morphism_id != staleness.assessment.morphism_id
                || bundle.evidence.target_snapshot_id != staleness.target_snapshot_id
                || !entry_ids.contains(&bundle.evidence.correspondence_entry_id)
                || bundle.verification.evidence_id != bundle.evidence.id
                || bundle.verification.target_snapshot_id != bundle.evidence.target_snapshot_id
                || bundle.verification.target_obligation_id != bundle.evidence.target_obligation_id
                || !staleness
                    .is_sealed_preservation_candidate(&bundle.evidence.target_obligation_id)?
            {
                return Err(M6Error::PreservationUnsupported(
                    "preservation phase pair is not bound to the sealed M6 inputs",
                ));
            }
            evidence.push(bundle.evidence.clone());
            verifications.push(bundle.verification.clone());
        }
        Ok(Self {
            source_closure_id: staleness.assessment.source_closure_id.clone(),
            morphism_id: staleness.assessment.morphism_id.clone(),
            correspondence_id: staleness.assessment.correspondence_id.clone(),
            staleness_assessment_id: staleness.assessment.id.clone(),
            target_snapshot_id: staleness.target_snapshot_id.clone(),
            evidence,
            verifications,
        })
    }

    /// Creates the exact, phase-bound empty preservation handoff.
    #[must_use]
    pub fn empty(staleness: &M6StalenessPhaseV5) -> Self {
        Self {
            source_closure_id: staleness.assessment.source_closure_id.clone(),
            morphism_id: staleness.assessment.morphism_id.clone(),
            correspondence_id: staleness.assessment.correspondence_id.clone(),
            staleness_assessment_id: staleness.assessment.id.clone(),
            target_snapshot_id: staleness.target_snapshot_id.clone(),
            evidence: Vec::new(),
            verifications: Vec::new(),
        }
    }
}

fn partial_rerun_working_bytes_v5(
    action_count: usize,
    metadata_ids: usize,
    limit: usize,
) -> M6Result<usize> {
    let per_action = std::mem::size_of::<PartialRerunActionV5>()
        .checked_add(1_024)
        .ok_or(M6Error::Incomplete {
            operation: "M6 partial rerun working bytes",
            limit,
            observed: usize::MAX,
        })?;
    let per_metadata_id =
        std::mem::size_of::<StableId>()
            .checked_add(128)
            .ok_or(M6Error::Incomplete {
                operation: "M6 partial rerun working bytes",
                limit,
                observed: usize::MAX,
            })?;
    action_count
        .checked_mul(per_action)
        .and_then(|value| {
            metadata_ids
                .checked_mul(per_metadata_id)
                .and_then(|metadata| value.checked_add(metadata))
        })
        .and_then(|value| value.checked_add(MAX_M6_CANONICAL_BYTES))
        .ok_or(M6Error::Incomplete {
            operation: "M6 partial rerun working bytes",
            limit,
            observed: usize::MAX,
        })
}

/// Immutable target-side record witness used when a later partial-rerun slice
/// implements exact predecessor suppression.  The initial planner deliberately
/// emits no values of this type.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExistingTargetRecordV5 {
    record_id: StableId,
    body_hash: ContentHash,
    event_id: StableId,
}

impl ExistingTargetRecordV5 {
    pub fn new(record_id: StableId, body_hash: ContentHash, event_id: StableId) -> M6Result<Self> {
        full_sha256("body_hash", &body_hash)?;
        require_kind(&event_id, "event", "event_id")?;
        let value = Self {
            record_id,
            body_hash,
            event_id,
        };
        bounded_serialized(
            &value,
            MAX_M6_CANONICAL_BYTES,
            "M6 existing target record DTO bytes",
        )?;
        Ok(value)
    }

    pub fn from_json_bytes(input: &[u8]) -> M6Result<Self> {
        bounded(
            input.len(),
            MAX_M6_CANONICAL_BYTES,
            "M6 existing target record JSON bytes",
        )?;
        let value: Self = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        let expected = Self::new(value.record_id, value.body_hash, value.event_id)?;
        if crate::canonical_json(&expected)? != input {
            return Err(M6Error::InvalidWire(
                "existing target record JSON is not exact canonical content".to_owned(),
            ));
        }
        Ok(expected)
    }

    #[must_use]
    pub fn record_id(&self) -> &StableId {
        &self.record_id
    }
    #[must_use]
    pub fn body_hash(&self) -> &ContentHash {
        &self.body_hash
    }
    #[must_use]
    pub fn event_id(&self) -> &StableId {
        &self.event_id
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActionPrerequisiteV5 {
    ScheduledAction {
        action_id: StableId,
    },
    ExistingTargetRecord {
        record_id: StableId,
        body_hash: ContentHash,
        event_id: StableId,
    },
}

impl ActionPrerequisiteV5 {
    pub(crate) fn scheduled(action_id: StableId) -> M6Result<Self> {
        if action_id.kind() != "partial-rerun-action-v5"
            && action_id.kind() != "gluing-rerun-action-v5"
        {
            return Err(M6Error::Canonical(format!(
                "action_id must have a closed M6 action kind, got {action_id}"
            )));
        }
        Ok(Self::ScheduledAction { action_id })
    }

    fn validate(&self) -> M6Result<()> {
        match self {
            Self::ScheduledAction { action_id } => Self::scheduled(action_id.clone()).map(|_| ()),
            Self::ExistingTargetRecord {
                record_id,
                body_hash,
                event_id,
            } => {
                ExistingTargetRecordV5::new(record_id.clone(), body_hash.clone(), event_id.clone())
                    .map(|_| ())
            }
        }
    }

    pub fn from_json_bytes(input: &[u8]) -> M6Result<Self> {
        bounded(
            input.len(),
            MAX_M6_CANONICAL_BYTES,
            "M6 action prerequisite JSON bytes",
        )?;
        let value: Self = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        value.validate()?;
        if crate::canonical_json(&value)? != input {
            return Err(M6Error::InvalidWire(
                "action prerequisite JSON is not exact canonical content".to_owned(),
            ));
        }
        Ok(value)
    }
}

fn validate_partial_action_prerequisite_v5(value: &ActionPrerequisiteV5) -> M6Result<()> {
    match value {
        ActionPrerequisiteV5::ScheduledAction { action_id } => require_kind(
            action_id,
            "partial-rerun-action-v5",
            "partial action prerequisite",
        ),
        ActionPrerequisiteV5::ExistingTargetRecord {
            record_id,
            body_hash,
            event_id,
        } => ExistingTargetRecordV5::new(record_id.clone(), body_hash.clone(), event_id.clone())
            .map(|_| ()),
    }
}

fn validate_gluing_action_prerequisite_v5(
    action: GluingRerunActionKindV5,
    value: &ActionPrerequisiteV5,
) -> M6Result<()> {
    match value {
        ActionPrerequisiteV5::ScheduledAction { action_id } => match action {
            GluingRerunActionKindV5::RegisterGluingInput => Err(M6Error::Canonical(
                "gluing registration has no prerequisites".to_owned(),
            )),
            GluingRerunActionKindV5::RebuildSection => {
                if action_id.kind() != "gluing-rerun-action-v5"
                    && action_id.kind() != "partial-rerun-action-v5"
                {
                    return Err(M6Error::Canonical(
                        "gluing rebuild prerequisite has an unclosed action namespace".to_owned(),
                    ));
                }
                Ok(())
            }
            GluingRerunActionKindV5::Reglue => {
                require_kind(action_id, "gluing-rerun-action-v5", "reglue prerequisite")
            }
        },
        ActionPrerequisiteV5::ExistingTargetRecord {
            record_id,
            body_hash,
            event_id,
        } => ExistingTargetRecordV5::new(record_id.clone(), body_hash.clone(), event_id.clone())
            .map(|_| ()),
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PartialRerunSubjectKindV5 {
    Obligation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PartialRerunActionKindV5 {
    ReprojectContext,
    RerunReviewer,
    RerunVerifier,
    RerunHumanDecision,
}

fn validate_action_prerequisite_shape_v5(
    action: PartialRerunActionKindV5,
    prerequisites: &[ActionPrerequisiteV5],
) -> M6Result<()> {
    let scheduled = prerequisites
        .iter()
        .filter(|value| matches!(value, ActionPrerequisiteV5::ScheduledAction { .. }))
        .count();
    let existing = prerequisites.len().saturating_sub(scheduled);
    let existing_kinds = prerequisites
        .iter()
        .filter_map(|value| match value {
            ActionPrerequisiteV5::ExistingTargetRecord { record_id, .. } => Some(record_id.kind()),
            ActionPrerequisiteV5::ScheduledAction { .. } => None,
        })
        .collect::<Vec<_>>();
    let valid = match action {
        PartialRerunActionKindV5::ReprojectContext => prerequisites.is_empty(),
        PartialRerunActionKindV5::RerunReviewer => {
            (prerequisites.len() == 1 && scheduled == 1)
                || existing_kinds.as_slice() == ["context-envelope"]
        }
        PartialRerunActionKindV5::RerunVerifier => {
            (prerequisites.len() == 1 && scheduled == 1)
                || (prerequisites.len() == 2
                    && existing == 2
                    && existing_kinds.contains(&"execution")
                    && existing_kinds.contains(&"claim"))
        }
        PartialRerunActionKindV5::RerunHumanDecision => {
            (prerequisites.len() == 1 && scheduled == 1)
                || existing_kinds.as_slice() == ["verification"]
        }
    };
    if !valid {
        return Err(M6Error::InvalidStalenessAssessment(
            "partial rerun prerequisite shape does not match the action",
        ));
    }
    Ok(())
}

#[derive(Serialize)]
struct PartialRerunActionIdentityV5<'a> {
    staleness_assessment_id: &'a StableId,
    subject_kind: PartialRerunSubjectKindV5,
    subject_ids: &'a BTreeSet<StableId>,
    action: PartialRerunActionKindV5,
    prerequisites: &'a [ActionPrerequisiteV5],
    stale_source_record_ids: &'a BTreeSet<StableId>,
    reasons: &'a BTreeSet<StaleReasonV5>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PartialRerunActionV5 {
    schema: &'static str,
    id: StableId,
    staleness_assessment_id: StableId,
    subject_kind: PartialRerunSubjectKindV5,
    subject_ids: BTreeSet<StableId>,
    action: PartialRerunActionKindV5,
    prerequisites: Vec<ActionPrerequisiteV5>,
    stale_source_record_ids: BTreeSet<StableId>,
    reasons: BTreeSet<StaleReasonV5>,
    source_ids: BTreeSet<StableId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PartialRerunActionWireV5 {
    schema: String,
    id: StableId,
    staleness_assessment_id: StableId,
    subject_kind: PartialRerunSubjectKindV5,
    subject_ids: BTreeSet<StableId>,
    action: PartialRerunActionKindV5,
    prerequisites: Vec<ActionPrerequisiteV5>,
    stale_source_record_ids: BTreeSet<StableId>,
    reasons: BTreeSet<StaleReasonV5>,
    source_ids: BTreeSet<StableId>,
}

impl PartialRerunActionV5 {
    pub(crate) fn retained_bytes(&self) -> M6Result<usize> {
        let prerequisite_bytes = self
            .prerequisites
            .iter()
            .try_fold(0_usize, |total, value| {
                let owned = match value {
                    ActionPrerequisiteV5::ScheduledAction { action_id } => {
                        action_id.allocated_bytes()
                    }
                    ActionPrerequisiteV5::ExistingTargetRecord {
                        record_id,
                        body_hash,
                        event_id,
                    } => record_id
                        .allocated_bytes()
                        .checked_add(body_hash.allocated_bytes())
                        .and_then(|value| value.checked_add(event_id.allocated_bytes()))
                        .ok_or(M6Error::Incomplete {
                            operation: "M6 partial action retained bytes",
                            limit: MAX_M6_PARTIAL_RERUN_WORKING_BYTES,
                            observed: usize::MAX,
                        })?,
                };
                total.checked_add(owned).ok_or(M6Error::Incomplete {
                    operation: "M6 partial action retained bytes",
                    limit: MAX_M6_PARTIAL_RERUN_WORKING_BYTES,
                    observed: usize::MAX,
                })
            })?;
        [
            std::mem::size_of::<Self>(),
            self.id.allocated_bytes(),
            self.staleness_assessment_id.allocated_bytes(),
            conservative_id_set_heap(&self.subject_ids)?,
            self.prerequisites
                .capacity()
                .checked_mul(std::mem::size_of::<ActionPrerequisiteV5>())
                .ok_or(M6Error::Incomplete {
                    operation: "M6 partial action retained bytes",
                    limit: MAX_M6_PARTIAL_RERUN_WORKING_BYTES,
                    observed: usize::MAX,
                })?,
            prerequisite_bytes,
            conservative_id_set_heap(&self.stale_source_record_ids)?,
            conservative_btree_node_bytes(self.reasons.len())?
                .checked_add(
                    self.reasons
                        .len()
                        .checked_mul(std::mem::size_of::<StaleReasonV5>())
                        .ok_or(M6Error::Incomplete {
                            operation: "M6 partial action retained bytes",
                            limit: MAX_M6_PARTIAL_RERUN_WORKING_BYTES,
                            observed: usize::MAX,
                        })?,
                )
                .ok_or(M6Error::Incomplete {
                    operation: "M6 partial action retained bytes",
                    limit: MAX_M6_PARTIAL_RERUN_WORKING_BYTES,
                    observed: usize::MAX,
                })?,
            conservative_id_set_heap(&self.source_ids)?,
        ]
        .into_iter()
        .try_fold(0_usize, |total, value| {
            total.checked_add(value).ok_or(M6Error::Incomplete {
                operation: "M6 partial action retained bytes",
                limit: MAX_M6_PARTIAL_RERUN_WORKING_BYTES,
                observed: usize::MAX,
            })
        })
    }
    fn derive(
        staleness_assessment_id: StableId,
        target_obligation_id: StableId,
        action: PartialRerunActionKindV5,
        prerequisites: Vec<ActionPrerequisiteV5>,
        stale_source_record_ids: BTreeSet<StableId>,
        reasons: BTreeSet<StaleReasonV5>,
    ) -> M6Result<Self> {
        require_kind(
            &staleness_assessment_id,
            "staleness-assessment-v5",
            "staleness_assessment_id",
        )?;
        require_kind(&target_obligation_id, "obligation", "subject_ids")?;
        bounded(
            prerequisites.len(),
            MAX_M6_ACTION_METADATA_IDS,
            "M6 action prerequisites",
        )?;
        bounded(
            stale_source_record_ids.len(),
            MAX_M6_ACTION_METADATA_IDS,
            "M6 action stale source records",
        )?;
        if prerequisites.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(M6Error::InvalidStalenessAssessment(
                "partial rerun prerequisites are not strictly ordered",
            ));
        }
        for prerequisite in &prerequisites {
            validate_partial_action_prerequisite_v5(prerequisite)?;
        }
        validate_action_prerequisite_shape_v5(action, &prerequisites)?;
        let subject_ids = BTreeSet::from([target_obligation_id]);
        let identity = PartialRerunActionIdentityV5 {
            staleness_assessment_id: &staleness_assessment_id,
            subject_kind: PartialRerunSubjectKindV5::Obligation,
            subject_ids: &subject_ids,
            action,
            prerequisites: &prerequisites,
            stale_source_record_ids: &stale_source_record_ids,
            reasons: &reasons,
        };
        let id = derive("partial-rerun-action-v5", &identity)?;
        let source_ids = std::iter::once(staleness_assessment_id.clone())
            .chain(subject_ids.iter().cloned())
            .chain(stale_source_record_ids.iter().cloned())
            .chain(prerequisites.iter().flat_map(|value| match value {
                ActionPrerequisiteV5::ScheduledAction { action_id } => vec![action_id.clone()],
                ActionPrerequisiteV5::ExistingTargetRecord {
                    record_id,
                    event_id,
                    ..
                } => vec![record_id.clone(), event_id.clone()],
            }))
            .collect::<BTreeSet<_>>();
        bounded(
            source_ids.len(),
            MAX_M6_ACTION_METADATA_IDS,
            "M6 action source IDs",
        )?;
        let value = Self {
            schema: "reviewgraphen.partial_rerun_action.v5",
            id,
            staleness_assessment_id,
            subject_kind: PartialRerunSubjectKindV5::Obligation,
            subject_ids,
            action,
            prerequisites,
            stale_source_record_ids,
            reasons,
            source_ids,
        };
        bounded_event_dto(
            &value,
            MAX_M6_CANONICAL_BYTES,
            "M6 partial rerun action DTO bytes",
        )?;
        Ok(value)
    }

    pub fn from_json_bytes(input: &[u8]) -> M6Result<Self> {
        bounded(
            input.len(),
            MAX_M6_CANONICAL_BYTES,
            "M6 partial rerun action JSON bytes",
        )?;
        preflight_event_line(input.len(), 1)?;
        let wire: PartialRerunActionWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        if wire.schema != "reviewgraphen.partial_rerun_action.v5"
            || wire.subject_kind != PartialRerunSubjectKindV5::Obligation
            || wire.subject_ids.len() != 1
        {
            return Err(M6Error::InvalidWire(
                "invalid partial rerun action shape".to_owned(),
            ));
        }
        let target = wire
            .subject_ids
            .iter()
            .next()
            .expect("singleton checked")
            .clone();
        let expected = Self::derive(
            wire.staleness_assessment_id,
            target,
            wire.action,
            wire.prerequisites,
            wire.stale_source_record_ids,
            wire.reasons,
        )?;
        if expected.id != wire.id
            || expected.source_ids != wire.source_ids
            || crate::canonical_json(&expected)? != input
        {
            return Err(M6Error::InvalidWire(
                "partial rerun action JSON is not exact canonical derived content".to_owned(),
            ));
        }
        Ok(expected)
    }

    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    #[must_use]
    pub fn staleness_assessment_id(&self) -> &StableId {
        &self.staleness_assessment_id
    }
    #[must_use]
    pub const fn action(&self) -> PartialRerunActionKindV5 {
        self.action
    }
    #[must_use]
    pub fn subject_ids(&self) -> &BTreeSet<StableId> {
        &self.subject_ids
    }
    #[must_use]
    pub fn prerequisites(&self) -> &[ActionPrerequisiteV5] {
        &self.prerequisites
    }
    #[must_use]
    pub fn stale_source_record_ids(&self) -> &BTreeSet<StableId> {
        &self.stale_source_record_ids
    }
    #[must_use]
    pub fn reasons(&self) -> &BTreeSet<StaleReasonV5> {
        &self.reasons
    }
    #[must_use]
    pub fn source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }

    /// Event-level authority tests need to exercise the native reducer's
    /// predecessor-witness checks independently of the planner seal.  This
    /// deliberately leaves the derived ID and source set untouched: it is
    /// only suitable for testing a private opaque phase after that phase has
    /// already been minted by the normal planner.
    #[cfg(test)]
    pub(crate) fn replace_prerequisites_unchecked_for_test(
        &mut self,
        prerequisites: Vec<ActionPrerequisiteV5>,
    ) {
        self.prerequisites = prerequisites;
    }

    pub fn body_hash(&self) -> M6Result<ContentHash> {
        body_hash(self)
    }
}

#[derive(Serialize)]
struct PartialRerunPlanIdentityV5<'a> {
    source_closure_id: &'a StableId,
    morphism_id: &'a StableId,
    correspondence_id: &'a StableId,
    staleness_assessment_id: &'a StableId,
    planner_descriptor_id: &'static str,
    target_plan_id: &'a StableId,
    target_gluing_required: bool,
    selected_target_count: u64,
    selected_target_digest: &'a ContentHash,
    action_count: u64,
    action_set_digest: &'a ContentHash,
    preservation_verification_count: u64,
    preservation_verification_digest: &'a ContentHash,
    required_human_resolution_count: u64,
    required_human_resolution_digest: &'a ContentHash,
    source_ids: &'a BTreeSet<StableId>,
}

// Field order is the lexical order produced by `canonical_json`. Keeping this
// borrowing projection separate lets the event admission boundary compare a
// durable plan without decoding it or allocating a canonical JSON buffer.
#[derive(Serialize)]
struct PartialRerunPlanCanonicalV5<'a> {
    action_count: u64,
    action_set_digest: &'a ContentHash,
    correspondence_id: &'a StableId,
    id: &'a StableId,
    morphism_id: &'a StableId,
    planner_descriptor_id: &'static str,
    preservation_verification_count: u64,
    preservation_verification_digest: &'a ContentHash,
    required_human_resolution_count: u64,
    required_human_resolution_digest: &'a ContentHash,
    schema: &'static str,
    selected_target_count: u64,
    selected_target_digest: &'a ContentHash,
    source_closure_id: &'a StableId,
    source_ids: &'a BTreeSet<StableId>,
    staleness_assessment_id: &'a StableId,
    target_gluing_required: bool,
    target_plan_id: &'a StableId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PartialRerunPlanV5 {
    schema: &'static str,
    id: StableId,
    source_closure_id: StableId,
    morphism_id: StableId,
    correspondence_id: StableId,
    staleness_assessment_id: StableId,
    planner_descriptor_id: &'static str,
    target_plan_id: StableId,
    target_gluing_required: bool,
    selected_target_count: u64,
    selected_target_digest: ContentHash,
    action_count: u64,
    action_set_digest: ContentHash,
    preservation_verification_count: u64,
    preservation_verification_digest: ContentHash,
    required_human_resolution_count: u64,
    required_human_resolution_digest: ContentHash,
    source_ids: BTreeSet<StableId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PartialRerunPlanWireV5 {
    schema: String,
    id: StableId,
    source_closure_id: StableId,
    morphism_id: StableId,
    correspondence_id: StableId,
    staleness_assessment_id: StableId,
    planner_descriptor_id: String,
    target_plan_id: StableId,
    target_gluing_required: bool,
    selected_target_count: u64,
    selected_target_digest: ContentHash,
    action_count: u64,
    action_set_digest: ContentHash,
    preservation_verification_count: u64,
    preservation_verification_digest: ContentHash,
    required_human_resolution_count: u64,
    required_human_resolution_digest: ContentHash,
    source_ids: BTreeSet<StableId>,
}

struct PartialRerunPlanPartsV5<'a> {
    source_closure_id: StableId,
    morphism_id: StableId,
    correspondence_id: StableId,
    staleness_assessment_id: StableId,
    target_plan_id: StableId,
    target_gluing_required: bool,
    selected_target_ids: &'a BTreeSet<StableId>,
    actions: &'a [PartialRerunActionV5],
    preservation_verifications: &'a [PreservationVerificationV5],
    required_human_resolution_ids: &'a BTreeSet<StableId>,
}

impl PartialRerunPlanV5 {
    #[must_use]
    pub const fn target_gluing_required(&self) -> bool {
        self.target_gluing_required
    }
    pub(crate) fn canonical_body_matches(&self, input: &[u8]) -> M6Result<bool> {
        crate::canonical::compact_json_eq_streaming(
            &PartialRerunPlanCanonicalV5 {
                action_count: self.action_count,
                action_set_digest: &self.action_set_digest,
                correspondence_id: &self.correspondence_id,
                id: &self.id,
                morphism_id: &self.morphism_id,
                planner_descriptor_id: self.planner_descriptor_id,
                preservation_verification_count: self.preservation_verification_count,
                preservation_verification_digest: &self.preservation_verification_digest,
                required_human_resolution_count: self.required_human_resolution_count,
                required_human_resolution_digest: &self.required_human_resolution_digest,
                schema: self.schema,
                selected_target_count: self.selected_target_count,
                selected_target_digest: &self.selected_target_digest,
                source_closure_id: &self.source_closure_id,
                source_ids: &self.source_ids,
                staleness_assessment_id: &self.staleness_assessment_id,
                target_plan_id: &self.target_plan_id,
                target_gluing_required: self.target_gluing_required,
            },
            input,
        )
        .map_err(Into::into)
    }

    pub(crate) fn retained_bytes(&self) -> M6Result<usize> {
        [
            std::mem::size_of::<Self>(),
            self.id.allocated_bytes(),
            self.source_closure_id.allocated_bytes(),
            self.morphism_id.allocated_bytes(),
            self.correspondence_id.allocated_bytes(),
            self.staleness_assessment_id.allocated_bytes(),
            self.target_plan_id.allocated_bytes(),
            self.selected_target_digest.allocated_bytes(),
            self.action_set_digest.allocated_bytes(),
            self.preservation_verification_digest.allocated_bytes(),
            self.required_human_resolution_digest.allocated_bytes(),
            conservative_id_set_heap(&self.source_ids)?,
        ]
        .into_iter()
        .try_fold(0_usize, |total, value| {
            total.checked_add(value).ok_or(M6Error::Incomplete {
                operation: "M6 partial plan retained bytes",
                limit: MAX_M6_PARTIAL_RERUN_WORKING_BYTES,
                observed: usize::MAX,
            })
        })
    }
    fn seal(parts: PartialRerunPlanPartsV5<'_>) -> M6Result<Self> {
        require_kind(
            &parts.source_closure_id,
            "incremental-source-closure-v5",
            "source_closure_id",
        )?;
        require_kind(&parts.morphism_id, "change-morphism-v5", "morphism_id")?;
        require_kind(
            &parts.correspondence_id,
            "obligation-correspondence-v5",
            "correspondence_id",
        )?;
        require_kind(
            &parts.staleness_assessment_id,
            "staleness-assessment-v5",
            "staleness_assessment_id",
        )?;
        require_kind(&parts.target_plan_id, "plan", "target_plan_id")?;
        bounded(
            parts.selected_target_ids.len(),
            MAX_M6_OBLIGATIONS_PER_UNIVERSE,
            "M6 selected rerun targets",
        )?;
        bounded(
            parts.actions.len(),
            MAX_M6_PARTIAL_RERUN_ACTIONS,
            "M6 partial rerun actions",
        )?;
        bounded(
            parts.preservation_verifications.len(),
            MAX_M6_PRESERVATION_RECORDS,
            "M6 preservation verifications",
        )?;
        bounded(
            parts.required_human_resolution_ids.len(),
            MAX_M6_OBLIGATIONS_PER_UNIVERSE,
            "M6 required human resolutions",
        )?;
        if parts
            .selected_target_ids
            .iter()
            .any(|id| id.kind() != "obligation")
            || !parts
                .required_human_resolution_ids
                .is_subset(parts.selected_target_ids)
            || parts
                .actions
                .windows(2)
                .any(|pair| pair[0].id >= pair[1].id)
            || parts
                .preservation_verifications
                .windows(2)
                .any(|pair| pair[0].id >= pair[1].id)
            || parts.actions.iter().any(|action| {
                action.staleness_assessment_id != parts.staleness_assessment_id
                    || !action.subject_ids.is_subset(parts.selected_target_ids)
            })
        {
            return Err(M6Error::InvalidStalenessAssessment(
                "partial rerun plan members are not exact, ordered, or subject-bound",
            ));
        }
        let action_by_id = parts
            .actions
            .iter()
            .map(|action| (&action.id, action))
            .collect::<BTreeMap<_, _>>();
        for action in parts.actions {
            let expected_predecessor = match action.action {
                PartialRerunActionKindV5::ReprojectContext => None,
                PartialRerunActionKindV5::RerunReviewer => {
                    Some(PartialRerunActionKindV5::ReprojectContext)
                }
                PartialRerunActionKindV5::RerunVerifier => {
                    Some(PartialRerunActionKindV5::RerunReviewer)
                }
                PartialRerunActionKindV5::RerunHumanDecision => {
                    Some(PartialRerunActionKindV5::RerunVerifier)
                }
            };
            for prerequisite in &action.prerequisites {
                let ActionPrerequisiteV5::ScheduledAction { action_id } = prerequisite else {
                    continue;
                };
                let Some(predecessor) = action_by_id.get(action_id) else {
                    return Err(M6Error::InvalidStalenessAssessment(
                        "partial rerun scheduled prerequisite is dangling",
                    ));
                };
                if Some(predecessor.action) != expected_predecessor
                    || predecessor.subject_ids != action.subject_ids
                    || predecessor.staleness_assessment_id != action.staleness_assessment_id
                {
                    return Err(M6Error::InvalidStalenessAssessment(
                        "partial rerun scheduled prerequisite is not the same-subject expected predecessor",
                    ));
                }
            }
        }
        let action_records = parts
            .actions
            .iter()
            .map(|value| {
                value
                    .body_hash()
                    .and_then(|hash| IdBodyHashV5::new(value.id.clone(), hash))
            })
            .collect::<M6Result<Vec<_>>>()?;
        let preservation_records = parts
            .preservation_verifications
            .iter()
            .map(|value| {
                value
                    .body_hash()
                    .and_then(|hash| IdBodyHashV5::new(value.id.clone(), hash))
            })
            .collect::<M6Result<Vec<_>>>()?;
        let selected_target_digest = digest_ids(parts.selected_target_ids)?;
        let action_set_digest = digest_records(&action_records)?;
        let preservation_verification_digest = digest_records(&preservation_records)?;
        let required_human_resolution_digest = digest_ids(parts.required_human_resolution_ids)?;
        let source_ids = BTreeSet::from([
            parts.source_closure_id.clone(),
            parts.morphism_id.clone(),
            parts.correspondence_id.clone(),
            parts.staleness_assessment_id.clone(),
            parts.target_plan_id.clone(),
        ]);
        let mut value = Self {
            schema: "reviewgraphen.partial_rerun_plan.v5",
            id: StableId::parse("partial-rerun-plan-v5:pending")?,
            source_closure_id: parts.source_closure_id,
            morphism_id: parts.morphism_id,
            correspondence_id: parts.correspondence_id,
            staleness_assessment_id: parts.staleness_assessment_id,
            planner_descriptor_id: PARTIAL_RERUN_PLANNER_DESCRIPTOR_V5,
            target_plan_id: parts.target_plan_id,
            target_gluing_required: parts.target_gluing_required,
            selected_target_count: u64::try_from(parts.selected_target_ids.len()).map_err(
                |_| M6Error::Incomplete {
                    operation: "M6 selected target count",
                    limit: MAX_M6_OBLIGATIONS_PER_UNIVERSE,
                    observed: usize::MAX,
                },
            )?,
            selected_target_digest,
            action_count: u64::try_from(parts.actions.len()).map_err(|_| M6Error::Incomplete {
                operation: "M6 action count",
                limit: MAX_M6_PARTIAL_RERUN_ACTIONS,
                observed: usize::MAX,
            })?,
            action_set_digest,
            preservation_verification_count: u64::try_from(parts.preservation_verifications.len())
                .map_err(|_| M6Error::Incomplete {
                    operation: "M6 preservation verification count",
                    limit: MAX_M6_PRESERVATION_RECORDS,
                    observed: usize::MAX,
                })?,
            preservation_verification_digest,
            required_human_resolution_count: u64::try_from(
                parts.required_human_resolution_ids.len(),
            )
            .map_err(|_| M6Error::Incomplete {
                operation: "M6 required human count",
                limit: MAX_M6_OBLIGATIONS_PER_UNIVERSE,
                observed: usize::MAX,
            })?,
            required_human_resolution_digest,
            source_ids,
        };
        value.id = derive("partial-rerun-plan-v5", &value.identity())?;
        bounded_event_dto(
            &value,
            MAX_M6_CANONICAL_BYTES,
            "M6 partial rerun plan DTO bytes",
        )?;
        Ok(value)
    }

    fn identity(&self) -> PartialRerunPlanIdentityV5<'_> {
        PartialRerunPlanIdentityV5 {
            source_closure_id: &self.source_closure_id,
            morphism_id: &self.morphism_id,
            correspondence_id: &self.correspondence_id,
            staleness_assessment_id: &self.staleness_assessment_id,
            planner_descriptor_id: self.planner_descriptor_id,
            target_plan_id: &self.target_plan_id,
            target_gluing_required: self.target_gluing_required,
            selected_target_count: self.selected_target_count,
            selected_target_digest: &self.selected_target_digest,
            action_count: self.action_count,
            action_set_digest: &self.action_set_digest,
            preservation_verification_count: self.preservation_verification_count,
            preservation_verification_digest: &self.preservation_verification_digest,
            required_human_resolution_count: self.required_human_resolution_count,
            required_human_resolution_digest: &self.required_human_resolution_digest,
            source_ids: &self.source_ids,
        }
    }

    pub fn from_json_bytes(input: &[u8], expected: &Self) -> M6Result<Self> {
        bounded(
            input.len(),
            MAX_M6_CANONICAL_BYTES,
            "M6 partial rerun plan JSON bytes",
        )?;
        preflight_event_line(input.len(), 1)?;
        let wire: PartialRerunPlanWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        for (field, digest) in [
            ("selected_target_digest", &wire.selected_target_digest),
            ("action_set_digest", &wire.action_set_digest),
            (
                "preservation_verification_digest",
                &wire.preservation_verification_digest,
            ),
            (
                "required_human_resolution_digest",
                &wire.required_human_resolution_digest,
            ),
        ] {
            full_sha256(field, digest)?;
        }
        let source_ids = BTreeSet::from([
            wire.source_closure_id.clone(),
            wire.morphism_id.clone(),
            wire.correspondence_id.clone(),
            wire.staleness_assessment_id.clone(),
            wire.target_plan_id.clone(),
        ]);
        let value = Self {
            schema: "reviewgraphen.partial_rerun_plan.v5",
            id: wire.id,
            source_closure_id: wire.source_closure_id,
            morphism_id: wire.morphism_id,
            correspondence_id: wire.correspondence_id,
            staleness_assessment_id: wire.staleness_assessment_id,
            planner_descriptor_id: PARTIAL_RERUN_PLANNER_DESCRIPTOR_V5,
            target_plan_id: wire.target_plan_id,
            target_gluing_required: wire.target_gluing_required,
            selected_target_count: wire.selected_target_count,
            selected_target_digest: wire.selected_target_digest,
            action_count: wire.action_count,
            action_set_digest: wire.action_set_digest,
            preservation_verification_count: wire.preservation_verification_count,
            preservation_verification_digest: wire.preservation_verification_digest,
            required_human_resolution_count: wire.required_human_resolution_count,
            required_human_resolution_digest: wire.required_human_resolution_digest,
            source_ids,
        };
        if wire.schema != value.schema
            || wire.planner_descriptor_id != PARTIAL_RERUN_PLANNER_DESCRIPTOR_V5
            || wire.source_ids != value.source_ids
            || value.id != derive("partial-rerun-plan-v5", &value.identity())?
            || value.selected_target_count > MAX_M6_OBLIGATIONS_PER_UNIVERSE as u64
            || value.action_count > MAX_M6_PARTIAL_RERUN_ACTIONS as u64
            || value.preservation_verification_count > MAX_M6_PRESERVATION_RECORDS as u64
            || value.required_human_resolution_count > value.selected_target_count
            || &value != expected
            || crate::canonical_json(&value)? != input
        {
            return Err(M6Error::InvalidWire(
                "partial rerun plan JSON is not exact canonical derived content".to_owned(),
            ));
        }
        Ok(value)
    }

    /// Validates a closed event body before the event layer compares it with
    /// the opaque roots/CAS-derived phase.  This deliberately proves only the
    /// DTO's own identity and canonical form; action-set equality belongs to
    /// the phase admission boundary and cannot be supplied by event JSON.
    pub(crate) fn from_event_json_bytes(input: &[u8]) -> M6Result<Self> {
        bounded(
            input.len(),
            MAX_M6_CANONICAL_BYTES,
            "M6 partial rerun plan event JSON bytes",
        )?;
        preflight_event_line(input.len(), 1)?;
        let wire: PartialRerunPlanWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        for (field, digest) in [
            ("selected_target_digest", &wire.selected_target_digest),
            ("action_set_digest", &wire.action_set_digest),
            (
                "preservation_verification_digest",
                &wire.preservation_verification_digest,
            ),
            (
                "required_human_resolution_digest",
                &wire.required_human_resolution_digest,
            ),
        ] {
            full_sha256(field, digest)?;
        }
        let source_ids = BTreeSet::from([
            wire.source_closure_id.clone(),
            wire.morphism_id.clone(),
            wire.correspondence_id.clone(),
            wire.staleness_assessment_id.clone(),
            wire.target_plan_id.clone(),
        ]);
        let value = Self {
            schema: "reviewgraphen.partial_rerun_plan.v5",
            id: wire.id,
            source_closure_id: wire.source_closure_id,
            morphism_id: wire.morphism_id,
            correspondence_id: wire.correspondence_id,
            staleness_assessment_id: wire.staleness_assessment_id,
            planner_descriptor_id: PARTIAL_RERUN_PLANNER_DESCRIPTOR_V5,
            target_plan_id: wire.target_plan_id,
            target_gluing_required: wire.target_gluing_required,
            selected_target_count: wire.selected_target_count,
            selected_target_digest: wire.selected_target_digest,
            action_count: wire.action_count,
            action_set_digest: wire.action_set_digest,
            preservation_verification_count: wire.preservation_verification_count,
            preservation_verification_digest: wire.preservation_verification_digest,
            required_human_resolution_count: wire.required_human_resolution_count,
            required_human_resolution_digest: wire.required_human_resolution_digest,
            source_ids,
        };
        if wire.schema != value.schema
            || wire.planner_descriptor_id != PARTIAL_RERUN_PLANNER_DESCRIPTOR_V5
            || wire.source_ids != value.source_ids
            || value.id != derive("partial-rerun-plan-v5", &value.identity())?
            || value.selected_target_count > MAX_M6_OBLIGATIONS_PER_UNIVERSE as u64
            || value.action_count > MAX_M6_PARTIAL_RERUN_ACTIONS as u64
            || value.preservation_verification_count > MAX_M6_PRESERVATION_RECORDS as u64
            || value.required_human_resolution_count > value.selected_target_count
            || crate::canonical_json(&value)? != input
        {
            return Err(M6Error::InvalidWire(
                "partial rerun plan event JSON is not exact canonical derived content".to_owned(),
            ));
        }
        Ok(value)
    }

    pub(crate) fn validate_event_wire(input: &[u8]) -> M6Result<()> {
        Self::from_event_json_bytes(input).map(|_| ())
    }

    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    /// Read-only provenance of this derived plan.  This exposes no planning
    /// or append authority and lets the terminal proof verifier bind a sealed
    /// plan to the exact source-closure event that carried it.
    #[must_use]
    pub fn source_closure_id(&self) -> &StableId {
        &self.source_closure_id
    }
    #[must_use]
    pub fn morphism_id(&self) -> &StableId {
        &self.morphism_id
    }
    #[must_use]
    pub fn correspondence_id(&self) -> &StableId {
        &self.correspondence_id
    }
    #[must_use]
    pub fn staleness_assessment_id(&self) -> &StableId {
        &self.staleness_assessment_id
    }
    #[must_use]
    pub fn target_plan_id(&self) -> &StableId {
        &self.target_plan_id
    }
    #[must_use]
    pub const fn action_count(&self) -> u64 {
        self.action_count
    }
    #[must_use]
    pub const fn selected_target_count(&self) -> u64 {
        self.selected_target_count
    }
    #[must_use]
    pub fn selected_target_digest(&self) -> &ContentHash {
        &self.selected_target_digest
    }
    #[must_use]
    pub const fn preservation_verification_count(&self) -> u64 {
        self.preservation_verification_count
    }
    #[must_use]
    pub fn preservation_verification_digest(&self) -> &ContentHash {
        &self.preservation_verification_digest
    }
    #[must_use]
    pub const fn required_human_resolution_count(&self) -> u64 {
        self.required_human_resolution_count
    }
    #[must_use]
    pub fn action_set_digest(&self) -> &ContentHash {
        &self.action_set_digest
    }
    #[must_use]
    pub fn required_human_resolution_digest(&self) -> &ContentHash {
        &self.required_human_resolution_digest
    }
    #[must_use]
    pub fn source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
    pub fn body_hash(&self) -> M6Result<ContentHash> {
        body_hash(self)
    }
}

/// The only two M5 contexts, in the fixed order used by the post-D2 seal.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GluingClaimBindingStatusV5 {
    Selected,
    Missing,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GluingClaimBindingV5 {
    context_id: StableId,
    status: GluingClaimBindingStatusV5,
    claim_id: Option<StableId>,
    claim_body_hash: Option<ContentHash>,
    obligation_id: Option<StableId>,
}

impl<'de> Deserialize<'de> for GluingClaimBindingV5 {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            context_id: StableId,
            status: GluingClaimBindingStatusV5,
            claim_id: Option<StableId>,
            claim_body_hash: Option<ContentHash>,
            obligation_id: Option<StableId>,
        }
        let wire = Wire::deserialize(deserializer)?;
        let selected = match (wire.claim_id, wire.claim_body_hash, wire.obligation_id) {
            (None, None, None) => None,
            (Some(claim_id), Some(claim_body_hash), Some(obligation_id)) => {
                Some(((claim_id, claim_body_hash), obligation_id))
            }
            _ => {
                return Err(serde::de::Error::custom(
                    "gluing claim binding option fields must be all present or all absent",
                ));
            }
        };
        Self::new(wire.context_id, wire.status, selected).map_err(serde::de::Error::custom)
    }
}

impl GluingClaimBindingV5 {
    pub(crate) fn new(
        context_id: StableId,
        status: GluingClaimBindingStatusV5,
        selected: Option<((StableId, ContentHash), StableId)>,
    ) -> M6Result<Self> {
        if context_id.as_str() != crate::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID
            && context_id.as_str() != crate::DOUBLE_SUBMIT_UI_CONTEXT_ID
        {
            return Err(M6Error::Canonical(
                "gluing claim binding context is outside the fixed M5 cover".to_owned(),
            ));
        }
        let (claim_id, claim_body_hash, obligation_id) = match (status, selected) {
            (GluingClaimBindingStatusV5::Missing, None) => (None, None, None),
            (
                GluingClaimBindingStatusV5::Selected,
                Some(((claim_id, body_hash), obligation_id)),
            ) => {
                require_kind(&claim_id, "claim", "claim_id")?;
                require_kind(&obligation_id, "obligation", "obligation_id")?;
                full_sha256("claim_body_hash", &body_hash)?;
                (Some(claim_id), Some(body_hash), Some(obligation_id))
            }
            _ => {
                return Err(M6Error::Canonical(
                    "gluing claim binding status and optional selection differ".to_owned(),
                ));
            }
        };
        Ok(Self {
            context_id,
            status,
            claim_id,
            claim_body_hash,
            obligation_id,
        })
    }

    #[must_use]
    pub fn context_id(&self) -> &StableId {
        &self.context_id
    }
    #[must_use]
    pub const fn status(&self) -> GluingClaimBindingStatusV5 {
        self.status
    }
    #[must_use]
    pub fn claim_id(&self) -> Option<&StableId> {
        self.claim_id.as_ref()
    }
    #[must_use]
    pub fn claim_body_hash(&self) -> Option<&ContentHash> {
        self.claim_body_hash.as_ref()
    }
    #[must_use]
    pub fn obligation_id(&self) -> Option<&StableId> {
        self.obligation_id.as_ref()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GluingRerunSubjectKindV5 {
    GluingContext,
    GluingAttempt,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GluingRerunActionKindV5 {
    RegisterGluingInput,
    RebuildSection,
    Reglue,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GluingRerunReasonV5 {
    FreshTargetGluingRequired,
}

#[derive(Serialize)]
struct GluingRerunActionIdentityV5<'a> {
    planning_scope_id: &'a StableId,
    subject_kind: GluingRerunSubjectKindV5,
    subject_ids: &'a BTreeSet<StableId>,
    action: GluingRerunActionKindV5,
    prerequisites: &'a [ActionPrerequisiteV5],
    reasons: &'a BTreeSet<GluingRerunReasonV5>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GluingRerunActionV5 {
    schema: &'static str,
    id: StableId,
    planning_scope_id: StableId,
    subject_kind: GluingRerunSubjectKindV5,
    subject_ids: BTreeSet<StableId>,
    action: GluingRerunActionKindV5,
    prerequisites: Vec<ActionPrerequisiteV5>,
    reasons: BTreeSet<GluingRerunReasonV5>,
    source_ids: BTreeSet<StableId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GluingRerunActionWireV5 {
    schema: String,
    id: StableId,
    planning_scope_id: StableId,
    subject_kind: GluingRerunSubjectKindV5,
    subject_ids: BTreeSet<StableId>,
    action: GluingRerunActionKindV5,
    prerequisites: Vec<ActionPrerequisiteV5>,
    reasons: BTreeSet<GluingRerunReasonV5>,
    source_ids: BTreeSet<StableId>,
}

impl GluingRerunActionV5 {
    /// Exact recursive ownership used by the event-layer post-D2 append
    /// oracle.  Keeping this beside the DTO prevents the opaque phase from
    /// estimating its retained action bodies via serialization.
    pub(crate) fn retained_bytes(&self) -> M6Result<usize> {
        let prerequisite_bytes = self
            .prerequisites
            .iter()
            .try_fold(0_usize, |total, value| {
                let owned = match value {
                    ActionPrerequisiteV5::ScheduledAction { action_id } => {
                        action_id.allocated_bytes()
                    }
                    ActionPrerequisiteV5::ExistingTargetRecord {
                        record_id,
                        body_hash,
                        event_id,
                    } => record_id
                        .allocated_bytes()
                        .checked_add(body_hash.allocated_bytes())
                        .and_then(|value| value.checked_add(event_id.allocated_bytes()))
                        .ok_or(M6Error::Incomplete {
                            operation: "M6 gluing action prerequisite ownership",
                            limit: MAX_M6_CANONICAL_BYTES,
                            observed: usize::MAX,
                        })?,
                };
                total.checked_add(owned).ok_or(M6Error::Incomplete {
                    operation: "M6 gluing action prerequisite ownership",
                    limit: MAX_M6_CANONICAL_BYTES,
                    observed: usize::MAX,
                })
            })?;
        [
            std::mem::size_of::<Self>(),
            self.id.allocated_bytes(),
            self.planning_scope_id.allocated_bytes(),
            conservative_id_set_heap(&self.subject_ids)?,
            self.prerequisites
                .capacity()
                .checked_mul(std::mem::size_of::<ActionPrerequisiteV5>())
                .ok_or(M6Error::Incomplete {
                    operation: "M6 gluing action prerequisite slots",
                    limit: MAX_M6_CANONICAL_BYTES,
                    observed: usize::MAX,
                })?,
            prerequisite_bytes,
            conservative_btree_node_bytes(self.reasons.len())?
                .checked_add(
                    self.reasons
                        .len()
                        .checked_mul(std::mem::size_of::<GluingRerunReasonV5>())
                        .ok_or(M6Error::Incomplete {
                            operation: "M6 gluing action reason ownership",
                            limit: MAX_M6_CANONICAL_BYTES,
                            observed: usize::MAX,
                        })?,
                )
                .ok_or(M6Error::Incomplete {
                    operation: "M6 gluing action reason ownership",
                    limit: MAX_M6_CANONICAL_BYTES,
                    observed: usize::MAX,
                })?,
            conservative_id_set_heap(&self.source_ids)?,
        ]
        .into_iter()
        .try_fold(0_usize, |total, value| {
            total.checked_add(value).ok_or(M6Error::Incomplete {
                operation: "M6 gluing action retained bytes",
                limit: MAX_M6_CANONICAL_BYTES,
                observed: usize::MAX,
            })
        })
    }

    pub(crate) fn derive(
        planning_scope_id: StableId,
        subject_kind: GluingRerunSubjectKindV5,
        subject_id: StableId,
        action: GluingRerunActionKindV5,
        prerequisites: Vec<ActionPrerequisiteV5>,
    ) -> M6Result<Self> {
        require_kind(
            &planning_scope_id,
            "gluing-rerun-scope-v5",
            "planning_scope_id",
        )?;
        if prerequisites.len() > MAX_M6_GLUING_PREREQUISITES
            || prerequisites.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(M6Error::Canonical(
                "gluing action prerequisites are not a bounded strict set".to_owned(),
            ));
        }
        for prerequisite in &prerequisites {
            validate_gluing_action_prerequisite_v5(action, prerequisite)?;
        }
        let expected = match action {
            GluingRerunActionKindV5::RegisterGluingInput => {
                subject_kind == GluingRerunSubjectKindV5::GluingContext
                    && prerequisites.is_empty()
                    && (subject_id.as_str() == crate::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID
                        || subject_id.as_str() == crate::DOUBLE_SUBMIT_UI_CONTEXT_ID)
            }
            GluingRerunActionKindV5::RebuildSection => {
                subject_kind == GluingRerunSubjectKindV5::GluingContext
                    && (subject_id.as_str() == crate::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID
                        || subject_id.as_str() == crate::DOUBLE_SUBMIT_UI_CONTEXT_ID)
                    && (2..=3).contains(&prerequisites.len())
            }
            GluingRerunActionKindV5::Reglue => {
                subject_kind == GluingRerunSubjectKindV5::GluingAttempt
                    && subject_id.as_str() == crate::DOUBLE_SUBMIT_INVARIANT_ID
                    && (2..=MAX_M6_GLUING_PREREQUISITES).contains(&prerequisites.len())
            }
        };
        if !expected {
            return Err(M6Error::Canonical(
                "gluing action/subject/prerequisite shape is outside the closed DAG".to_owned(),
            ));
        }
        let subject_ids = BTreeSet::from([subject_id]);
        let reasons = BTreeSet::from([GluingRerunReasonV5::FreshTargetGluingRequired]);
        let identity = GluingRerunActionIdentityV5 {
            planning_scope_id: &planning_scope_id,
            subject_kind,
            subject_ids: &subject_ids,
            action,
            prerequisites: &prerequisites,
            reasons: &reasons,
        };
        let id = derive("gluing-rerun-action-v5", &identity)?;
        let source_ids = std::iter::once(planning_scope_id.clone())
            .chain(subject_ids.iter().cloned())
            .chain(prerequisites.iter().flat_map(|value| match value {
                ActionPrerequisiteV5::ScheduledAction { action_id } => vec![action_id.clone()],
                ActionPrerequisiteV5::ExistingTargetRecord {
                    record_id,
                    event_id,
                    ..
                } => {
                    vec![record_id.clone(), event_id.clone()]
                }
            }))
            .collect::<BTreeSet<_>>();
        let value = Self {
            schema: "reviewgraphen.gluing_rerun_action.v5",
            id,
            planning_scope_id,
            subject_kind,
            subject_ids,
            action,
            prerequisites,
            reasons,
            source_ids,
        };
        bounded_event_dto(
            &value,
            MAX_M6_CANONICAL_BYTES,
            "M6 gluing rerun action DTO bytes",
        )?;
        Ok(value)
    }

    pub(crate) fn from_event_json_bytes(input: &[u8]) -> M6Result<Self> {
        bounded(
            input.len(),
            MAX_M6_CANONICAL_BYTES,
            "M6 gluing rerun action JSON bytes",
        )?;
        preflight_event_line(input.len(), 1)?;
        let wire: GluingRerunActionWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        if wire.schema != "reviewgraphen.gluing_rerun_action.v5" || wire.subject_ids.len() != 1 {
            return Err(M6Error::InvalidWire(
                "invalid gluing rerun action shape".to_owned(),
            ));
        }
        let value = Self::derive(
            wire.planning_scope_id,
            wire.subject_kind,
            wire.subject_ids
                .iter()
                .next()
                .expect("checked singleton")
                .clone(),
            wire.action,
            wire.prerequisites,
        )?;
        if value.id != wire.id
            || value.reasons != wire.reasons
            || value.source_ids != wire.source_ids
            || crate::canonical_json(&value)? != input
        {
            return Err(M6Error::InvalidWire(
                "gluing rerun action is not exact derived content".to_owned(),
            ));
        }
        Ok(value)
    }
    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    #[must_use]
    pub fn planning_scope_id(&self) -> &StableId {
        &self.planning_scope_id
    }
    #[must_use]
    pub const fn subject_kind(&self) -> GluingRerunSubjectKindV5 {
        self.subject_kind
    }
    #[must_use]
    pub const fn action(&self) -> GluingRerunActionKindV5 {
        self.action
    }
    #[must_use]
    pub fn subject_ids(&self) -> &BTreeSet<StableId> {
        &self.subject_ids
    }
    #[must_use]
    pub fn prerequisites(&self) -> &[ActionPrerequisiteV5] {
        &self.prerequisites
    }
    #[must_use]
    pub fn reasons(&self) -> &BTreeSet<GluingRerunReasonV5> {
        &self.reasons
    }
    #[must_use]
    pub fn source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
    pub fn body_hash(&self) -> M6Result<ContentHash> {
        body_hash(self)
    }
}

/// Validates the entire second-plan DAG before its seal is derived.  Local
/// DTO validation intentionally cannot prove that a scheduled prerequisite is
/// in this plan, or that it is an immediate legal predecessor.
fn validate_gluing_action_dag_v5(
    planning_scope_id: &StableId,
    bindings: &[GluingClaimBindingV5; 2],
    actions: &[GluingRerunActionV5],
) -> M6Result<()> {
    let by_id = actions
        .iter()
        .map(|action| (action.id(), action))
        .collect::<BTreeMap<_, _>>();
    if by_id.len() != actions.len() || actions.windows(2).any(|pair| pair[0].id >= pair[1].id) {
        return Err(M6Error::Canonical(
            "gluing action IDs must be a strict whole-plan order".to_owned(),
        ));
    }
    if actions.is_empty() {
        return Ok(());
    }
    let mut remaining = BTreeMap::<StableId, usize>::new();
    let mut reverse = BTreeMap::<StableId, Vec<StableId>>::new();
    for action in actions {
        if action.planning_scope_id != *planning_scope_id {
            return Err(M6Error::Canonical(
                "gluing action escapes the sealed planning scope".to_owned(),
            ));
        }
        let mut scheduled = BTreeSet::new();
        for prerequisite in &action.prerequisites {
            if let ActionPrerequisiteV5::ScheduledAction { action_id } = prerequisite
                && action_id.kind() == "gluing-rerun-action-v5"
            {
                if !by_id.contains_key(action_id) || !scheduled.insert(action_id.clone()) {
                    return Err(M6Error::Canonical(
                        "gluing action has a dangling or duplicate scheduled prerequisite"
                            .to_owned(),
                    ));
                }
                reverse
                    .entry(action_id.clone())
                    .or_default()
                    .push(action.id.clone());
            }
        }
        remaining.insert(action.id.clone(), scheduled.len());
    }

    let registration_for = |context_id: &StableId| {
        actions.iter().find(|action| {
            action.action == GluingRerunActionKindV5::RegisterGluingInput
                && action.subject_ids.contains(context_id)
        })
    };
    let rebuild_for = |context_id: &StableId| {
        actions.iter().find(|action| {
            action.action == GluingRerunActionKindV5::RebuildSection
                && action.subject_ids.contains(context_id)
        })
    };
    let existing_of_kind = |action: &GluingRerunActionV5, kind: &str| {
        action
            .prerequisites
            .iter()
            .filter(|value| {
                matches!(value, ActionPrerequisiteV5::ExistingTargetRecord { record_id, .. }
                    if record_id.kind() == kind)
            })
            .cloned()
            .collect::<BTreeSet<_>>()
    };
    let scheduled_of_kind = |action: &GluingRerunActionV5, kind: &str| {
        action
            .prerequisites
            .iter()
            .filter_map(|value| match value {
                ActionPrerequisiteV5::ScheduledAction { action_id } if action_id.kind() == kind => {
                    Some(action_id.clone())
                }
                _ => None,
            })
            .collect::<BTreeSet<_>>()
    };

    let reglue = actions
        .iter()
        .filter(|action| action.action == GluingRerunActionKindV5::Reglue)
        .collect::<Vec<_>>();
    if reglue.len() != 1 {
        return Err(M6Error::Canonical(
            "a nonempty gluing DAG requires exactly one reglue action".to_owned(),
        ));
    }
    let reglue = reglue[0];
    let mut expected_reglue = BTreeSet::new();
    let mut unassigned_existing = reglue
        .prerequisites
        .iter()
        .filter(|value| matches!(value, ActionPrerequisiteV5::ExistingTargetRecord { .. }))
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut missing_existing_registration_pairs = 0_usize;

    for binding in bindings {
        let registration = registration_for(binding.context_id());
        let rebuild = rebuild_for(binding.context_id());
        if (binding.status == GluingClaimBindingStatusV5::Selected) != rebuild.is_some() {
            return Err(M6Error::Canonical(
                "gluing binding does not have its exact context rebuild".to_owned(),
            ));
        }
        let context_registration = if let Some(registration) = registration {
            let prerequisite = ActionPrerequisiteV5::scheduled(registration.id.clone())?;
            expected_reglue.insert(prerequisite.clone());
            BTreeSet::from([prerequisite])
        } else if let Some(rebuild) = rebuild {
            let pair = rebuild
                .prerequisites
                .iter()
                .filter(|value| {
                    matches!(value, ActionPrerequisiteV5::ExistingTargetRecord { record_id, .. }
                        if matches!(record_id.kind(), "gluing-input-descriptor-v4" | "registration-v4"))
                })
                .cloned()
                .collect::<BTreeSet<_>>();
            if pair.len() != 2
                || pair
                    .iter()
                    .filter(|value| matches!(value, ActionPrerequisiteV5::ExistingTargetRecord { record_id, .. } if record_id.kind() == "gluing-input-descriptor-v4"))
                    .count()
                    != 1
                || pair
                    .iter()
                    .filter(|value| matches!(value, ActionPrerequisiteV5::ExistingTargetRecord { record_id, .. } if record_id.kind() == "registration-v4"))
                    .count()
                    != 1
            {
                return Err(M6Error::Canonical(
                    "gluing rebuild lacks its exact existing descriptor/registration pair"
                        .to_owned(),
                ));
            }
            expected_reglue.extend(pair.iter().cloned());
            for prerequisite in &pair {
                unassigned_existing.remove(prerequisite);
            }
            pair
        } else {
            // A Missing context has no rebuild to carry the pair. Its exact
            // descriptor/registration pair is the remaining pair on reglue.
            missing_existing_registration_pairs = missing_existing_registration_pairs
                .checked_add(1)
                .ok_or(M6Error::Incomplete {
                    operation: "M6 missing gluing registration pairs",
                    limit: 2,
                    observed: usize::MAX,
                })?;
            BTreeSet::new()
        };

        if let Some(rebuild) = rebuild {
            let scheduled_registrations = scheduled_of_kind(rebuild, "gluing-rerun-action-v5")
                .into_iter()
                .map(|action_id| ActionPrerequisiteV5::ScheduledAction { action_id })
                .collect::<BTreeSet<_>>();
            let existing_registration = existing_of_kind(rebuild, "registration-v4")
                .into_iter()
                .chain(existing_of_kind(rebuild, "gluing-input-descriptor-v4"))
                .collect::<BTreeSet<_>>();
            if (registration.is_some() && scheduled_registrations != context_registration)
                || (registration.is_none() && existing_registration != context_registration)
            {
                return Err(M6Error::Canonical(
                    "gluing rebuild registration closure crosses contexts".to_owned(),
                ));
            }
            let scheduled_verifier = scheduled_of_kind(rebuild, "partial-rerun-action-v5");
            let existing_verifier = existing_of_kind(rebuild, "verification");
            if scheduled_verifier.len() + existing_verifier.len() != 1
                || rebuild.prerequisites.len() != context_registration.len() + 1
            {
                return Err(M6Error::Canonical(
                    "gluing rebuild must have one exact verifier for its bound obligation"
                        .to_owned(),
                ));
            }
            // This inert DTO contract has no first-plan action set or native
            // verification metadata. It closes the one verifier-shaped slot;
            // the event/authority integration must prove that slot addresses
            // this binding's exact obligation before accepting the seal.
            expected_reglue.insert(ActionPrerequisiteV5::scheduled(rebuild.id.clone())?);
        }
    }

    if !unassigned_existing.is_empty() || missing_existing_registration_pairs != 0 {
        let descriptors = unassigned_existing
            .iter()
            .filter(|value| matches!(value, ActionPrerequisiteV5::ExistingTargetRecord { record_id, .. } if record_id.kind() == "gluing-input-descriptor-v4"))
            .count();
        let registrations = unassigned_existing
            .iter()
            .filter(|value| matches!(value, ActionPrerequisiteV5::ExistingTargetRecord { record_id, .. } if record_id.kind() == "registration-v4"))
            .count();
        if descriptors != missing_existing_registration_pairs
            || registrations != missing_existing_registration_pairs
            || unassigned_existing.len() != missing_existing_registration_pairs * 2
        {
            return Err(M6Error::Canonical(
                "reglue has an unbound existing registration closure".to_owned(),
            ));
        }
        expected_reglue.extend(unassigned_existing);
    }
    if reglue
        .prerequisites
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        != expected_reglue
        || reglue.prerequisites.len() != expected_reglue.len()
    {
        return Err(M6Error::Canonical(
            "reglue prerequisites are not the exact two-context closure".to_owned(),
        ));
    }
    let mut ready = remaining
        .iter()
        .filter_map(|(id, count)| (*count == 0).then_some((*id).clone()))
        .collect::<BTreeSet<_>>();
    let mut visited = 0_usize;
    while let Some(id) = ready.pop_first() {
        visited = visited.checked_add(1).ok_or(M6Error::Incomplete {
            operation: "gluing action DAG visit",
            limit: MAX_M6_GLUING_RERUN_ACTIONS,
            observed: usize::MAX,
        })?;
        for child in reverse.get(&id).into_iter().flatten() {
            let count = remaining
                .get_mut(child)
                .expect("reverse edge member exists");
            *count = count
                .checked_sub(1)
                .ok_or_else(|| M6Error::Canonical("gluing action DAG underflow".to_owned()))?;
            if *count == 0 {
                ready.insert(child.clone());
            }
        }
    }
    if visited != actions.len() {
        return Err(M6Error::Canonical(
            "gluing action prerequisites contain a cycle".to_owned(),
        ));
    }
    Ok(())
}

/// Enforces the part of §11.1 that depends on the complete fixed binding
/// vector rather than on a single action's local shape.  This runs before a
/// seal is derived, so a caller cannot omit a required rebuild or manufacture
/// an empty second plan without the one exact existing-M5 witness.
fn validate_gluing_plan_shape_v5(
    bindings: &[GluingClaimBindingV5; 2],
    actions: &[GluingRerunActionV5],
    existing_target_bundle_witness: &Option<ExistingTargetRecordV5>,
) -> M6Result<()> {
    if actions.is_empty() {
        return existing_target_bundle_witness.as_ref().map_or_else(
            || {
                Err(M6Error::Canonical(
                    "a zero-action gluing plan requires its exact existing target M5 witness"
                        .to_owned(),
                ))
            },
            |witness| {
                require_kind(
                    witness.record_id(),
                    "gluing-attempt-v4",
                    "existing_target_bundle_witness.record_id",
                )
            },
        );
    }
    if existing_target_bundle_witness.is_some() {
        return Err(M6Error::Canonical(
            "an existing target M5 witness suppresses every gluing action".to_owned(),
        ));
    }
    let reglue = actions
        .iter()
        .filter(|action| action.action == GluingRerunActionKindV5::Reglue)
        .collect::<Vec<_>>();
    if reglue.len() != 1 {
        return Err(M6Error::Canonical(
            "a nonempty gluing plan requires exactly one reglue action".to_owned(),
        ));
    }
    for binding in bindings {
        let registrations = actions
            .iter()
            .filter(|action| {
                action.action == GluingRerunActionKindV5::RegisterGluingInput
                    && action.subject_ids.contains(&binding.context_id)
            })
            .count();
        let rebuilds = actions
            .iter()
            .filter(|action| {
                action.action == GluingRerunActionKindV5::RebuildSection
                    && action.subject_ids.contains(&binding.context_id)
            })
            .count();
        if registrations > 1
            || rebuilds != usize::from(binding.status == GluingClaimBindingStatusV5::Selected)
        {
            return Err(M6Error::Canonical(
                "gluing binding does not have its exact registration/rebuild action shape"
                    .to_owned(),
            ));
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct GluingRerunPlanSealIdentityV5<'a> {
    planning_scope_id: &'a StableId,
    source_closure_id: &'a StableId,
    partial_rerun_plan_id: &'a StableId,
    target_plan_id: &'a StableId,
    selection_descriptor_id: &'static str,
    claim_bindings: &'a [GluingClaimBindingV5; 2],
    action_count: u64,
    action_set_digest: &'a ContentHash,
    existing_target_bundle_witness: &'a Option<ExistingTargetRecordV5>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GluingRerunPlanSealV5 {
    schema: &'static str,
    id: StableId,
    planning_scope_id: StableId,
    source_closure_id: StableId,
    partial_rerun_plan_id: StableId,
    target_plan_id: StableId,
    selection_descriptor_id: &'static str,
    claim_bindings: [GluingClaimBindingV5; 2],
    action_count: u64,
    action_set_digest: ContentHash,
    existing_target_bundle_witness: Option<ExistingTargetRecordV5>,
    source_ids: BTreeSet<StableId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GluingRerunPlanSealWireV5 {
    schema: String,
    id: StableId,
    planning_scope_id: StableId,
    source_closure_id: StableId,
    partial_rerun_plan_id: StableId,
    target_plan_id: StableId,
    selection_descriptor_id: String,
    claim_bindings: [GluingClaimBindingV5; 2],
    action_count: u64,
    action_set_digest: ContentHash,
    existing_target_bundle_witness: Option<ExistingTargetRecordV5>,
    source_ids: BTreeSet<StableId>,
}

impl GluingRerunPlanSealV5 {
    /// Exact recursive ownership used by the opaque event append phase.
    pub(crate) fn retained_bytes(&self) -> M6Result<usize> {
        let binding_bytes = self
            .claim_bindings
            .iter()
            .try_fold(0_usize, |total, binding| {
                let dynamic = binding
                    .context_id
                    .allocated_bytes()
                    .checked_add(
                        binding
                            .claim_id
                            .as_ref()
                            .map_or(0, StableId::allocated_bytes),
                    )
                    .and_then(|value| {
                        value.checked_add(
                            binding
                                .claim_body_hash
                                .as_ref()
                                .map_or(0, ContentHash::allocated_bytes),
                        )
                    })
                    .and_then(|value| {
                        value.checked_add(
                            binding
                                .obligation_id
                                .as_ref()
                                .map_or(0, StableId::allocated_bytes),
                        )
                    })
                    .ok_or(M6Error::Incomplete {
                        operation: "M6 gluing seal binding ownership",
                        limit: MAX_M6_CANONICAL_BYTES,
                        observed: usize::MAX,
                    })?;
                total.checked_add(dynamic).ok_or(M6Error::Incomplete {
                    operation: "M6 gluing seal binding ownership",
                    limit: MAX_M6_CANONICAL_BYTES,
                    observed: usize::MAX,
                })
            })?;
        let witness_bytes =
            self.existing_target_bundle_witness
                .as_ref()
                .map_or(Ok(0), |value| {
                    value
                        .record_id
                        .allocated_bytes()
                        .checked_add(value.body_hash.allocated_bytes())
                        .and_then(|bytes| bytes.checked_add(value.event_id.allocated_bytes()))
                        .ok_or(M6Error::Incomplete {
                            operation: "M6 gluing seal witness ownership",
                            limit: MAX_M6_CANONICAL_BYTES,
                            observed: usize::MAX,
                        })
                })?;
        [
            std::mem::size_of::<Self>(),
            self.id.allocated_bytes(),
            self.planning_scope_id.allocated_bytes(),
            self.source_closure_id.allocated_bytes(),
            self.partial_rerun_plan_id.allocated_bytes(),
            self.target_plan_id.allocated_bytes(),
            self.action_set_digest.allocated_bytes(),
            binding_bytes,
            witness_bytes,
            conservative_id_set_heap(&self.source_ids)?,
        ]
        .into_iter()
        .try_fold(0_usize, |total, value| {
            total.checked_add(value).ok_or(M6Error::Incomplete {
                operation: "M6 gluing seal retained bytes",
                limit: MAX_M6_CANONICAL_BYTES,
                observed: usize::MAX,
            })
        })
    }

    pub(crate) fn derive(
        source_closure_id: StableId,
        partial_rerun_plan_id: StableId,
        target_plan_id: StableId,
        claim_bindings: [GluingClaimBindingV5; 2],
        actions: &[GluingRerunActionV5],
        existing_target_bundle_witness: Option<ExistingTargetRecordV5>,
    ) -> M6Result<Self> {
        require_kind(
            &source_closure_id,
            "incremental-source-closure-v5",
            "source_closure_id",
        )?;
        require_kind(
            &partial_rerun_plan_id,
            "partial-rerun-plan-v5",
            "partial_rerun_plan_id",
        )?;
        require_kind(&target_plan_id, "plan", "target_plan_id")?;
        if claim_bindings[0].context_id.as_str() != crate::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID
            || claim_bindings[1].context_id.as_str() != crate::DOUBLE_SUBMIT_UI_CONTEXT_ID
            || actions.len() > MAX_M6_GLUING_RERUN_ACTIONS
            || actions.windows(2).any(|pair| pair[0].id >= pair[1].id)
        {
            return Err(M6Error::Canonical(
                "gluing plan members are not in their fixed canonical order".to_owned(),
            ));
        }
        let planning_scope_id = derive(
            "gluing-rerun-scope-v5",
            &(
                &source_closure_id,
                &partial_rerun_plan_id,
                &target_plan_id,
                &claim_bindings,
            ),
        )?;
        validate_gluing_action_dag_v5(&planning_scope_id, &claim_bindings, actions)?;
        validate_gluing_plan_shape_v5(&claim_bindings, actions, &existing_target_bundle_witness)?;
        let records = actions
            .iter()
            .map(|action| IdBodyHashV5::new(action.id.clone(), action.body_hash()?))
            .collect::<M6Result<Vec<_>>>()?;
        let action_set_digest = digest_records(&records)?;
        let mut source_ids = BTreeSet::from([
            source_closure_id.clone(),
            partial_rerun_plan_id.clone(),
            target_plan_id.clone(),
            planning_scope_id.clone(),
        ]);
        for binding in &claim_bindings {
            if let Some(id) = binding.claim_id() {
                source_ids.insert(id.clone());
            }
        }
        for action in actions {
            source_ids.insert(action.id.clone());
        }
        if let Some(witness) = &existing_target_bundle_witness {
            source_ids.insert(witness.record_id.clone());
            source_ids.insert(witness.event_id.clone());
        }
        let mut value = Self {
            schema: "reviewgraphen.gluing_rerun_plan.v5",
            id: StableId::parse("gluing-rerun-plan-v5:pending")?,
            planning_scope_id,
            source_closure_id,
            partial_rerun_plan_id,
            target_plan_id,
            selection_descriptor_id: M5_CLAIM_SELECTION_DESCRIPTOR_V5,
            claim_bindings,
            action_count: u64::try_from(actions.len()).map_err(|_| M6Error::Incomplete {
                operation: "M6 gluing action count",
                limit: MAX_M6_GLUING_RERUN_ACTIONS,
                observed: usize::MAX,
            })?,
            action_set_digest,
            existing_target_bundle_witness,
            source_ids,
        };
        value.id = derive(
            "gluing-rerun-plan-v5",
            &GluingRerunPlanSealIdentityV5 {
                planning_scope_id: &value.planning_scope_id,
                source_closure_id: &value.source_closure_id,
                partial_rerun_plan_id: &value.partial_rerun_plan_id,
                target_plan_id: &value.target_plan_id,
                selection_descriptor_id: value.selection_descriptor_id,
                claim_bindings: &value.claim_bindings,
                action_count: value.action_count,
                action_set_digest: &value.action_set_digest,
                existing_target_bundle_witness: &value.existing_target_bundle_witness,
            },
        )?;
        bounded_event_dto(
            &value,
            MAX_M6_CANONICAL_BYTES,
            "M6 gluing rerun plan seal DTO bytes",
        )?;
        Ok(value)
    }
    pub(crate) fn from_event_json_bytes(input: &[u8]) -> M6Result<Self> {
        bounded(
            input.len(),
            MAX_M6_CANONICAL_BYTES,
            "M6 gluing rerun plan JSON bytes",
        )?;
        preflight_event_line(input.len(), 1)?;
        let wire: GluingRerunPlanSealWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        full_sha256("action_set_digest", &wire.action_set_digest)?;
        let scope = derive(
            "gluing-rerun-scope-v5",
            &(
                &wire.source_closure_id,
                &wire.partial_rerun_plan_id,
                &wire.target_plan_id,
                &wire.claim_bindings,
            ),
        )?;
        let value = Self {
            schema: "reviewgraphen.gluing_rerun_plan.v5",
            id: wire.id,
            planning_scope_id: wire.planning_scope_id,
            source_closure_id: wire.source_closure_id,
            partial_rerun_plan_id: wire.partial_rerun_plan_id,
            target_plan_id: wire.target_plan_id,
            selection_descriptor_id: M5_CLAIM_SELECTION_DESCRIPTOR_V5,
            claim_bindings: wire.claim_bindings,
            action_count: wire.action_count,
            action_set_digest: wire.action_set_digest,
            existing_target_bundle_witness: wire.existing_target_bundle_witness,
            source_ids: wire.source_ids,
        };
        let existing_witness_shape = match (
            value.action_count,
            value.existing_target_bundle_witness.as_ref(),
        ) {
            (0, Some(witness)) => witness.record_id().kind() == "gluing-attempt-v4",
            (0, None) => false,
            (_, None) => true,
            (_, Some(_)) => false,
        };
        let mut expected_sources = BTreeSet::from([
            value.source_closure_id.clone(),
            value.partial_rerun_plan_id.clone(),
            value.target_plan_id.clone(),
            scope.clone(),
        ]);
        for binding in &value.claim_bindings {
            if let Some(id) = binding.claim_id() {
                expected_sources.insert(id.clone());
            }
        }
        if let Some(witness) = &value.existing_target_bundle_witness {
            expected_sources.insert(witness.record_id.clone());
            expected_sources.insert(witness.event_id.clone());
        }
        let action_ids = value
            .source_ids
            .iter()
            .filter(|id| id.kind() == "gluing-rerun-action-v5")
            .cloned()
            .collect::<BTreeSet<_>>();
        if action_ids.len() != usize::try_from(value.action_count).unwrap_or(usize::MAX) {
            return Err(M6Error::InvalidWire(
                "gluing plan action source IDs do not match action_count".to_owned(),
            ));
        }
        expected_sources.extend(action_ids);
        let identity = GluingRerunPlanSealIdentityV5 {
            planning_scope_id: &scope,
            source_closure_id: &value.source_closure_id,
            partial_rerun_plan_id: &value.partial_rerun_plan_id,
            target_plan_id: &value.target_plan_id,
            selection_descriptor_id: value.selection_descriptor_id,
            claim_bindings: &value.claim_bindings,
            action_count: value.action_count,
            action_set_digest: &value.action_set_digest,
            existing_target_bundle_witness: &value.existing_target_bundle_witness,
        };
        if value.schema != wire.schema
            || value.selection_descriptor_id != wire.selection_descriptor_id
            || value.claim_bindings[0].context_id.as_str()
                != crate::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID
            || value.claim_bindings[1].context_id.as_str() != crate::DOUBLE_SUBMIT_UI_CONTEXT_ID
            || !existing_witness_shape
            || value.planning_scope_id != scope
            || value.source_ids != expected_sources
            || value.action_count > MAX_M6_GLUING_RERUN_ACTIONS as u64
            || value.id != derive("gluing-rerun-plan-v5", &identity)?
            || crate::canonical_json(&value)? != input
        {
            return Err(M6Error::InvalidWire(
                "gluing rerun plan seal is not exact derived content".to_owned(),
            ));
        }
        Ok(value)
    }
    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    #[must_use]
    pub fn planning_scope_id(&self) -> &StableId {
        &self.planning_scope_id
    }
    #[must_use]
    pub fn source_closure_id(&self) -> &StableId {
        &self.source_closure_id
    }
    #[must_use]
    pub fn partial_rerun_plan_id(&self) -> &StableId {
        &self.partial_rerun_plan_id
    }
    #[must_use]
    pub fn target_plan_id(&self) -> &StableId {
        &self.target_plan_id
    }
    #[must_use]
    pub fn claim_bindings(&self) -> &[GluingClaimBindingV5; 2] {
        &self.claim_bindings
    }
    #[must_use]
    pub fn action_set_digest(&self) -> &ContentHash {
        &self.action_set_digest
    }

    #[must_use]
    pub fn existing_target_bundle_witness(&self) -> Option<&ExistingTargetRecordV5> {
        self.existing_target_bundle_witness.as_ref()
    }
    #[must_use]
    pub const fn action_count(&self) -> u64 {
        self.action_count
    }
    #[must_use]
    pub fn source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
    pub fn body_hash(&self) -> M6Result<ContentHash> {
        body_hash(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IdBodyHashV5 {
    body_hash: ContentHash,
    id: StableId,
}

impl<'de> Deserialize<'de> for IdBodyHashV5 {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            body_hash: ContentHash,
            id: StableId,
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.id, wire.body_hash).map_err(serde::de::Error::custom)
    }
}

impl IdBodyHashV5 {
    pub fn new(id: StableId, body_hash: ContentHash) -> M6Result<Self> {
        full_sha256("body_hash", &body_hash)?;
        Ok(Self { body_hash, id })
    }

    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }

    #[must_use]
    pub fn body_hash(&self) -> &ContentHash {
        &self.body_hash
    }
}

fn digest_records(records: &[IdBodyHashV5]) -> M6Result<ContentHash> {
    if records.windows(2).any(|pair| pair[0].id >= pair[1].id) {
        return Err(M6Error::Canonical(
            "M6 digest records must be strictly ID ordered".to_owned(),
        ));
    }
    crate::canonical::compact_json_sha256_streaming(&records).map_err(Into::into)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[doc(hidden)]
pub(crate) struct IncrementalStructuralInputV5 {
    repository_id: StableId,
    repository_identity_hash: ContentHash,
    source_run_id: StableId,
    source_genesis_hash: ContentHash,
    source_confirmed_offset: u64,
    source_tail_hash: ContentHash,
    source_event_count: u64,
    source_snapshot_id: StableId,
    source_universe_id: StableId,
    source_index_snapshot_hash: ContentHash,
    source_authority_policy_revision_hash: ContentHash,
    source_authority_replay_basis_digest: ContentHash,
    source_resolved_target_commit_oid: String,
    source_target_tree_hash: ContentHash,
    source_gluing_bundle_id: StableId,
    target_run_id: StableId,
    target_genesis_hash: ContentHash,
    target_predecessor_offset: u64,
    target_predecessor_tail_hash: ContentHash,
    target_predecessor_event_count: u64,
    target_snapshot_id: StableId,
    target_universe_id: StableId,
    target_predecessor_index_snapshot_hash: ContentHash,
    target_authority_policy_revision_hash: ContentHash,
    target_pre_incremental_authority_replay_basis_digest: ContentHash,
    target_resolved_base_commit_oid: String,
    target_base_tree_hash: ContentHash,
    target_resolved_target_commit_oid: String,
    target_target_tree_hash: ContentHash,
}

#[derive(Clone, Debug)]
pub(crate) struct ValidatedIncrementalStructureV5 {
    input: IncrementalStructuralInputV5,
}

impl ValidatedIncrementalStructureV5 {
    /// Validates Store's inert projection of an opaque locked dual-run proof.
    /// This value grants no append or replay authority.
    #[doc(hidden)]
    pub(crate) fn validate_store_projection(
        source: &ProgramSpace,
        target: &ProgramSpace,
        input: IncrementalStructuralInputV5,
    ) -> M6Result<Self> {
        if source.repository_id() != target.repository_id()
            || source.repository_id() != &input.repository_id
        {
            return Err(M6Error::InvalidSourceClosure(
                "source and target must be the same accepted repository",
            ));
        }
        if source.m6_is_dirty() || target.m6_is_dirty() {
            return Err(M6Error::InvalidSourceClosure(
                "source and target ProgramSpace snapshots must be clean",
            ));
        }
        if source.snapshot_id() != &input.source_snapshot_id
            || target.snapshot_id() != &input.target_snapshot_id
            || source.m6_tree_hash() != &input.source_target_tree_hash
            || target.m6_tree_hash() != &input.target_target_tree_hash
            || source.target_revision() != input.source_resolved_target_commit_oid
            || target.base_revision() != input.target_resolved_base_commit_oid
            || target.target_revision() != input.target_resolved_target_commit_oid
        {
            return Err(M6Error::InvalidSourceClosure(
                "session proof does not match the accepted ProgramSpace revisions",
            ));
        }
        if input.source_gluing_bundle_id.kind() != "event" {
            return Err(M6Error::InvalidSourceClosure(
                "source gluing bundle must bind its durable event ID",
            ));
        }
        Ok(Self { input })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IncrementalSourceClosureV5 {
    schema: &'static str,
    id: StableId,
    #[serde(flatten)]
    input: IncrementalStructuralInputV5,
}

/// Read-only, complete report projection of a closure already admitted by the
/// dual-run replay.  This is descriptive data only: it has no constructor,
/// no mutation API, and cannot be supplied to an append path.
#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IncrementalSourceClosureReportProjectionV5 {
    pub repository_id: StableId,
    pub repository_identity_hash: ContentHash,
    pub source_run_id: StableId,
    pub source_genesis_hash: ContentHash,
    pub source_confirmed_offset: u64,
    pub source_tail_hash: ContentHash,
    pub source_event_count: u64,
    pub source_snapshot_id: StableId,
    pub source_universe_id: StableId,
    pub source_index_snapshot_hash: ContentHash,
    pub source_authority_policy_revision_hash: ContentHash,
    pub source_authority_replay_basis_digest: ContentHash,
    pub source_resolved_target_commit_oid: String,
    pub source_target_tree_hash: ContentHash,
    pub source_gluing_bundle_id: StableId,
    pub target_run_id: StableId,
    pub target_genesis_hash: ContentHash,
    pub target_predecessor_offset: u64,
    pub target_predecessor_tail_hash: ContentHash,
    pub target_predecessor_event_count: u64,
    pub target_snapshot_id: StableId,
    pub target_universe_id: StableId,
    pub target_predecessor_index_snapshot_hash: ContentHash,
    pub target_authority_policy_revision_hash: ContentHash,
    pub target_pre_incremental_authority_replay_basis_digest: ContentHash,
    pub target_resolved_base_commit_oid: String,
    pub target_base_tree_hash: ContentHash,
    pub target_resolved_target_commit_oid: String,
    pub target_target_tree_hash: ContentHash,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct IncrementalSourceClosureWireV5 {
    schema: String,
    id: StableId,
    #[serde(flatten)]
    input: IncrementalStructuralInputV5,
}

impl IncrementalSourceClosureV5 {
    /// Store-only descriptive projection parser. It validates the closed
    /// durable wire but produces no admission, append, or replay capability.
    pub(crate) fn from_projection_event_json(input: &[u8]) -> M6Result<Self> {
        Self::validate_event_wire(input)?;
        let wire: IncrementalSourceClosureWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        Ok(Self {
            schema: "reviewgraphen.incremental_source_closure.v5",
            id: wire.id,
            input: wire.input,
        })
    }

    pub(crate) fn validate_event_wire(input: &[u8]) -> M6Result<()> {
        preflight_event_line(input.len(), 1)?;
        bounded(
            input.len(),
            MAX_M6_CLOSURE_DTO_BYTES,
            "M6 closure event JSON bytes",
        )?;
        let wire: IncrementalSourceClosureWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        // `deny_unknown_fields` cannot close a flattened serde struct by
        // itself.  Equality with the normalized wire projection makes any
        // discarded top-level member an explicit admission failure.
        let input_value: serde_json::Value = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        let normalized =
            serde_json::to_value(&wire).map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        if input_value != normalized {
            return Err(M6Error::InvalidWire(
                "closure event wire contains an unknown or lossy field".to_owned(),
            ));
        }
        let expected_id = Self::validate_input_and_derive_id(&wire.input)?;
        if wire.schema != "reviewgraphen.incremental_source_closure.v5" || wire.id != expected_id {
            return Err(M6Error::InvalidWire(
                "closure event wire has a wrong schema or derived ID".to_owned(),
            ));
        }
        Ok(())
    }

    fn validate_input_and_derive_id(input: &IncrementalStructuralInputV5) -> M6Result<StableId> {
        require_kind(&input.repository_id, "repository", "repository_id")?;
        require_kind(&input.source_run_id, "run", "source_run_id")?;
        require_kind(&input.target_run_id, "run", "target_run_id")?;
        require_kind(&input.source_snapshot_id, "snapshot", "source_snapshot_id")?;
        require_kind(&input.target_snapshot_id, "snapshot", "target_snapshot_id")?;
        require_kind(&input.source_universe_id, "universe", "source_universe_id")?;
        require_kind(&input.target_universe_id, "universe", "target_universe_id")?;
        require_kind(
            &input.source_gluing_bundle_id,
            "event",
            "source_gluing_bundle_id",
        )?;
        for (field, hash) in [
            ("repository_identity_hash", &input.repository_identity_hash),
            ("source_genesis_hash", &input.source_genesis_hash),
            ("source_tail_hash", &input.source_tail_hash),
            (
                "source_index_snapshot_hash",
                &input.source_index_snapshot_hash,
            ),
            (
                "source_authority_policy_revision_hash",
                &input.source_authority_policy_revision_hash,
            ),
            (
                "source_authority_replay_basis_digest",
                &input.source_authority_replay_basis_digest,
            ),
            ("target_genesis_hash", &input.target_genesis_hash),
            (
                "target_predecessor_tail_hash",
                &input.target_predecessor_tail_hash,
            ),
            (
                "target_predecessor_index_snapshot_hash",
                &input.target_predecessor_index_snapshot_hash,
            ),
            (
                "target_authority_policy_revision_hash",
                &input.target_authority_policy_revision_hash,
            ),
            (
                "target_pre_incremental_authority_replay_basis_digest",
                &input.target_pre_incremental_authority_replay_basis_digest,
            ),
        ] {
            full_sha256(field, hash)?;
        }
        git_oid(
            "source_resolved_target_commit_oid",
            &input.source_resolved_target_commit_oid,
        )?;
        git_oid(
            "target_resolved_base_commit_oid",
            &input.target_resolved_base_commit_oid,
        )?;
        git_oid(
            "target_resolved_target_commit_oid",
            &input.target_resolved_target_commit_oid,
        )?;
        git_tree("source_target_tree_hash", &input.source_target_tree_hash)?;
        git_tree("target_base_tree_hash", &input.target_base_tree_hash)?;
        git_tree("target_target_tree_hash", &input.target_target_tree_hash)?;
        if input.source_event_count == 0 || input.target_predecessor_event_count == 0 {
            return Err(M6Error::InvalidSourceClosure(
                "both pinned prefixes must be nonempty",
            ));
        }
        if input.source_run_id == input.target_run_id
            || input.source_snapshot_id == input.target_snapshot_id
        {
            return Err(M6Error::InvalidSourceClosure(
                "source and target run/snapshot IDs must differ",
            ));
        }
        if input.source_resolved_target_commit_oid != input.target_resolved_base_commit_oid
            || input.source_target_tree_hash != input.target_base_tree_hash
        {
            return Err(M6Error::InvalidSourceClosure(
                "source target must equal target base commit and tree",
            ));
        }
        if input.target_resolved_base_commit_oid == input.target_resolved_target_commit_oid {
            return Err(M6Error::InvalidSourceClosure(
                "target base and target commit must differ",
            ));
        }
        derive("incremental-source-closure-v5", input)
    }

    #[doc(hidden)]
    pub(crate) fn from_validated_structure(
        proof: ValidatedIncrementalStructureV5,
    ) -> M6Result<Self> {
        let input = proof.input;
        let id = Self::validate_input_and_derive_id(&input)?;
        let value = Self {
            schema: "reviewgraphen.incremental_source_closure.v5",
            id,
            input,
        };
        bounded_event_dto(&value, MAX_M6_CLOSURE_DTO_BYTES, "M6 closure DTO bytes")?;
        Ok(value)
    }

    pub(crate) fn from_json_bytes(input: &[u8], expected: &Self) -> M6Result<Self> {
        preflight_event_line(input.len(), 1)?;
        bounded(
            input.len(),
            MAX_M6_CLOSURE_DTO_BYTES,
            "M6 closure JSON bytes",
        )?;
        let wire: IncrementalSourceClosureWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        if wire.schema != "reviewgraphen.incremental_source_closure.v5"
            || wire.id != expected.id
            || wire.input != expected.input
            || crate::canonical_json(expected)? != input
        {
            return Err(M6Error::InvalidWire(
                "closure wire is not exact canonical session-proof content".to_owned(),
            ));
        }
        Ok(expected.clone())
    }

    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }

    /// Projects all report-visible dual-run coordinates from an already
    /// validated closure. The result is intentionally owned and has no
    /// identity/constructor method, preventing use as an authority proposal.
    #[must_use]
    pub fn report_projection(&self) -> IncrementalSourceClosureReportProjectionV5 {
        let value = &self.input;
        IncrementalSourceClosureReportProjectionV5 {
            repository_id: value.repository_id.clone(),
            repository_identity_hash: value.repository_identity_hash.clone(),
            source_run_id: value.source_run_id.clone(),
            source_genesis_hash: value.source_genesis_hash.clone(),
            source_confirmed_offset: value.source_confirmed_offset,
            source_tail_hash: value.source_tail_hash.clone(),
            source_event_count: value.source_event_count,
            source_snapshot_id: value.source_snapshot_id.clone(),
            source_universe_id: value.source_universe_id.clone(),
            source_index_snapshot_hash: value.source_index_snapshot_hash.clone(),
            source_authority_policy_revision_hash: value
                .source_authority_policy_revision_hash
                .clone(),
            source_authority_replay_basis_digest: value
                .source_authority_replay_basis_digest
                .clone(),
            source_resolved_target_commit_oid: value.source_resolved_target_commit_oid.clone(),
            source_target_tree_hash: value.source_target_tree_hash.clone(),
            source_gluing_bundle_id: value.source_gluing_bundle_id.clone(),
            target_run_id: value.target_run_id.clone(),
            target_genesis_hash: value.target_genesis_hash.clone(),
            target_predecessor_offset: value.target_predecessor_offset,
            target_predecessor_tail_hash: value.target_predecessor_tail_hash.clone(),
            target_predecessor_event_count: value.target_predecessor_event_count,
            target_snapshot_id: value.target_snapshot_id.clone(),
            target_universe_id: value.target_universe_id.clone(),
            target_predecessor_index_snapshot_hash: value
                .target_predecessor_index_snapshot_hash
                .clone(),
            target_authority_policy_revision_hash: value
                .target_authority_policy_revision_hash
                .clone(),
            target_pre_incremental_authority_replay_basis_digest: value
                .target_pre_incremental_authority_replay_basis_digest
                .clone(),
            target_resolved_base_commit_oid: value.target_resolved_base_commit_oid.clone(),
            target_base_tree_hash: value.target_base_tree_hash.clone(),
            target_resolved_target_commit_oid: value.target_resolved_target_commit_oid.clone(),
            target_target_tree_hash: value.target_target_tree_hash.clone(),
        }
    }

    #[must_use]
    pub(crate) fn input(&self) -> &IncrementalStructuralInputV5 {
        &self.input
    }

    // These deliberately remain crate-private.  They bind the reducer input
    // to an already validated dual-run proof; they do not expose a way to
    // manufacture or amend that proof.
    pub(crate) fn source_tail_hash(&self) -> &ContentHash {
        &self.input.source_tail_hash
    }

    pub(crate) fn target_predecessor_tail_hash(&self) -> &ContentHash {
        &self.input.target_predecessor_tail_hash
    }

    pub(crate) fn target_run_id(&self) -> &StableId {
        &self.input.target_run_id
    }

    pub(crate) fn source_run_id(&self) -> &StableId {
        &self.input.source_run_id
    }

    pub(crate) fn target_genesis_hash(&self) -> &ContentHash {
        &self.input.target_genesis_hash
    }

    pub(crate) const fn target_predecessor_offset(&self) -> u64 {
        self.input.target_predecessor_offset
    }

    pub(crate) const fn target_predecessor_event_count(&self) -> u64 {
        self.input.target_predecessor_event_count
    }

    pub(crate) fn source_authority_replay_basis_digest(&self) -> &ContentHash {
        &self.input.source_authority_replay_basis_digest
    }

    pub(crate) fn source_snapshot_id(&self) -> &StableId {
        &self.input.source_snapshot_id
    }

    pub(crate) fn target_snapshot_id(&self) -> &StableId {
        &self.input.target_snapshot_id
    }

    pub(crate) fn source_universe_id(&self) -> &StableId {
        &self.input.source_universe_id
    }

    pub(crate) fn target_universe_id(&self) -> &StableId {
        &self.input.target_universe_id
    }

    pub(crate) fn target_authority_policy_revision_hash(&self) -> &ContentHash {
        &self.input.target_authority_policy_revision_hash
    }

    pub(crate) fn target_pre_incremental_authority_replay_basis_digest(&self) -> &ContentHash {
        &self
            .input
            .target_pre_incremental_authority_replay_basis_digest
    }

    /// Rebinds an otherwise inert structural proposal to the only target
    /// authority proof the reducer accepts: a roots-validated terminal replay.
    /// This deliberately changes the closure ID, so every mapping and
    /// correspondence derived from the structural-only proposal must be
    /// rederived before reduction.
    pub(crate) fn bind_terminal_target_authority_v5(
        &self,
        target_actual: &TargetActualRecordInventoryV5<'_>,
    ) -> M6Result<Self> {
        if target_actual.target_run_id() != &self.input.target_run_id
            || target_actual.target_genesis_hash() != &self.input.target_genesis_hash
            || target_actual.target_tail_hash() != &self.input.target_predecessor_tail_hash
            || target_actual.target_event_count() != self.input.target_predecessor_event_count
        {
            return Err(M6Error::InvalidSourceClosure(
                "terminal target authority does not match the structural target predecessor",
            ));
        }
        let mut input = self.input.clone();
        input.target_authority_policy_revision_hash =
            target_actual.authority_policy_revision_hash().clone();
        input.target_pre_incremental_authority_replay_basis_digest =
            target_actual.authority_replay_basis_digest().clone();
        Self::from_validated_structure(ValidatedIncrementalStructureV5 { input })
    }

    pub fn body_hash(&self) -> M6Result<ContentHash> {
        body_hash(self)
    }
}

/// Deterministic M6 proposal rebuilt from already-replayed Core prefixes.
/// It carries no durable-store, CAS, index, append, report, or acceptance
/// authority. Store may retain it only inside its separate locked accepted
/// owner after independently validating the supplied index hashes.
///
/// The structural DTO and closure mint are deliberately not nameable outside
/// Core; callers can only obtain this zero-authority proposal by replay:
/// ```compile_fail
/// fn bypass(_: reviewgraphen_core::m6::IncrementalStructuralInputV5) {}
/// ```
pub struct UntrustedIncrementalMappingProposalV5 {
    closure: IncrementalSourceClosureV5,
    phase: M6MappingPhaseV5,
}

impl UntrustedIncrementalMappingProposalV5 {
    #[must_use]
    pub const fn closure(&self) -> &IncrementalSourceClosureV5 {
        &self.closure
    }

    #[must_use]
    pub const fn mapping_phase(&self) -> &M6MappingPhaseV5 {
        &self.phase
    }

    #[must_use]
    pub fn morphism(&self) -> &ChangeMorphismV5 {
        self.phase.morphism()
    }
}

#[derive(Serialize)]
struct ProposalRepositoryIdentityV5<'a> {
    repository_id: &'a StableId,
    repository_identity: &'a str,
}

#[derive(Serialize)]
struct ProposalTargetPolicyV5<'a> {
    schema: &'static str,
    profile_id: &'a str,
    profile_version: &'a str,
    policy_version: &'a str,
    rule_set_hash: &'a ContentHash,
    extractor_set_hash: &'a ContentHash,
}

#[derive(Serialize)]
struct ProposalTargetReplayBasisV5<'a> {
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

/// Rebuilds a zero-authority proposal from exact replay-owned source/target
/// logs. The only external scalars are hashes of Store's already locked index
/// snapshots; they remain proposal inputs and gain authority only through the
/// Store-owned accepted wrapper that retains those index handles.
#[doc(hidden)]
pub fn derive_untrusted_incremental_mapping_proposal_v5(
    source_log: &EventLogV4,
    source_basis: &AuthorityReplayBasisV4,
    completed: &M5CompletedGluingProfileV4,
    source_index_snapshot_hash: &ContentHash,
    target_log: &EventLogV5,
    target_index_snapshot_hash: &ContentHash,
) -> M6Result<UntrustedIncrementalMappingProposalV5> {
    let source_genesis = crate::RunGenesisSnapshot::from_canonical_v4_bytes_for_store(
        source_log.canonical_genesis_bytes(),
    )?;
    let source_program = source_genesis.program_space_for_store();
    let target_state = target_log.replay_initial_state_for_incremental_proposal()?;
    let target_projection = target_state.projection();
    let target_program = target_projection.program_space();
    let source_git =
        source_program
            .accepted_git_revision_closure()
            .ok_or(M6Error::InvalidSourceClosure(
                "source ProgramSpace lacks accepted Git revision closure",
            ))?;
    let target_git =
        target_program
            .accepted_git_revision_closure()
            .ok_or(M6Error::InvalidSourceClosure(
                "target ProgramSpace lacks accepted Git revision closure",
            ))?;
    let source_event_count =
        u64::try_from(source_log.envelopes().len()).map_err(|_| M6Error::Incomplete {
            operation: "M6 source replay event count",
            limit: usize::MAX,
            observed: usize::MAX,
        })?;
    let target_event_count =
        u64::try_from(target_log.envelopes().len()).map_err(|_| M6Error::Incomplete {
            operation: "M6 target replay event count",
            limit: usize::MAX,
            observed: usize::MAX,
        })?;
    if source_basis.run_id() != source_log.run_id()
        || source_basis.genesis_hash() != source_log.genesis_hash()
        || source_basis.confirmed_tail_hash() != source_log.tail_hash()
        || source_basis.confirmed_event_count() != source_event_count
        || completed.confirmed_tail_hash() != source_log.tail_hash()
        || completed.confirmed_event_count() != source_event_count
        || !source_log
            .envelopes()
            .iter()
            .any(|event| event.id() == completed.event_id())
    {
        return Err(M6Error::InvalidSourceClosure(
            "source replay basis and completed M5 event must equal the replayed prefix",
        ));
    }
    let repository_identity_hash =
        ContentHash::sha256(&crate::canonical_json(&ProposalRepositoryIdentityV5 {
            repository_id: source_program.repository_id(),
            repository_identity: source_program.repository_identity(),
        })?);
    let target_policy_revision_hash =
        ContentHash::sha256(&crate::canonical_json(&ProposalTargetPolicyV5 {
            schema: "reviewgraphen.target_policy_identity.v6",
            profile_id: target_program.profile_id(),
            profile_version: target_program.profile_version(),
            policy_version: target_program.policy_version(),
            rule_set_hash: target_program.rule_set_hash(),
            extractor_set_hash: target_program.extractor_set_hash(),
        })?);
    let target_plan_body_hash = ContentHash::sha256(&target_projection.plan().canonical_bytes()?);
    let target_confirmed_offset = target_log.canonical_prefix_bytes_for_store();
    let target_replay_basis_digest =
        ContentHash::sha256(&crate::canonical_json(&ProposalTargetReplayBasisV5 {
            schema: "reviewgraphen.target_pre_incremental_replay_basis.v6",
            run_id: target_log.run_id(),
            genesis_hash: target_log.genesis_hash(),
            confirmed_offset: target_confirmed_offset,
            tail_hash: target_log.tail_hash(),
            event_count: target_event_count,
            snapshot_id: target_program.snapshot_id(),
            universe_id: target_projection.universe().id(),
            plan_id: target_projection.plan().id(),
            plan_body_hash: &target_plan_body_hash,
            policy_revision_hash: &target_policy_revision_hash,
        })?);
    let input = IncrementalStructuralInputV5 {
        repository_id: source_program.repository_id().clone(),
        repository_identity_hash,
        source_run_id: source_log.run_id().clone(),
        source_genesis_hash: source_log.genesis_hash().clone(),
        source_confirmed_offset: source_log.canonical_prefix_bytes_for_store()?,
        source_tail_hash: source_log.tail_hash().clone(),
        source_event_count,
        source_snapshot_id: source_program.snapshot_id().clone(),
        source_universe_id: source_genesis.universe().id().clone(),
        source_index_snapshot_hash: source_index_snapshot_hash.clone(),
        source_authority_policy_revision_hash: source_basis.policy_revision_hash().clone(),
        source_authority_replay_basis_digest: source_basis.basis_digest().clone(),
        source_resolved_target_commit_oid: source_git.target_commit_oid().to_owned(),
        source_target_tree_hash: source_git.target_tree_hash().clone(),
        source_gluing_bundle_id: completed.event_id().clone(),
        target_run_id: target_log.run_id().clone(),
        target_genesis_hash: target_log.genesis_hash().clone(),
        target_predecessor_offset: target_confirmed_offset,
        target_predecessor_tail_hash: target_log.tail_hash().clone(),
        target_predecessor_event_count: target_event_count,
        target_snapshot_id: target_program.snapshot_id().clone(),
        target_universe_id: target_projection.universe().id().clone(),
        target_predecessor_index_snapshot_hash: target_index_snapshot_hash.clone(),
        target_authority_policy_revision_hash: target_policy_revision_hash,
        target_pre_incremental_authority_replay_basis_digest: target_replay_basis_digest,
        target_resolved_base_commit_oid: target_git.base_commit_oid().to_owned(),
        target_base_tree_hash: target_git.base_tree_hash().clone(),
        target_resolved_target_commit_oid: target_git.target_commit_oid().to_owned(),
        target_target_tree_hash: target_git.target_tree_hash().clone(),
    };
    let validated = ValidatedIncrementalStructureV5::validate_store_projection(
        source_program,
        target_program,
        input,
    )?;
    let closure = IncrementalSourceClosureV5::from_validated_structure(validated)?;
    let phase = ChangeMorphismV5::derive_from_accepted_program_facts(
        &closure,
        source_program,
        target_program,
    )?;
    Ok(UntrustedIncrementalMappingProposalV5 { closure, phase })
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RustSymbolKindV1 {
    Function,
    Method,
    Type,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RustSymbolAnchorV1 {
    descriptor: &'static str,
    language: &'static str,
    symbol_kind: RustSymbolKindV1,
    signature_shape_hash: ContentHash,
    normalized_body_hash: ContentHash,
}

impl RustSymbolAnchorV1 {
    pub fn new(
        symbol_kind: RustSymbolKindV1,
        signature_shape_hash: ContentHash,
        normalized_body_hash: ContentHash,
    ) -> M6Result<Self> {
        full_sha256("signature_shape_hash", &signature_shape_hash)?;
        full_sha256("normalized_body_hash", &normalized_body_hash)?;
        Ok(Self {
            descriptor: RUST_SYMBOL_ANCHOR_V1,
            language: "rust",
            symbol_kind,
            signature_shape_hash,
            normalized_body_hash,
        })
    }

    #[must_use]
    pub fn symbol_kind(&self) -> RustSymbolKindV1 {
        self.symbol_kind
    }

    #[must_use]
    pub fn signature_shape_hash(&self) -> &ContentHash {
        &self.signature_shape_hash
    }

    #[must_use]
    pub fn normalized_body_hash(&self) -> &ContentHash {
        &self.normalized_body_hash
    }

    pub fn body_hash(&self) -> M6Result<ContentHash> {
        body_hash(self)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramObjectKindV5 {
    Repository,
    Snapshot,
    Artifact,
    Relation,
    Context,
    Invariant,
    Limitation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MappingStatusV5 {
    Preserved,
    Modified,
    Added,
    Removed,
    Split,
    Merged,
    Unresolved,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateKeyKindV5 {
    RepositoryIdentity,
    SnapshotPair,
    SamePath,
    GitRenameSameContent,
    RustSymbolAnchorV1,
    SameKindLabelLanguageLocation,
    MappedDirectedEndpoints,
    MappedMembers,
    MappedScope,
    MappedLimitationSources,
    NoCandidate,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MappingStatusCountsV5 {
    pub preserved: u64,
    pub modified: u64,
    pub added: u64,
    pub removed: u64,
    pub split: u64,
    pub merged: u64,
    pub unresolved: u64,
}

impl MappingStatusCountsV5 {
    fn record(&mut self, status: MappingStatusV5) {
        match status {
            MappingStatusV5::Preserved => self.preserved += 1,
            MappingStatusV5::Modified => self.modified += 1,
            MappingStatusV5::Added => self.added += 1,
            MappingStatusV5::Removed => self.removed += 1,
            MappingStatusV5::Split => self.split += 1,
            MappingStatusV5::Merged => self.merged += 1,
            MappingStatusV5::Unresolved => self.unresolved += 1,
        }
    }

    fn checked_total(&self) -> Option<u64> {
        [
            self.preserved,
            self.modified,
            self.added,
            self.removed,
            self.split,
            self.merged,
            self.unresolved,
        ]
        .into_iter()
        .try_fold(0_u64, u64::checked_add)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProgramMappingPartsV5 {
    source_closure_id: StableId,
    source_snapshot_id: StableId,
    target_snapshot_id: StableId,
    object_kind: ProgramObjectKindV5,
    from_ids: BTreeSet<StableId>,
    to_ids: BTreeSet<StableId>,
    status: MappingStatusV5,
    candidate_key_kind: CandidateKeyKindV5,
    source_body_hashes: Vec<IdBodyHashV5>,
    target_body_hashes: Vec<IdBodyHashV5>,
    change_fact_ids: BTreeSet<StableId>,
    predecessor_mapping_ids: BTreeSet<StableId>,
}

#[derive(Serialize)]
struct ProgramMappingIdentityV5<'a> {
    source_closure_id: &'a StableId,
    source_snapshot_id: &'a StableId,
    target_snapshot_id: &'a StableId,
    object_kind: ProgramObjectKindV5,
    from_ids: &'a BTreeSet<StableId>,
    to_ids: &'a BTreeSet<StableId>,
    status: MappingStatusV5,
    candidate_key_kind: CandidateKeyKindV5,
    source_body_hashes: &'a Vec<IdBodyHashV5>,
    target_body_hashes: &'a Vec<IdBodyHashV5>,
    change_fact_ids: &'a BTreeSet<StableId>,
    predecessor_mapping_ids: &'a BTreeSet<StableId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProgramMappingV5 {
    schema: &'static str,
    id: StableId,
    source_closure_id: StableId,
    source_snapshot_id: StableId,
    target_snapshot_id: StableId,
    object_kind: ProgramObjectKindV5,
    from_ids: BTreeSet<StableId>,
    to_ids: BTreeSet<StableId>,
    status: MappingStatusV5,
    candidate_key_kind: CandidateKeyKindV5,
    source_body_hashes: Vec<IdBodyHashV5>,
    target_body_hashes: Vec<IdBodyHashV5>,
    change_fact_ids: BTreeSet<StableId>,
    predecessor_mapping_ids: BTreeSet<StableId>,
    successor_ids: BTreeSet<StableId>,
    source_ids: BTreeSet<StableId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgramMappingWireV5 {
    schema: String,
    id: StableId,
    source_closure_id: StableId,
    source_snapshot_id: StableId,
    target_snapshot_id: StableId,
    object_kind: ProgramObjectKindV5,
    from_ids: BTreeSet<StableId>,
    to_ids: BTreeSet<StableId>,
    status: MappingStatusV5,
    candidate_key_kind: CandidateKeyKindV5,
    source_body_hashes: Vec<IdBodyHashV5>,
    target_body_hashes: Vec<IdBodyHashV5>,
    change_fact_ids: BTreeSet<StableId>,
    predecessor_mapping_ids: BTreeSet<StableId>,
    successor_ids: BTreeSet<StableId>,
    source_ids: BTreeSet<StableId>,
}

impl ProgramMappingV5 {
    fn allocated_bytes(&self) -> usize {
        fn records(values: &Vec<IdBodyHashV5>) -> usize {
            values
                .capacity()
                .saturating_mul(std::mem::size_of::<IdBodyHashV5>())
                .saturating_add(
                    values
                        .iter()
                        .map(|value| {
                            value
                                .id
                                .allocated_bytes()
                                .saturating_add(value.body_hash.allocated_bytes())
                        })
                        .sum::<usize>(),
                )
        }
        [
            std::mem::size_of::<Self>(),
            self.id.allocated_bytes(),
            self.source_closure_id.allocated_bytes(),
            self.source_snapshot_id.allocated_bytes(),
            self.target_snapshot_id.allocated_bytes(),
            id_set_heap(&self.from_ids),
            id_set_heap(&self.to_ids),
            records(&self.source_body_hashes),
            records(&self.target_body_hashes),
            id_set_heap(&self.change_fact_ids),
            id_set_heap(&self.predecessor_mapping_ids),
            id_set_heap(&self.successor_ids),
            id_set_heap(&self.source_ids),
        ]
        .into_iter()
        .fold(0_usize, usize::saturating_add)
    }

    fn from_parts(input: ProgramMappingPartsV5) -> M6Result<Self> {
        require_kind(
            &input.source_closure_id,
            "incremental-source-closure-v5",
            "source_closure_id",
        )?;
        require_kind(&input.source_snapshot_id, "snapshot", "source_snapshot_id")?;
        require_kind(&input.target_snapshot_id, "snapshot", "target_snapshot_id")?;
        bounded(
            input.from_ids.len(),
            MAX_M6_MAPPING_SIDE_IDS,
            "M6 mapping source side",
        )?;
        bounded(
            input.to_ids.len(),
            MAX_M6_MAPPING_SIDE_IDS,
            "M6 mapping target side",
        )?;
        bounded(
            input.change_fact_ids.len(),
            MAX_M6_MAPPING_LINK_IDS,
            "M6 mapping change facts",
        )?;
        bounded(
            input.predecessor_mapping_ids.len(),
            MAX_M6_MAPPING_LINK_IDS,
            "M6 mapping predecessors",
        )?;
        validate_component_shape(input.from_ids.len(), input.to_ids.len(), input.status)?;
        if matches!(
            input.status,
            MappingStatusV5::Added | MappingStatusV5::Removed
        ) != matches!(input.candidate_key_kind, CandidateKeyKindV5::NoCandidate)
        {
            return Err(M6Error::InvalidMapping(
                "no_candidate is required exactly for added/removed components",
            ));
        }
        if !candidate_key_matches_object(input.object_kind, input.candidate_key_kind) {
            return Err(M6Error::InvalidMapping(
                "candidate key is not valid for the mapped object kind",
            ));
        }
        validate_body_hash_records(
            "source_body_hashes",
            &input.from_ids,
            &input.source_body_hashes,
        )?;
        validate_body_hash_records(
            "target_body_hashes",
            &input.to_ids,
            &input.target_body_hashes,
        )?;

        let identity = ProgramMappingIdentityV5 {
            source_closure_id: &input.source_closure_id,
            source_snapshot_id: &input.source_snapshot_id,
            target_snapshot_id: &input.target_snapshot_id,
            object_kind: input.object_kind,
            from_ids: &input.from_ids,
            to_ids: &input.to_ids,
            status: input.status,
            candidate_key_kind: input.candidate_key_kind,
            source_body_hashes: &input.source_body_hashes,
            target_body_hashes: &input.target_body_hashes,
            change_fact_ids: &input.change_fact_ids,
            predecessor_mapping_ids: &input.predecessor_mapping_ids,
        };
        let id = derive("program-mapping-v5", &identity)?;
        let successor_ids = if input.from_ids.is_empty() {
            BTreeSet::new()
        } else {
            input.to_ids.clone()
        };
        let source_ids = std::iter::once(input.source_closure_id.clone())
            .chain(input.from_ids.iter().cloned())
            .chain(input.to_ids.iter().cloned())
            .chain(input.change_fact_ids.iter().cloned())
            .chain(input.predecessor_mapping_ids.iter().cloned())
            .collect();
        Ok(Self {
            schema: "reviewgraphen.program_mapping.v5",
            id,
            source_closure_id: input.source_closure_id,
            source_snapshot_id: input.source_snapshot_id,
            target_snapshot_id: input.target_snapshot_id,
            object_kind: input.object_kind,
            from_ids: input.from_ids,
            to_ids: input.to_ids,
            status: input.status,
            candidate_key_kind: input.candidate_key_kind,
            source_body_hashes: input.source_body_hashes,
            target_body_hashes: input.target_body_hashes,
            change_fact_ids: input.change_fact_ids,
            predecessor_mapping_ids: input.predecessor_mapping_ids,
            successor_ids,
            source_ids,
        })
        .and_then(|value| {
            bounded_event_dto(&value, MAX_M6_MAPPING_DTO_BYTES, "M6 mapping DTO bytes")?;
            Ok(value)
        })
    }

    pub(crate) fn from_json_bytes(input: &[u8]) -> M6Result<Self> {
        preflight_event_line(input.len(), 1)?;
        bounded(
            input.len(),
            MAX_M6_MAPPING_DTO_BYTES,
            "M6 mapping JSON bytes",
        )?;
        let wire: ProgramMappingWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        if wire.schema != "reviewgraphen.program_mapping.v5" {
            return Err(M6Error::InvalidWire("wrong mapping schema".to_owned()));
        }
        let expected = Self::from_parts(ProgramMappingPartsV5 {
            source_closure_id: wire.source_closure_id,
            source_snapshot_id: wire.source_snapshot_id,
            target_snapshot_id: wire.target_snapshot_id,
            object_kind: wire.object_kind,
            from_ids: wire.from_ids,
            to_ids: wire.to_ids,
            status: wire.status,
            candidate_key_kind: wire.candidate_key_kind,
            source_body_hashes: wire.source_body_hashes,
            target_body_hashes: wire.target_body_hashes,
            change_fact_ids: wire.change_fact_ids,
            predecessor_mapping_ids: wire.predecessor_mapping_ids,
        })?;
        if expected.id != wire.id
            || expected.successor_ids != wire.successor_ids
            || expected.source_ids != wire.source_ids
            || crate::canonical_json(&expected)? != input
        {
            return Err(M6Error::InvalidWire(
                "mapping wire is not exact canonical derived content".to_owned(),
            ));
        }
        Ok(expected)
    }

    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    #[must_use]
    pub fn source_closure_id(&self) -> &StableId {
        &self.source_closure_id
    }
    #[must_use]
    pub fn source_snapshot_id(&self) -> &StableId {
        &self.source_snapshot_id
    }
    #[must_use]
    pub fn target_snapshot_id(&self) -> &StableId {
        &self.target_snapshot_id
    }
    #[must_use]
    pub fn from_ids(&self) -> &BTreeSet<StableId> {
        &self.from_ids
    }
    #[must_use]
    pub fn to_ids(&self) -> &BTreeSet<StableId> {
        &self.to_ids
    }
    #[must_use]
    pub fn status(&self) -> MappingStatusV5 {
        self.status
    }
    #[must_use]
    pub fn object_kind(&self) -> ProgramObjectKindV5 {
        self.object_kind
    }
    #[must_use]
    pub fn candidate_key_kind(&self) -> CandidateKeyKindV5 {
        self.candidate_key_kind
    }
    #[must_use]
    pub fn source_body_hashes(&self) -> &[IdBodyHashV5] {
        &self.source_body_hashes
    }
    #[must_use]
    pub fn target_body_hashes(&self) -> &[IdBodyHashV5] {
        &self.target_body_hashes
    }
    #[must_use]
    pub fn change_fact_ids(&self) -> &BTreeSet<StableId> {
        &self.change_fact_ids
    }
    #[must_use]
    pub fn predecessor_mapping_ids(&self) -> &BTreeSet<StableId> {
        &self.predecessor_mapping_ids
    }
    #[must_use]
    pub fn successor_ids(&self) -> &BTreeSet<StableId> {
        &self.successor_ids
    }
    #[must_use]
    pub fn source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
    pub fn body_hash(&self) -> M6Result<ContentHash> {
        body_hash(self)
    }
}

fn preflight_event_line(payload_bytes: usize, envelope_bytes: usize) -> M6Result<usize> {
    let observed = payload_bytes
        .checked_add(envelope_bytes)
        .ok_or(M6Error::Incomplete {
            operation: "M6 event-line bytes",
            limit: MAX_M6_EVENT_LINE_BYTES,
            observed: usize::MAX,
        })?;
    bounded(observed, MAX_M6_EVENT_LINE_BYTES, "M6 event-line bytes")?;
    Ok(observed)
}

fn validate_component_shape(from: usize, to: usize, status: MappingStatusV5) -> M6Result<()> {
    let valid = match (from, to, status) {
        (0, 1, MappingStatusV5::Added) | (1, 0, MappingStatusV5::Removed) => true,
        (
            1,
            1,
            MappingStatusV5::Preserved | MappingStatusV5::Modified | MappingStatusV5::Unresolved,
        ) => true,
        (1, target, MappingStatusV5::Split) if target > 1 => true,
        (source, 1, MappingStatusV5::Merged) if source > 1 => true,
        (source, target, MappingStatusV5::Unresolved) if source > 1 && target > 1 => true,
        _ => false,
    };
    if !valid {
        return Err(M6Error::InvalidMapping(
            "status does not match the exclusive component cardinality",
        ));
    }
    Ok(())
}

fn candidate_key_matches_object(
    object_kind: ProgramObjectKindV5,
    candidate_key_kind: CandidateKeyKindV5,
) -> bool {
    match candidate_key_kind {
        CandidateKeyKindV5::NoCandidate => true,
        CandidateKeyKindV5::RepositoryIdentity => object_kind == ProgramObjectKindV5::Repository,
        CandidateKeyKindV5::SnapshotPair => object_kind == ProgramObjectKindV5::Snapshot,
        CandidateKeyKindV5::SamePath
        | CandidateKeyKindV5::GitRenameSameContent
        | CandidateKeyKindV5::RustSymbolAnchorV1
        | CandidateKeyKindV5::SameKindLabelLanguageLocation => {
            object_kind == ProgramObjectKindV5::Artifact
        }
        CandidateKeyKindV5::MappedDirectedEndpoints => object_kind == ProgramObjectKindV5::Relation,
        CandidateKeyKindV5::MappedMembers => object_kind == ProgramObjectKindV5::Context,
        CandidateKeyKindV5::MappedScope => object_kind == ProgramObjectKindV5::Invariant,
        CandidateKeyKindV5::MappedLimitationSources => {
            object_kind == ProgramObjectKindV5::Limitation
        }
    }
}

fn validate_body_hash_records(
    field: &'static str,
    ids: &BTreeSet<StableId>,
    hashes: &[IdBodyHashV5],
) -> M6Result<()> {
    if hashes.windows(2).any(|pair| pair[0].id >= pair[1].id)
        || hashes.iter().map(|record| &record.id).ne(ids.iter())
    {
        return Err(M6Error::InvalidMapping(
            "body-hash records must be sorted, unique, and exactly equal component IDs",
        ));
    }
    for record in hashes {
        full_sha256(field, &record.body_hash)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitChangeKindV5 {
    Renamed,
    Copied,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GitChangeFactV5 {
    fact_id: StableId,
    kind: GitChangeKindV5,
    source_artifact_id: StableId,
    target_artifact_id: StableId,
    source_path: String,
    target_path: String,
    equal_content_hash: Option<ContentHash>,
}

impl GitChangeFactV5 {
    fn from_accepted_artifact(
        fact: &crate::Artifact,
        source: &ProgramSpace,
        target: &ProgramSpace,
    ) -> M6Result<Option<Self>> {
        if fact.kind != "custom" || fact.provenance.extraction_method() != GIT_CHANGE_PROVENANCE_V1
        {
            return Ok(None);
        }
        let string_attribute = |name| {
            fact.attributes
                .get(name)
                .and_then(serde_json::Value::as_str)
        };
        let kind = match string_attribute("change_kind") {
            Some("renamed") => GitChangeKindV5::Renamed,
            Some("copied") => GitChangeKindV5::Copied,
            _ => return Ok(None),
        };
        let source_path = string_attribute("base_path").ok_or(M6Error::InvalidMapping(
            "accepted Git change fact is missing base_path",
        ))?;
        let target_path = string_attribute("target_path").ok_or(M6Error::InvalidMapping(
            "accepted Git change fact is missing target_path",
        ))?;
        if source_path == target_path
            || !normalized_mapping_path(source_path)
            || !normalized_mapping_path(target_path)
        {
            return Err(M6Error::InvalidMapping(
                "accepted Git change paths must be distinct normalized paths",
            ));
        }
        let source_file = unique_file_at_path(source, source_path)?;
        let target_file = unique_file_at_path(target, target_path)?;
        let equal_content_hash = match (&source_file.content_hash, &target_file.content_hash) {
            (Some(left), Some(right)) if left == right => Some(left.clone()),
            _ => None,
        };
        Ok(Some(Self {
            fact_id: fact.id.clone(),
            kind,
            source_artifact_id: source_file.id.clone(),
            target_artifact_id: target_file.id.clone(),
            source_path: source_path.to_owned(),
            target_path: target_path.to_owned(),
            equal_content_hash,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    // Unit-level algorithm fixture; production uses `from_accepted_artifact`.
    #[cfg(test)]
    fn fixture_from_parts(
        fact_id: StableId,
        kind: GitChangeKindV5,
        source_artifact_id: StableId,
        target_artifact_id: StableId,
        source_path: String,
        target_path: String,
        content_hash: ContentHash,
        provenance_descriptor_id: &str,
    ) -> M6Result<Self> {
        full_sha256("git change content_hash", &content_hash)?;
        if fact_id.kind() != "git-change-fact-v5" {
            return Err(M6Error::InvalidMapping(
                "Git change fact ID must use git-change-fact-v5",
            ));
        }
        if provenance_descriptor_id != GIT_CHANGE_PROVENANCE_V1 {
            return Err(M6Error::InvalidMapping(
                "Git change fact has untrusted extractor provenance",
            ));
        }
        if source_path == target_path
            || !normalized_mapping_path(&source_path)
            || !normalized_mapping_path(&target_path)
        {
            return Err(M6Error::InvalidMapping(
                "Git change paths must be distinct nonempty normalized paths",
            ));
        }
        Ok(Self {
            fact_id,
            kind,
            source_artifact_id,
            target_artifact_id,
            source_path,
            target_path,
            equal_content_hash: Some(content_hash),
        })
    }
}

fn normalized_mapping_path(path: &str) -> bool {
    !(path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || (path.len() >= 2
            && path.as_bytes()[0].is_ascii_alphabetic()
            && path.as_bytes()[1] == b':'))
        && path
            .split('/')
            .all(|segment| !matches!(segment, "" | "." | ".."))
}

fn unique_file_at_path<'a>(space: &'a ProgramSpace, path: &str) -> M6Result<&'a crate::Artifact> {
    let mut files = space.artifacts().iter().filter(|artifact| {
        artifact.kind == "file"
            && artifact
                .location
                .as_ref()
                .map(|location| location.path.as_str())
                == Some(path)
    });
    let file = files.next().ok_or(M6Error::InvalidMapping(
        "accepted Git change path has no file artifact",
    ))?;
    if files.next().is_some() {
        return Err(M6Error::InvalidMapping(
            "accepted Git change path has multiple file artifacts",
        ));
    }
    Ok(file)
}

#[derive(Clone, Debug)]
pub(crate) struct ValidatedIncrementalInputsV5 {
    source_snapshot_id: StableId,
    target_snapshot_id: StableId,
    rust_source_anchors: BTreeMap<StableId, RustSymbolAnchorV1>,
    rust_target_anchors: BTreeMap<StableId, RustSymbolAnchorV1>,
    source_relation_target_order: BTreeMap<StableId, Vec<StableId>>,
    target_relation_target_order: BTreeMap<StableId, Vec<StableId>>,
    git_change_facts: Vec<GitChangeFactV5>,
}

impl ValidatedIncrementalInputsV5 {
    fn derive_from_accepted_program_facts(
        source: &ProgramSpace,
        target: &ProgramSpace,
    ) -> M6Result<Self> {
        if source.extractor_set_hash() != target.extractor_set_hash() {
            return Err(M6Error::InvalidMapping(
                "source and target extractor sets must be identical",
            ));
        }
        let source_anchors = source.accepted_rust_symbol_anchors().ok_or_else(|| {
            M6Error::MissingAcceptedMappingFact {
                kind: "incremental Rust anchor set",
                object_id: source.snapshot_id().clone(),
            }
        })?;
        let target_anchors = target.accepted_rust_symbol_anchors().ok_or_else(|| {
            M6Error::MissingAcceptedMappingFact {
                kind: "incremental Rust anchor set",
                object_id: target.snapshot_id().clone(),
            }
        })?;
        let source_relation_order = source.accepted_relation_target_order().ok_or_else(|| {
            M6Error::MissingAcceptedMappingFact {
                kind: "incremental ordered relation set",
                object_id: source.snapshot_id().clone(),
            }
        })?;
        let target_relation_order = target.accepted_relation_target_order().ok_or_else(|| {
            M6Error::MissingAcceptedMappingFact {
                kind: "incremental ordered relation set",
                object_id: target.snapshot_id().clone(),
            }
        })?;
        let mut git_change_facts = target
            .artifacts()
            .iter()
            .filter_map(|artifact| {
                GitChangeFactV5::from_accepted_artifact(artifact, source, target).transpose()
            })
            .collect::<M6Result<Vec<_>>>()?;
        git_change_facts.sort_by(|left, right| left.fact_id.cmp(&right.fact_id));
        if git_change_facts
            .windows(2)
            .any(|pair| pair[0].fact_id == pair[1].fact_id)
        {
            return Err(M6Error::InvalidMapping("duplicate Git change fact ID"));
        }
        let mut seen_pairs = BTreeSet::new();
        let mut renamed_sources = BTreeSet::new();
        let mut targets = BTreeSet::new();
        for fact in &git_change_facts {
            if !seen_pairs.insert((
                fact.source_artifact_id.clone(),
                fact.target_artifact_id.clone(),
            )) || !targets.insert(fact.target_artifact_id.clone())
                || (fact.kind == GitChangeKindV5::Renamed
                    && !renamed_sources.insert(fact.source_artifact_id.clone()))
            {
                return Err(M6Error::InvalidMapping(
                    "accepted Git change facts violate exact pair/source/target ownership",
                ));
            }
        }
        Ok(Self {
            source_snapshot_id: source.snapshot_id().clone(),
            target_snapshot_id: target.snapshot_id().clone(),
            rust_source_anchors: source_anchors.clone(),
            rust_target_anchors: target_anchors.clone(),
            source_relation_target_order: source_relation_order,
            target_relation_target_order: target_relation_order,
            git_change_facts,
        })
    }

    // Unit-level algorithm fixture; production always uses
    // `derive_from_accepted_program_facts` above.
    #[cfg(test)]
    fn fixture_from_parts(
        source: &ProgramSpace,
        target: &ProgramSpace,
        rust_source_anchors: BTreeMap<StableId, RustSymbolAnchorV1>,
        rust_target_anchors: BTreeMap<StableId, RustSymbolAnchorV1>,
        source_relation_target_order: BTreeMap<StableId, Vec<StableId>>,
        target_relation_target_order: BTreeMap<StableId, Vec<StableId>>,
        mut git_change_facts: Vec<GitChangeFactV5>,
    ) -> M6Result<Self> {
        if source.extractor_set_hash() != target.extractor_set_hash() {
            return Err(M6Error::InvalidMapping(
                "source and target extractor sets must be identical",
            ));
        }
        validate_anchor_domain(source, &rust_source_anchors)?;
        validate_anchor_domain(target, &rust_target_anchors)?;
        validate_relation_orders(source, &source_relation_target_order)?;
        validate_relation_orders(target, &target_relation_target_order)?;
        git_change_facts.sort_by(|left, right| left.fact_id.cmp(&right.fact_id));
        if git_change_facts
            .windows(2)
            .any(|pair| pair[0].fact_id == pair[1].fact_id)
        {
            return Err(M6Error::InvalidMapping("duplicate Git change fact ID"));
        }
        let mut seen_pairs = BTreeSet::new();
        let mut renamed_sources = BTreeSet::new();
        let mut targets = BTreeSet::new();
        for fact in &git_change_facts {
            if !seen_pairs.insert((
                fact.source_artifact_id.clone(),
                fact.target_artifact_id.clone(),
            )) || !targets.insert(fact.target_artifact_id.clone())
                || (fact.kind == GitChangeKindV5::Renamed
                    && !renamed_sources.insert(fact.source_artifact_id.clone()))
            {
                return Err(M6Error::InvalidMapping(
                    "Git change facts violate exact pair/source/target ownership",
                ));
            }
        }
        let source_artifacts = source
            .artifacts()
            .iter()
            .map(|artifact| (&artifact.id, artifact))
            .collect::<BTreeMap<_, _>>();
        let target_artifacts = target
            .artifacts()
            .iter()
            .map(|artifact| (&artifact.id, artifact))
            .collect::<BTreeMap<_, _>>();
        for fact in &git_change_facts {
            let source_artifact = source_artifacts
                .get(&fact.source_artifact_id)
                .ok_or(M6Error::InvalidMapping("Git change source is outside S0"))?;
            let target_artifact = target_artifacts
                .get(&fact.target_artifact_id)
                .ok_or(M6Error::InvalidMapping("Git change target is outside S1"))?;
            let source_path = source_artifact
                .location
                .as_ref()
                .map(|value| value.path.as_str());
            let target_path = target_artifact
                .location
                .as_ref()
                .map(|value| value.path.as_str());
            if source_artifact.kind != "file"
                || target_artifact.kind != "file"
                || source_path != Some(fact.source_path.as_str())
                || target_path != Some(fact.target_path.as_str())
                || source_artifact.content_hash.as_ref() != fact.equal_content_hash.as_ref()
                || target_artifact.content_hash.as_ref() != fact.equal_content_hash.as_ref()
            {
                return Err(M6Error::InvalidMapping(
                    "Git change fact must bind exact equal-content file artifacts",
                ));
            }
        }
        Ok(Self {
            source_snapshot_id: source.snapshot_id().clone(),
            target_snapshot_id: target.snapshot_id().clone(),
            rust_source_anchors,
            rust_target_anchors,
            source_relation_target_order,
            target_relation_target_order,
            git_change_facts,
        })
    }
}

fn rust_symbol(artifact: &crate::Artifact) -> bool {
    artifact.language.as_deref() == Some("rust")
        && matches!(artifact.kind.as_str(), "function" | "method" | "type")
}

fn validate_anchor_domain(
    space: &ProgramSpace,
    anchors: &BTreeMap<StableId, RustSymbolAnchorV1>,
) -> M6Result<()> {
    let expected = space
        .artifacts()
        .iter()
        .filter(|artifact| rust_symbol(artifact))
        .map(|artifact| artifact.id.clone())
        .collect::<BTreeSet<_>>();
    if anchors.keys().ne(expected.iter()) {
        return Err(M6Error::InvalidMapping(
            "Rust anchor sidecar must exactly cover every Rust symbol",
        ));
    }
    Ok(())
}

fn validate_relation_orders(
    space: &ProgramSpace,
    orders: &BTreeMap<StableId, Vec<StableId>>,
) -> M6Result<()> {
    if orders.len() != space.relations().len() {
        return Err(M6Error::InvalidMapping(
            "ordered endpoint sidecar must cover every relation",
        ));
    }
    for relation in space.relations() {
        let ordered = orders.get(&relation.id).ok_or(M6Error::InvalidMapping(
            "ordered endpoint sidecar is missing a relation",
        ))?;
        let unique = ordered.iter().cloned().collect::<BTreeSet<_>>();
        if unique.len() != ordered.len() || unique != relation.target_ids {
            return Err(M6Error::InvalidMapping(
                "ordered endpoints must be a duplicate-free permutation of target_ids",
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
enum NodeDataV5 {
    Repository {
        identity: String,
    },
    Snapshot,
    Artifact {
        kind: String,
        label: String,
        language: Option<String>,
        path: Option<String>,
        location: Option<Box<crate::Location>>,
        content_hash: Option<ContentHash>,
        anchor: Option<RustSymbolAnchorV1>,
        base_hash: ContentHash,
        provenance: NormalizedProvenanceV5,
        change_fact_ids: BTreeSet<StableId>,
        symbol_ref: Option<StableId>,
    },
    Relation {
        kind: String,
        directed: bool,
        source_id: StableId,
        ordered_target_ids: Vec<StableId>,
        base_hash: ContentHash,
        provenance: NormalizedProvenanceV5,
    },
    Context {
        kind: String,
        members: BTreeSet<StableId>,
        base_hash: ContentHash,
        provenance: NormalizedProvenanceV5,
    },
    Invariant {
        property_id: String,
        scope: BTreeSet<StableId>,
        base_hash: ContentHash,
        provenance: NormalizedProvenanceV5,
    },
    Limitation {
        description: String,
        kind: crate::LimitationKind,
        severity: crate::Severity,
        sources: BTreeSet<StableId>,
        base_hash: ContentHash,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "scope", content = "value")]
enum NormalizedRevisionRefV5 {
    Absent,
    TargetSnapshot,
    Exact(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "scope", content = "value")]
enum NormalizedContentRefV5 {
    Absent,
    TargetTree,
    SelfArtifact,
    ProgramArtifact(StableId),
    Exact(ContentHash),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "scope", content = "value")]
enum NormalizedLocalRefV5 {
    Absent,
    SelfArtifact,
    ProgramArtifact(StableId),
    Exact(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct NormalizedProvenanceV5 {
    stable_hash: ContentHash,
    revision: NormalizedRevisionRefV5,
    content: NormalizedContentRefV5,
    local: NormalizedLocalRefV5,
}

impl NormalizedProvenanceV5 {
    fn allocated_bytes(&self) -> usize {
        fn revision(value: &NormalizedRevisionRefV5) -> usize {
            match value {
                NormalizedRevisionRefV5::Exact(value) => value.capacity(),
                NormalizedRevisionRefV5::Absent | NormalizedRevisionRefV5::TargetSnapshot => 0,
            }
        }
        fn content(value: &NormalizedContentRefV5) -> usize {
            match value {
                NormalizedContentRefV5::ProgramArtifact(id) => id.allocated_bytes(),
                NormalizedContentRefV5::Exact(hash) => hash.allocated_bytes(),
                NormalizedContentRefV5::Absent
                | NormalizedContentRefV5::TargetTree
                | NormalizedContentRefV5::SelfArtifact => 0,
            }
        }
        fn local(value: &NormalizedLocalRefV5) -> usize {
            match value {
                NormalizedLocalRefV5::ProgramArtifact(id) => id.allocated_bytes(),
                NormalizedLocalRefV5::Exact(value) => value.capacity(),
                NormalizedLocalRefV5::Absent | NormalizedLocalRefV5::SelfArtifact => 0,
            }
        }
        self.stable_hash
            .allocated_bytes()
            .saturating_add(revision(&self.revision))
            .saturating_add(content(&self.content))
            .saturating_add(local(&self.local))
    }
}

#[derive(Clone, Debug)]
struct SnapshotPathArtifactV5 {
    id: StableId,
    content_hash: Option<ContentHash>,
}

#[derive(Serialize)]
struct ProvenanceStableBodyV5<'a> {
    confidence: Option<f64>,
    extraction_method: &'a str,
    review_status: crate::ReviewStatus,
    source_kind: &'a str,
    source_locator: &'a str,
    tool_version: Option<&'a str>,
}

fn snapshot_path_artifacts(space: &ProgramSpace) -> BTreeMap<String, SnapshotPathArtifactV5> {
    let mut paths = BTreeMap::<String, Option<SnapshotPathArtifactV5>>::new();
    for artifact in space
        .artifacts()
        .iter()
        .filter(|artifact| artifact.kind == "file")
    {
        let Some(location) = &artifact.location else {
            continue;
        };
        let value = SnapshotPathArtifactV5 {
            id: artifact.id.clone(),
            content_hash: artifact.content_hash.clone(),
        };
        paths
            .entry(location.path.clone())
            .and_modify(|entry| *entry = None)
            .or_insert(Some(value));
    }
    paths
        .into_iter()
        .filter_map(|(path, value)| value.map(|value| (path, value)))
        .collect()
}

fn normalize_provenance(
    space: &ProgramSpace,
    provenance: &crate::Provenance,
    self_path: Option<&str>,
    self_content_hash: Option<&ContentHash>,
    paths: &BTreeMap<String, SnapshotPathArtifactV5>,
) -> M6Result<NormalizedProvenanceV5> {
    let source = provenance.source();
    // Snapshot roles are recognized only for this exact Git repository. An
    // external source that happens to reuse an OID/hash remains semantically
    // exact and therefore cannot be laundered by replacement.
    let is_snapshot_git = source.kind() == "git" && source.locator() == space.repository_identity();
    let revision = match source.revision() {
        None => NormalizedRevisionRefV5::Absent,
        Some(value) if is_snapshot_git && value == space.target_revision() => {
            NormalizedRevisionRefV5::TargetSnapshot
        }
        Some(value) => NormalizedRevisionRefV5::Exact(value.to_owned()),
    };
    let local_path = source.source_local_id();
    let referenced_artifact = local_path.and_then(|path| paths.get(path));
    let content = match source.content_hash() {
        None => NormalizedContentRefV5::Absent,
        Some(value) if is_snapshot_git && value == space.incremental_tree_hash_for_store() => {
            NormalizedContentRefV5::TargetTree
        }
        Some(value)
            if is_snapshot_git
                && self_path == local_path
                && self_content_hash.is_some_and(|hash| hash == value) =>
        {
            NormalizedContentRefV5::SelfArtifact
        }
        Some(value)
            if is_snapshot_git
                && referenced_artifact
                    .and_then(|artifact| artifact.content_hash.as_ref())
                    .is_some_and(|hash| hash == value) =>
        {
            NormalizedContentRefV5::ProgramArtifact(referenced_artifact.unwrap().id.clone())
        }
        Some(value) => NormalizedContentRefV5::Exact(value.clone()),
    };
    let local = match local_path {
        None => NormalizedLocalRefV5::Absent,
        Some(_) if is_snapshot_git && self_path == local_path => NormalizedLocalRefV5::SelfArtifact,
        Some(_) if is_snapshot_git && referenced_artifact.is_some() => {
            NormalizedLocalRefV5::ProgramArtifact(referenced_artifact.unwrap().id.clone())
        }
        Some(value) => NormalizedLocalRefV5::Exact(value.to_owned()),
    };
    Ok(NormalizedProvenanceV5 {
        stable_hash: body_hash(&ProvenanceStableBodyV5 {
            confidence: provenance.confidence_for_mapping(),
            extraction_method: provenance.extraction_method(),
            review_status: provenance.review_status(),
            source_kind: source.kind(),
            source_locator: source.locator(),
            tool_version: provenance.tool_version_for_mapping(),
        })?,
        revision,
        content,
        local,
    })
}

fn normalized_program_ref_equal(
    source: &StableId,
    target: &StableId,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> bool {
    successors
        .get(source)
        .is_some_and(|ids| ids.len() == 1 && ids.contains(target))
}

fn normalized_provenance_equal(
    source: &NormalizedProvenanceV5,
    target: &NormalizedProvenanceV5,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> bool {
    if source.stable_hash != target.stable_hash || source.revision != target.revision {
        return false;
    }
    let content_equal = match (&source.content, &target.content) {
        (NormalizedContentRefV5::Absent, NormalizedContentRefV5::Absent)
        | (NormalizedContentRefV5::TargetTree, NormalizedContentRefV5::TargetTree)
        | (NormalizedContentRefV5::SelfArtifact, NormalizedContentRefV5::SelfArtifact) => true,
        (
            NormalizedContentRefV5::ProgramArtifact(source),
            NormalizedContentRefV5::ProgramArtifact(target),
        ) => normalized_program_ref_equal(source, target, successors),
        (NormalizedContentRefV5::Exact(source), NormalizedContentRefV5::Exact(target)) => {
            source == target
        }
        _ => false,
    };
    let local_equal = match (&source.local, &target.local) {
        (NormalizedLocalRefV5::Absent, NormalizedLocalRefV5::Absent)
        | (NormalizedLocalRefV5::SelfArtifact, NormalizedLocalRefV5::SelfArtifact) => true,
        (
            NormalizedLocalRefV5::ProgramArtifact(source),
            NormalizedLocalRefV5::ProgramArtifact(target),
        ) => normalized_program_ref_equal(source, target, successors),
        (NormalizedLocalRefV5::Exact(source), NormalizedLocalRefV5::Exact(target)) => {
            source == target
        }
        _ => false,
    };
    content_equal && local_equal
}

#[derive(Serialize)]
struct NormalizedProvenanceBodyV5<'a> {
    content: NormalizedScopedRefBodyV5<'a>,
    local: NormalizedScopedRefBodyV5<'a>,
    revision: &'a NormalizedRevisionRefV5,
    stable_hash: &'a ContentHash,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case", tag = "scope", content = "value")]
enum NormalizedScopedRefBodyV5<'a> {
    Absent,
    TargetTree,
    SelfArtifact,
    ProgramTargets(BTreeSet<StableId>),
    ExactHash(&'a ContentHash),
    ExactString(&'a str),
}

fn normalized_provenance_body_hash(
    provenance: &NormalizedProvenanceV5,
    side: ProgramSideV5,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> M6Result<ContentHash> {
    let content = match &provenance.content {
        NormalizedContentRefV5::Absent => NormalizedScopedRefBodyV5::Absent,
        NormalizedContentRefV5::TargetTree => NormalizedScopedRefBodyV5::TargetTree,
        NormalizedContentRefV5::SelfArtifact => NormalizedScopedRefBodyV5::SelfArtifact,
        NormalizedContentRefV5::ProgramArtifact(id) => {
            NormalizedScopedRefBodyV5::ProgramTargets(normalized_targets(id, side, successors))
        }
        NormalizedContentRefV5::Exact(hash) => NormalizedScopedRefBodyV5::ExactHash(hash),
    };
    let local = match &provenance.local {
        NormalizedLocalRefV5::Absent => NormalizedScopedRefBodyV5::Absent,
        NormalizedLocalRefV5::SelfArtifact => NormalizedScopedRefBodyV5::SelfArtifact,
        NormalizedLocalRefV5::ProgramArtifact(id) => {
            NormalizedScopedRefBodyV5::ProgramTargets(normalized_targets(id, side, successors))
        }
        NormalizedLocalRefV5::Exact(value) => NormalizedScopedRefBodyV5::ExactString(value),
    };
    body_hash(&NormalizedProvenanceBodyV5 {
        content,
        local,
        revision: &provenance.revision,
        stable_hash: &provenance.stable_hash,
    })
}

#[derive(Clone, Debug)]
struct ProgramNodeV5 {
    id: StableId,
    object_kind: ProgramObjectKindV5,
    body_hash: ContentHash,
    data: NodeDataV5,
}

fn id_set_heap(ids: &BTreeSet<StableId>) -> usize {
    ids.len()
        .saturating_mul(std::mem::size_of::<StableId>())
        .saturating_add(ids.iter().map(StableId::allocated_bytes).sum::<usize>())
}

// BTreeSet nodes have allocator metadata and links not reflected by the
// StableId backing allocations.  Keep a conservative per-node charge in the
// M6 peak contract instead of treating a set as a flat ID list.
fn conservative_id_set_heap(ids: &BTreeSet<StableId>) -> M6Result<usize> {
    id_set_heap(ids)
        .checked_add(conservative_btree_node_bytes(ids.len())?)
        .ok_or(M6Error::Incomplete {
            operation: "M6 BTreeSet node ownership",
            limit: MAX_M6_PARTIAL_RERUN_WORKING_BYTES,
            observed: usize::MAX,
        })
}

fn conservative_btree_node_bytes(len: usize) -> M6Result<usize> {
    len.checked_mul(128).ok_or(M6Error::Incomplete {
        operation: "M6 BTreeSet node ownership",
        limit: MAX_M6_PARTIAL_RERUN_WORKING_BYTES,
        observed: usize::MAX,
    })
}

fn id_vec_heap(ids: &Vec<StableId>) -> usize {
    ids.capacity()
        .saturating_mul(std::mem::size_of::<StableId>())
        .saturating_add(ids.iter().map(StableId::allocated_bytes).sum::<usize>())
}

impl ProgramNodeV5 {
    fn allocated_bytes(&self) -> usize {
        let data = match &self.data {
            NodeDataV5::Repository { identity } => identity.capacity(),
            NodeDataV5::Snapshot => 0,
            NodeDataV5::Artifact {
                kind,
                label,
                language,
                path,
                location,
                content_hash,
                anchor,
                base_hash,
                provenance,
                change_fact_ids,
                symbol_ref,
            } => kind
                .capacity()
                .saturating_add(label.capacity())
                .saturating_add(language.as_ref().map_or(0, String::capacity))
                .saturating_add(path.as_ref().map_or(0, String::capacity))
                .saturating_add(location.as_ref().map_or(0, |location| {
                    std::mem::size_of::<crate::Location>()
                        + location.path.capacity()
                        + location
                            .symbol_id
                            .as_ref()
                            .map_or(0, StableId::allocated_bytes)
                }))
                .saturating_add(
                    content_hash
                        .as_ref()
                        .map_or(0, ContentHash::allocated_bytes),
                )
                .saturating_add(anchor.as_ref().map_or(0, |anchor| {
                    anchor.signature_shape_hash.allocated_bytes()
                        + anchor.normalized_body_hash.allocated_bytes()
                }))
                .saturating_add(base_hash.allocated_bytes())
                .saturating_add(provenance.allocated_bytes())
                .saturating_add(id_set_heap(change_fact_ids))
                .saturating_add(symbol_ref.as_ref().map_or(0, StableId::allocated_bytes)),
            NodeDataV5::Relation {
                kind,
                source_id,
                ordered_target_ids,
                base_hash,
                provenance,
                ..
            } => kind
                .capacity()
                .saturating_add(source_id.allocated_bytes())
                .saturating_add(id_vec_heap(ordered_target_ids))
                .saturating_add(base_hash.allocated_bytes())
                .saturating_add(provenance.allocated_bytes()),
            NodeDataV5::Context {
                kind,
                members,
                base_hash,
                provenance,
            } => kind
                .capacity()
                .saturating_add(id_set_heap(members))
                .saturating_add(base_hash.allocated_bytes())
                .saturating_add(provenance.allocated_bytes()),
            NodeDataV5::Invariant {
                property_id,
                scope,
                base_hash,
                provenance,
            } => property_id
                .capacity()
                .saturating_add(id_set_heap(scope))
                .saturating_add(base_hash.allocated_bytes())
                .saturating_add(provenance.allocated_bytes()),
            NodeDataV5::Limitation {
                description,
                sources,
                base_hash,
                ..
            } => description
                .capacity()
                .saturating_add(id_set_heap(sources))
                .saturating_add(base_hash.allocated_bytes()),
        };
        std::mem::size_of::<Self>()
            .saturating_add(self.id.allocated_bytes())
            .saturating_add(self.body_hash.allocated_bytes())
            .saturating_add(data)
    }
}

#[derive(Serialize)]
struct ArtifactNonReferenceBodyV5<'a> {
    attributes: &'a BTreeMap<String, serde_json::Value>,
    kind: &'a str,
    language: &'a Option<String>,
    provenance_stable_hash: &'a ContentHash,
}

#[derive(Serialize)]
struct RelationNonReferenceBodyV5<'a> {
    attributes: &'a BTreeMap<String, serde_json::Value>,
    directed: bool,
    kind: &'a str,
    provenance_stable_hash: &'a ContentHash,
}

#[derive(Serialize)]
struct ContextNonReferenceBodyV5<'a> {
    attributes: &'a BTreeMap<String, serde_json::Value>,
    kind: &'a str,
    label: &'a str,
    provenance_stable_hash: &'a ContentHash,
}

#[derive(Serialize)]
struct InvariantNonReferenceBodyV5<'a> {
    description: &'a str,
    property_id: &'a str,
    provenance_stable_hash: &'a ContentHash,
    severity: crate::Severity,
    verification_mode: &'a Option<String>,
}

#[derive(Serialize)]
struct LimitationNonReferenceBodyV5<'a> {
    description: &'a str,
    kind: crate::LimitationKind,
    related_capabilities: &'a BTreeSet<String>,
    severity: crate::Severity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProgramSideV5 {
    Source,
    Target,
}

#[derive(Serialize)]
struct ProgramSemanticBodyV5<'a> {
    accepted_program_body_hash: &'a ContentHash,
    change_fact_ids: &'a BTreeSet<StableId>,
    ordered_target_ids: Option<&'a Vec<StableId>>,
    rust_anchor: Option<&'a RustSymbolAnchorV1>,
}

fn semantic_program_body_hash(
    accepted_program_body_hash: &ContentHash,
    change_fact_ids: &BTreeSet<StableId>,
    ordered_target_ids: Option<&Vec<StableId>>,
    rust_anchor: Option<&RustSymbolAnchorV1>,
) -> M6Result<ContentHash> {
    body_hash(&ProgramSemanticBodyV5 {
        accepted_program_body_hash,
        change_fact_ids,
        ordered_target_ids,
        rust_anchor,
    })
}

fn program_nodes(
    space: &ProgramSpace,
    side: ProgramSideV5,
    anchors: &BTreeMap<StableId, RustSymbolAnchorV1>,
    relation_orders: &BTreeMap<StableId, Vec<StableId>>,
    git_change_facts: &[GitChangeFactV5],
) -> M6Result<(Vec<ProgramNodeV5>, usize)> {
    crate::canonical::canonical_json_count_bounded(
        &space.streaming_ref(),
        MAX_M6_MAPPING_WORKING_BYTES,
        "M6 ProgramSpace mapping preflight bytes",
    )?;
    let count = [
        2_usize,
        space.artifacts().len(),
        space.relations().len(),
        space.contexts().len(),
        space.invariants().len(),
        space.extraction().limitations.len(),
    ]
    .into_iter()
    .try_fold(0_usize, usize::checked_add)
    .ok_or(M6Error::Incomplete {
        operation: "M6 program domain",
        limit: MAX_M6_PROGRAM_DOMAIN_IDS,
        observed: usize::MAX,
    })?;
    bounded(count, MAX_M6_PROGRAM_DOMAIN_IDS, "M6 program domain")?;
    let mut nodes = Vec::new();
    nodes
        .try_reserve_exact(count)
        .map_err(|_| M6Error::Incomplete {
            operation: "M6 program node allocation",
            limit: MAX_M6_PROGRAM_DOMAIN_IDS,
            observed: usize::MAX,
        })?;
    nodes.push(ProgramNodeV5 {
        id: space.repository_id().clone(),
        object_kind: ProgramObjectKindV5::Repository,
        body_hash: space.m6_repository_body_hash()?,
        data: NodeDataV5::Repository {
            identity: space.repository_identity().to_owned(),
        },
    });
    nodes.push(ProgramNodeV5 {
        id: space.snapshot_id().clone(),
        object_kind: ProgramObjectKindV5::Snapshot,
        body_hash: space.m6_snapshot_body_hash()?,
        data: NodeDataV5::Snapshot,
    });
    let path_artifacts = snapshot_path_artifacts(space);
    for artifact in space.artifacts() {
        let accepted_body_hash = artifact.m6_body_hash()?;
        let change_fact_ids = git_change_facts
            .iter()
            .filter(|fact| match side {
                ProgramSideV5::Source => fact.source_artifact_id == artifact.id,
                ProgramSideV5::Target => fact.target_artifact_id == artifact.id,
            })
            .map(|fact| fact.fact_id.clone())
            .collect();
        let canonical_body_hash = semantic_program_body_hash(
            &accepted_body_hash,
            &change_fact_ids,
            None,
            anchors.get(&artifact.id),
        )?;
        let provenance = normalize_provenance(
            space,
            &artifact.provenance,
            artifact.location.as_ref().map(|value| value.path.as_str()),
            artifact.content_hash.as_ref(),
            &path_artifacts,
        )?;
        let base_hash = body_hash(&ArtifactNonReferenceBodyV5 {
            attributes: &artifact.attributes,
            kind: &artifact.kind,
            language: &artifact.language,
            provenance_stable_hash: &provenance.stable_hash,
        })?;
        nodes.push(ProgramNodeV5 {
            id: artifact.id.clone(),
            object_kind: ProgramObjectKindV5::Artifact,
            body_hash: canonical_body_hash,
            data: NodeDataV5::Artifact {
                kind: artifact.kind.clone(),
                label: artifact.label.clone(),
                language: artifact.language.clone(),
                path: artifact.location.as_ref().map(|value| value.path.clone()),
                location: artifact.location.clone().map(Box::new),
                content_hash: artifact.content_hash.clone(),
                anchor: anchors.get(&artifact.id).cloned(),
                base_hash,
                provenance,
                change_fact_ids,
                symbol_ref: artifact
                    .location
                    .as_ref()
                    .and_then(|value| value.symbol_id.clone()),
            },
        });
    }
    for relation in space.relations() {
        let accepted_body_hash = relation.m6_body_hash()?;
        let no_change_facts = BTreeSet::new();
        let provenance =
            normalize_provenance(space, &relation.provenance, None, None, &path_artifacts)?;
        nodes.push(ProgramNodeV5 {
            id: relation.id.clone(),
            object_kind: ProgramObjectKindV5::Relation,
            body_hash: semantic_program_body_hash(
                &accepted_body_hash,
                &no_change_facts,
                relation_orders.get(&relation.id),
                None,
            )?,
            data: NodeDataV5::Relation {
                kind: relation.kind.clone(),
                directed: relation.directed,
                source_id: relation.source_id.clone(),
                ordered_target_ids: relation_orders[&relation.id].clone(),
                base_hash: body_hash(&RelationNonReferenceBodyV5 {
                    attributes: &relation.attributes,
                    directed: relation.directed,
                    kind: &relation.kind,
                    provenance_stable_hash: &provenance.stable_hash,
                })?,
                provenance,
            },
        });
    }
    for context in space.contexts() {
        let provenance =
            normalize_provenance(space, &context.provenance, None, None, &path_artifacts)?;
        nodes.push(ProgramNodeV5 {
            id: context.id.clone(),
            object_kind: ProgramObjectKindV5::Context,
            body_hash: context.m6_body_hash()?,
            data: NodeDataV5::Context {
                kind: context.kind.clone(),
                members: context.member_ids.clone(),
                base_hash: body_hash(&ContextNonReferenceBodyV5 {
                    attributes: &context.attributes,
                    kind: &context.kind,
                    label: &context.label,
                    provenance_stable_hash: &provenance.stable_hash,
                })?,
                provenance,
            },
        });
    }
    for invariant in space.invariants() {
        let provenance =
            normalize_provenance(space, &invariant.provenance, None, None, &path_artifacts)?;
        nodes.push(ProgramNodeV5 {
            id: invariant.id.clone(),
            object_kind: ProgramObjectKindV5::Invariant,
            body_hash: invariant.m6_body_hash()?,
            data: NodeDataV5::Invariant {
                property_id: invariant.property_id.clone(),
                scope: invariant.scope_ids.clone(),
                base_hash: body_hash(&InvariantNonReferenceBodyV5 {
                    description: &invariant.description,
                    property_id: &invariant.property_id,
                    provenance_stable_hash: &provenance.stable_hash,
                    severity: invariant.severity,
                    verification_mode: &invariant.verification_mode,
                })?,
                provenance,
            },
        });
    }
    for limitation in &space.extraction().limitations {
        nodes.push(ProgramNodeV5 {
            id: limitation.id.clone(),
            object_kind: ProgramObjectKindV5::Limitation,
            body_hash: limitation.m6_body_hash()?,
            data: NodeDataV5::Limitation {
                description: limitation.description.clone(),
                kind: limitation.kind,
                severity: limitation.severity,
                sources: limitation.source_ids.clone(),
                base_hash: body_hash(&LimitationNonReferenceBodyV5 {
                    description: &limitation.description,
                    kind: limitation.kind,
                    related_capabilities: &limitation.related_capabilities,
                    severity: limitation.severity,
                })?,
            },
        });
    }
    nodes.sort_by(|left, right| left.id.cmp(&right.id));
    let retained_working = nodes.iter().try_fold(0_usize, |total, node| {
        checked_working_add(total, node.allocated_bytes())
    })?;
    Ok((nodes, retained_working))
}

#[derive(Clone, Debug)]
struct ComponentSeedV5 {
    object_kind: ProgramObjectKindV5,
    candidate_key_kind: CandidateKeyKindV5,
    from_ids: BTreeSet<StableId>,
    to_ids: BTreeSet<StableId>,
    stage: usize,
}

impl ComponentSeedV5 {
    fn allocated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            .saturating_add(id_set_heap(&self.from_ids))
            .saturating_add(id_set_heap(&self.to_ids))
    }
}

#[allow(clippy::too_many_arguments)] // Each argument is one closed candidate-stage boundary.
fn collect_components<F>(
    source: &[&ProgramNodeV5],
    target: &[&ProgramNodeV5],
    consumed_source: &mut BTreeSet<StableId>,
    consumed_target: &mut BTreeSet<StableId>,
    object_kind: ProgramObjectKindV5,
    candidate_key_kind: CandidateKeyKindV5,
    stage: usize,
    base_working: usize,
    mut edge: F,
) -> M6Result<Vec<ComponentSeedV5>>
where
    F: FnMut(&ProgramNodeV5, &ProgramNodeV5) -> bool,
{
    let source = source
        .iter()
        .filter(|node| !consumed_source.contains(&node.id))
        .copied()
        .collect::<Vec<_>>();
    let target = target
        .iter()
        .filter(|node| !consumed_target.contains(&node.id))
        .copied()
        .collect::<Vec<_>>();
    let mut adjacency_source = vec![Vec::<usize>::new(); source.len()];
    let mut adjacency_target = vec![Vec::<usize>::new(); target.len()];
    let mut working = base_working;
    for (source_index, source_node) in source.iter().enumerate() {
        for (target_index, target_node) in target.iter().enumerate() {
            if edge(source_node, target_node) {
                bounded(
                    adjacency_source[source_index].len() + 1,
                    MAX_M6_MAPPING_SIDE_IDS,
                    "M6 candidate source degree",
                )?;
                bounded(
                    adjacency_target[target_index].len() + 1,
                    MAX_M6_MAPPING_SIDE_IDS,
                    "M6 candidate target degree",
                )?;
                working = checked_working_add(working, 2 * std::mem::size_of::<usize>())?;
                adjacency_source[source_index]
                    .try_reserve(1)
                    .map_err(|_| M6Error::Incomplete {
                        operation: "M6 candidate edge allocation",
                        limit: MAX_M6_MAPPING_WORKING_BYTES,
                        observed: usize::MAX,
                    })?;
                adjacency_target[target_index]
                    .try_reserve(1)
                    .map_err(|_| M6Error::Incomplete {
                        operation: "M6 candidate edge allocation",
                        limit: MAX_M6_MAPPING_WORKING_BYTES,
                        observed: usize::MAX,
                    })?;
                adjacency_source[source_index].push(target_index);
                adjacency_target[target_index].push(source_index);
            }
        }
    }
    let mut seen_source = BTreeSet::new();
    let mut seen_target = BTreeSet::new();
    let mut components = Vec::new();
    let mut max_queue_capacity = 0_usize;
    for start in 0..source.len() {
        if adjacency_source[start].is_empty() || seen_source.contains(&start) {
            continue;
        }
        let mut queue = VecDeque::from([(true, start)]);
        let mut from_ids = BTreeSet::new();
        let mut to_ids = BTreeSet::new();
        while let Some((is_source, index)) = queue.pop_front() {
            max_queue_capacity = max_queue_capacity.max(queue.capacity());
            if is_source {
                if !seen_source.insert(index) {
                    continue;
                }
                from_ids.insert(source[index].id.clone());
                queue.extend(adjacency_source[index].iter().map(|value| (false, *value)));
            } else {
                if !seen_target.insert(index) {
                    continue;
                }
                to_ids.insert(target[index].id.clone());
                queue.extend(adjacency_target[index].iter().map(|value| (true, *value)));
            }
        }
        bounded(
            from_ids.len(),
            MAX_M6_MAPPING_SIDE_IDS,
            "M6 mapping source component",
        )?;
        bounded(
            to_ids.len(),
            MAX_M6_MAPPING_SIDE_IDS,
            "M6 mapping target component",
        )?;
        consumed_source.extend(from_ids.iter().cloned());
        consumed_target.extend(to_ids.iter().cloned());
        components.push(ComponentSeedV5 {
            object_kind,
            candidate_key_kind,
            from_ids,
            to_ids,
            stage,
        });
    }
    components.sort_by(|left, right| {
        left.from_ids
            .iter()
            .next()
            .cmp(&right.from_ids.iter().next())
            .then_with(|| left.to_ids.iter().next().cmp(&right.to_ids.iter().next()))
    });
    let pointer_vectors = source.capacity() * std::mem::size_of::<&ProgramNodeV5>()
        + target.capacity() * std::mem::size_of::<&ProgramNodeV5>();
    let adjacency = adjacency_source.capacity() * std::mem::size_of::<Vec<usize>>()
        + adjacency_target.capacity() * std::mem::size_of::<Vec<usize>>()
        + adjacency_source
            .iter()
            .chain(&adjacency_target)
            .map(|values| values.capacity() * std::mem::size_of::<usize>())
            .sum::<usize>();
    let traversal = (seen_source.len() + seen_target.len()) * std::mem::size_of::<usize>()
        + max_queue_capacity * std::mem::size_of::<(bool, usize)>();
    let returned = components
        .iter()
        .map(ComponentSeedV5::allocated_bytes)
        .sum();
    [pointer_vectors, adjacency, traversal, returned]
        .into_iter()
        .try_fold(base_working, checked_working_add)?;
    Ok(components)
}

fn finish_unmatched(
    source: &[&ProgramNodeV5],
    target: &[&ProgramNodeV5],
    consumed_source: &mut BTreeSet<StableId>,
    consumed_target: &mut BTreeSet<StableId>,
    object_kind: ProgramObjectKindV5,
    stage: usize,
    components: &mut Vec<ComponentSeedV5>,
) {
    for node in source {
        if consumed_source.insert(node.id.clone()) {
            components.push(ComponentSeedV5 {
                object_kind,
                candidate_key_kind: CandidateKeyKindV5::NoCandidate,
                from_ids: BTreeSet::from([node.id.clone()]),
                to_ids: BTreeSet::new(),
                stage,
            });
        }
    }
    for node in target {
        if consumed_target.insert(node.id.clone()) {
            components.push(ComponentSeedV5 {
                object_kind,
                candidate_key_kind: CandidateKeyKindV5::NoCandidate,
                from_ids: BTreeSet::new(),
                to_ids: BTreeSet::from([node.id.clone()]),
                stage,
            });
        }
    }
}

fn successor_map(seeds: &[ComponentSeedV5]) -> BTreeMap<StableId, BTreeSet<StableId>> {
    let mut result = BTreeMap::new();
    for seed in seeds {
        for source in &seed.from_ids {
            result.insert(source.clone(), seed.to_ids.clone());
        }
    }
    result
}

fn id_ref_map_heap(values: &BTreeMap<StableId, &ProgramNodeV5>) -> usize {
    values.len() * std::mem::size_of::<(StableId, &ProgramNodeV5)>()
        + values.keys().map(StableId::allocated_bytes).sum::<usize>()
}

fn successor_map_heap(values: &BTreeMap<StableId, BTreeSet<StableId>>) -> usize {
    values.len() * std::mem::size_of::<(StableId, BTreeSet<StableId>)>()
        + values
            .iter()
            .map(|(id, targets)| id.allocated_bytes() + id_set_heap(targets))
            .sum::<usize>()
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum MappingSideV5 {
    Source,
    Target,
}

fn owner_map_heap(values: &BTreeMap<(MappingSideV5, StableId), (usize, StableId)>) -> usize {
    values.len() * std::mem::size_of::<((MappingSideV5, StableId), (usize, StableId))>()
        + values
            .iter()
            .map(|((_, id), (_, mapping_id))| id.allocated_bytes() + mapping_id.allocated_bytes())
            .sum::<usize>()
}

fn anchor_map_heap(values: &BTreeMap<StableId, RustSymbolAnchorV1>) -> usize {
    values
        .len()
        .saturating_mul(std::mem::size_of::<(StableId, RustSymbolAnchorV1)>())
        .saturating_add(
            values
                .iter()
                .map(|(id, anchor)| {
                    id.allocated_bytes()
                        .saturating_add(anchor.signature_shape_hash.allocated_bytes())
                        .saturating_add(anchor.normalized_body_hash.allocated_bytes())
                })
                .sum(),
        )
}

fn relation_order_map_heap(values: &BTreeMap<StableId, Vec<StableId>>) -> usize {
    values
        .len()
        .saturating_mul(std::mem::size_of::<(StableId, Vec<StableId>)>())
        .saturating_add(
            values
                .iter()
                .map(|(id, targets)| id.allocated_bytes().saturating_add(id_vec_heap(targets)))
                .sum(),
        )
}

fn git_change_facts_heap(values: &[GitChangeFactV5]) -> usize {
    std::mem::size_of_val(values).saturating_add(
        values
            .iter()
            .map(|fact| {
                fact.fact_id
                    .allocated_bytes()
                    .saturating_add(fact.source_artifact_id.allocated_bytes())
                    .saturating_add(fact.target_artifact_id.allocated_bytes())
                    .saturating_add(fact.source_path.capacity())
                    .saturating_add(fact.target_path.capacity())
                    .saturating_add(
                        fact.equal_content_hash
                            .as_ref()
                            .map_or(0, ContentHash::allocated_bytes),
                    )
            })
            .sum(),
    )
}

fn candidate_scratch_upper_bound(source_count: usize, target_count: usize) -> usize {
    let source_edges = source_count.saturating_mul(MAX_M6_MAPPING_SIDE_IDS);
    let target_edges = target_count.saturating_mul(MAX_M6_MAPPING_SIDE_IDS);
    let edges = source_edges.min(target_edges);
    source_count
        .saturating_add(target_count)
        .saturating_mul(std::mem::size_of::<&ProgramNodeV5>())
        .saturating_add(
            source_count
                .saturating_add(target_count)
                .saturating_mul(std::mem::size_of::<Vec<usize>>()),
        )
        .saturating_add(
            edges
                .saturating_mul(2)
                .saturating_mul(std::mem::size_of::<usize>()),
        )
        .saturating_add(
            source_count.saturating_add(target_count).saturating_mul(
                std::mem::size_of::<usize>() + std::mem::size_of::<(bool, usize)>(),
            ),
        )
}

fn mapped_set(
    source_ids: &BTreeSet<StableId>,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> Option<BTreeSet<StableId>> {
    let mut mapped = BTreeSet::new();
    for source_id in source_ids {
        mapped.extend(successors.get(source_id)?.iter().cloned());
    }
    Some(mapped)
}

fn uniquely_mapped_set(
    source_ids: &BTreeSet<StableId>,
    target_ids: &BTreeSet<StableId>,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> bool {
    let mut mapped = BTreeSet::new();
    for source_id in source_ids {
        let Some(targets) = successors.get(source_id) else {
            return false;
        };
        if targets.len() != 1 {
            return false;
        }
        let target_id = targets.iter().next().unwrap();
        if successors
            .values()
            .filter(|candidate_targets| candidate_targets.contains(target_id))
            .count()
            != 1
        {
            return false;
        }
        mapped.insert(target_id.clone());
    }
    mapped == *target_ids
}

fn dependent_references_are_unique(
    source: &ProgramNodeV5,
    target: &ProgramNodeV5,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> bool {
    match (&source.data, &target.data) {
        (
            NodeDataV5::Relation {
                source_id,
                ordered_target_ids,
                ..
            },
            NodeDataV5::Relation {
                source_id: target_source,
                ordered_target_ids: target_targets,
                ..
            },
        ) => {
            let source_refs = std::iter::once(source_id.clone())
                .chain(ordered_target_ids.iter().cloned())
                .collect();
            let target_refs = std::iter::once(target_source.clone())
                .chain(target_targets.iter().cloned())
                .collect();
            uniquely_mapped_set(&source_refs, &target_refs, successors)
        }
        (
            NodeDataV5::Context { members, .. },
            NodeDataV5::Context {
                members: targets, ..
            },
        ) => uniquely_mapped_set(members, targets, successors),
        (NodeDataV5::Invariant { scope, .. }, NodeDataV5::Invariant { scope: targets, .. }) => {
            uniquely_mapped_set(scope, targets, successors)
        }
        (
            NodeDataV5::Limitation { sources, .. },
            NodeDataV5::Limitation {
                sources: targets, ..
            },
        ) => uniquely_mapped_set(sources, targets, successors),
        _ => true,
    }
}

fn artifact_parts(node: &ProgramNodeV5) -> Option<(&str, &str, Option<&str>, Option<&str>)> {
    let NodeDataV5::Artifact {
        kind,
        label,
        language,
        path,
        ..
    } = &node.data
    else {
        return None;
    };
    Some((kind, label, language.as_deref(), path.as_deref()))
}

fn relation_candidate(
    source: &ProgramNodeV5,
    target: &ProgramNodeV5,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> bool {
    let NodeDataV5::Relation {
        kind: source_kind,
        directed: source_directed,
        source_id,
        ordered_target_ids,
        ..
    } = &source.data
    else {
        return false;
    };
    let NodeDataV5::Relation {
        kind: target_kind,
        directed: target_directed,
        source_id: target_source_id,
        ordered_target_ids: target_targets,
        ..
    } = &target.data
    else {
        return false;
    };
    source_kind == target_kind
        && source_directed == target_directed
        && successors
            .get(source_id)
            .is_some_and(|ids| ids.contains(target_source_id))
        && ordered_target_ids.len() == target_targets.len()
        && ordered_target_ids
            .iter()
            .zip(target_targets)
            .all(|(source_id, target_id)| {
                successors
                    .get(source_id)
                    .is_some_and(|ids| ids.contains(target_id))
            })
}

#[derive(Serialize)]
struct RelationTopologySignatureV5<'a> {
    directed: bool,
    kind: &'a str,
    scc_size: usize,
    source_targets: BTreeSet<StableId>,
    target_shapes: Vec<RelationTopologyRefV5<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case", tag = "reference_kind", content = "value")]
enum RelationTopologyRefV5<'a> {
    External(BTreeSet<StableId>),
    Internal(&'a ContentHash),
}

/// Computes an ordered fixed-point topology color for every unresolved
/// relation. External references are normalized to target IDs; internal SCC
/// references use the previous round's color. Running one round per vertex
/// distinguishes finite ordered topology without guessing an ID pairing,
/// while truly symmetric/duplicate SCCs intentionally retain one ambiguous
/// color class.
fn relation_topology_signatures(
    nodes: &[&ProgramNodeV5],
    unresolved: &BTreeSet<StableId>,
    side: ProgramSideV5,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> M6Result<BTreeMap<StableId, ContentHash>> {
    #[derive(Serialize)]
    struct Initial<'a> {
        directed: bool,
        kind: &'a str,
        scc_size: usize,
        target_count: usize,
    }
    let scc_sizes = relation_scc_sizes(nodes, unresolved);
    let mut colors = BTreeMap::new();
    for node in nodes.iter().filter(|node| unresolved.contains(&node.id)) {
        let NodeDataV5::Relation {
            kind,
            directed,
            ordered_target_ids,
            ..
        } = &node.data
        else {
            continue;
        };
        colors.insert(
            node.id.clone(),
            body_hash(&Initial {
                directed: *directed,
                kind,
                scc_size: scc_sizes[&node.id],
                target_count: ordered_target_ids.len(),
            })?,
        );
    }
    for _ in 0..unresolved.len().max(1) {
        let mut next = BTreeMap::new();
        for node in nodes.iter().filter(|node| unresolved.contains(&node.id)) {
            let NodeDataV5::Relation {
                kind,
                directed,
                source_id,
                ordered_target_ids,
                ..
            } = &node.data
            else {
                continue;
            };
            let target_shapes = ordered_target_ids
                .iter()
                .map(|id| {
                    if unresolved.contains(id) {
                        RelationTopologyRefV5::Internal(&colors[id])
                    } else {
                        RelationTopologyRefV5::External(normalized_targets(id, side, successors))
                    }
                })
                .collect();
            next.insert(
                node.id.clone(),
                body_hash(&RelationTopologySignatureV5 {
                    directed: *directed,
                    kind,
                    scc_size: scc_sizes[&node.id],
                    source_targets: normalized_targets(source_id, side, successors),
                    target_shapes,
                })?,
            );
        }
        colors = next;
    }
    Ok(colors)
}

fn relation_scc_sizes(
    nodes: &[&ProgramNodeV5],
    unresolved: &BTreeSet<StableId>,
) -> BTreeMap<StableId, usize> {
    relation_scc_components(nodes, unresolved)
        .into_iter()
        .flat_map(|component| {
            let size = component.len();
            component.into_iter().map(move |id| (id, size))
        })
        .collect()
}

fn relation_scc_components(
    nodes: &[&ProgramNodeV5],
    unresolved: &BTreeSet<StableId>,
) -> Vec<BTreeSet<StableId>> {
    let ids = unresolved.iter().cloned().collect::<Vec<_>>();
    let index_by_id = ids
        .iter()
        .enumerate()
        .map(|(index, id)| (id.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let node_by_id = nodes
        .iter()
        .map(|node| (node.id.clone(), *node))
        .collect::<BTreeMap<_, _>>();
    let adjacency = ids
        .iter()
        .map(|id| match &node_by_id[id].data {
            NodeDataV5::Relation {
                ordered_target_ids, ..
            } => ordered_target_ids
                .iter()
                .filter_map(|target| index_by_id.get(target).copied())
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect::<Vec<_>>();
    // Signature grouping is a quotient of the raw relation SCC graph. That
    // quotient can introduce a cycle even when the raw condensation is a DAG,
    // so collapse the final seed graph again instead of refusing legal input.
    struct Tarjan<'a> {
        adjacency: &'a [Vec<usize>],
        next_index: usize,
        indices: Vec<Option<usize>>,
        lowlink: Vec<usize>,
        stack: Vec<usize>,
        on_stack: Vec<bool>,
        components: Vec<Vec<usize>>,
    }
    impl Tarjan<'_> {
        fn visit(&mut self, vertex: usize) {
            let index = self.next_index;
            self.next_index += 1;
            self.indices[vertex] = Some(index);
            self.lowlink[vertex] = index;
            self.stack.push(vertex);
            self.on_stack[vertex] = true;
            for target in &self.adjacency[vertex] {
                if self.indices[*target].is_none() {
                    self.visit(*target);
                    self.lowlink[vertex] = self.lowlink[vertex].min(self.lowlink[*target]);
                } else if self.on_stack[*target] {
                    self.lowlink[vertex] = self.lowlink[vertex].min(self.indices[*target].unwrap());
                }
            }
            if self.lowlink[vertex] == self.indices[vertex].unwrap() {
                let mut component = Vec::new();
                loop {
                    let member = self.stack.pop().unwrap();
                    self.on_stack[member] = false;
                    component.push(member);
                    if member == vertex {
                        break;
                    }
                }
                self.components.push(component);
            }
        }
    }
    let len = ids.len();
    let mut tarjan = Tarjan {
        adjacency: &adjacency,
        next_index: 0,
        indices: vec![None; len],
        lowlink: vec![0; len],
        stack: Vec::new(),
        on_stack: vec![false; len],
        components: Vec::new(),
    };
    for vertex in 0..len {
        if tarjan.indices[vertex].is_none() {
            tarjan.visit(vertex);
        }
    }
    let mut components = tarjan
        .components
        .into_iter()
        .map(|component| {
            component
                .into_iter()
                .map(|index| ids[index].clone())
                .collect::<BTreeSet<_>>()
        })
        .collect::<Vec<_>>();
    components.sort_by(|left, right| left.iter().next().cmp(&right.iter().next()));
    components
}

fn relation_scc_signature(
    component: &BTreeSet<StableId>,
    node_colors: &BTreeMap<StableId, ContentHash>,
) -> M6Result<ContentHash> {
    let mut colors = component
        .iter()
        .map(|id| node_colors[id].clone())
        .collect::<Vec<_>>();
    colors.sort();
    body_hash(&colors)
}

#[cfg(test)]
fn relation_scc_depths(
    nodes: &[&ProgramNodeV5],
    components: &[BTreeSet<StableId>],
) -> M6Result<Vec<usize>> {
    let membership = components
        .iter()
        .enumerate()
        .flat_map(|(index, component)| component.iter().cloned().map(move |id| (id, index)))
        .collect::<BTreeMap<_, _>>();
    let node_by_id = nodes
        .iter()
        .map(|node| (node.id.clone(), *node))
        .collect::<BTreeMap<_, _>>();
    let dependencies = components
        .iter()
        .enumerate()
        .map(|(index, component)| {
            component
                .iter()
                .flat_map(|id| match &node_by_id[id].data {
                    NodeDataV5::Relation {
                        ordered_target_ids, ..
                    } => ordered_target_ids.as_slice(),
                    _ => &[],
                })
                .filter_map(|target| membership.get(target).copied())
                .filter(|dependency| *dependency != index)
                .collect::<BTreeSet<_>>()
        })
        .collect::<Vec<_>>();
    // Tarjan's component graph is acyclic by construction. Its depth is the
    // only stage offset used for the final grouped seeds.
    fn depth(
        index: usize,
        dependencies: &[BTreeSet<usize>],
        memo: &mut [Option<usize>],
    ) -> M6Result<usize> {
        if let Some(value) = memo[index] {
            return Ok(value);
        }
        let mut value = 0_usize;
        for dependency in &dependencies[index] {
            value = value.max(
                depth(*dependency, dependencies, memo)?
                    .checked_add(1)
                    .ok_or(M6Error::Incomplete {
                        operation: "M6 SCC condensation depth",
                        limit: usize::MAX,
                        observed: usize::MAX,
                    })?,
            );
        }
        memo[index] = Some(value);
        Ok(value)
    }
    let mut memo = vec![None; components.len()];
    for index in 0..components.len() {
        depth(index, &dependencies, &mut memo)?;
    }
    Ok(memo.into_iter().map(Option::unwrap).collect())
}

#[allow(clippy::too_many_arguments)]
fn collect_atomic_relation_sccs(
    source_nodes: &[&ProgramNodeV5],
    target_nodes: &[&ProgramNodeV5],
    unresolved_source: &BTreeSet<StableId>,
    unresolved_target: &BTreeSet<StableId>,
    source_colors: &BTreeMap<StableId, ContentHash>,
    target_colors: &BTreeMap<StableId, ContentHash>,
    consumed_source: &mut BTreeSet<StableId>,
    consumed_target: &mut BTreeSet<StableId>,
    stage: usize,
) -> M6Result<Vec<ComponentSeedV5>> {
    let mut by_signature =
        BTreeMap::<ContentHash, (Vec<BTreeSet<StableId>>, Vec<BTreeSet<StableId>>)>::new();
    let source_components = relation_scc_components(source_nodes, unresolved_source);
    for component in source_components {
        let signature = relation_scc_signature(&component, source_colors)?;
        by_signature.entry(signature).or_default().0.push(component);
    }
    let target_components = relation_scc_components(target_nodes, unresolved_target);
    for component in target_components {
        let signature = relation_scc_signature(&component, target_colors)?;
        by_signature.entry(signature).or_default().1.push(component);
    }
    let mut seeds = Vec::new();
    for (_signature, (source_components, target_components)) in by_signature {
        if source_components.is_empty() || target_components.is_empty() {
            continue;
        }
        let from_ids = source_components
            .into_iter()
            .flatten()
            .collect::<BTreeSet<_>>();
        let to_ids = target_components
            .into_iter()
            .flatten()
            .collect::<BTreeSet<_>>();
        bounded(
            from_ids.len(),
            MAX_M6_MAPPING_SIDE_IDS,
            "M6 atomic relation SCC source IDs",
        )?;
        bounded(
            to_ids.len(),
            MAX_M6_MAPPING_SIDE_IDS,
            "M6 atomic relation SCC target IDs",
        )?;
        consumed_source.extend(from_ids.iter().cloned());
        consumed_target.extend(to_ids.iter().cloned());
        seeds.push(ComponentSeedV5 {
            object_kind: ProgramObjectKindV5::Relation,
            candidate_key_kind: CandidateKeyKindV5::MappedDirectedEndpoints,
            from_ids,
            to_ids,
            stage,
        });
    }
    collapse_and_stage_grouped_relation_seeds(seeds, source_nodes, target_nodes, stage)
}

fn collapse_and_stage_grouped_relation_seeds(
    seeds: Vec<ComponentSeedV5>,
    source_nodes: &[&ProgramNodeV5],
    target_nodes: &[&ProgramNodeV5],
    base_stage: usize,
) -> M6Result<Vec<ComponentSeedV5>> {
    let source_owners = seeds
        .iter()
        .enumerate()
        .flat_map(|(index, seed)| seed.from_ids.iter().cloned().map(move |id| (id, index)))
        .collect::<BTreeMap<_, _>>();
    let target_owners = seeds
        .iter()
        .enumerate()
        .flat_map(|(index, seed)| seed.to_ids.iter().cloned().map(move |id| (id, index)))
        .collect::<BTreeMap<_, _>>();
    let source_by_id = source_nodes
        .iter()
        .map(|node| (node.id.clone(), *node))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target_nodes
        .iter()
        .map(|node| (node.id.clone(), *node))
        .collect::<BTreeMap<_, _>>();
    let mut dependencies = vec![BTreeSet::new(); seeds.len()];
    for (index, seed) in seeds.iter().enumerate() {
        for (ids, nodes, owners) in [
            (&seed.from_ids, &source_by_id, &source_owners),
            (&seed.to_ids, &target_by_id, &target_owners),
        ] {
            for id in ids {
                let node = nodes.get(id).ok_or(M6Error::InvalidMapping(
                    "atomic relation seed references an unknown relation node",
                ))?;
                let NodeDataV5::Relation {
                    ordered_target_ids, ..
                } = &node.data
                else {
                    return Err(M6Error::InvalidMapping(
                        "atomic relation seed contains a non-relation node",
                    ));
                };
                dependencies[index].extend(
                    ordered_target_ids
                        .iter()
                        .filter_map(|target| owners.get(target).copied())
                        .filter(|owner| *owner != index),
                );
            }
        }
    }

    struct Tarjan<'a> {
        dependencies: &'a [BTreeSet<usize>],
        next_index: usize,
        indices: Vec<Option<usize>>,
        lowlink: Vec<usize>,
        stack: Vec<usize>,
        on_stack: Vec<bool>,
        components: Vec<Vec<usize>>,
    }
    impl Tarjan<'_> {
        fn visit(&mut self, vertex: usize) {
            let index = self.next_index;
            self.next_index += 1;
            self.indices[vertex] = Some(index);
            self.lowlink[vertex] = index;
            self.stack.push(vertex);
            self.on_stack[vertex] = true;
            for dependency in &self.dependencies[vertex] {
                if self.indices[*dependency].is_none() {
                    self.visit(*dependency);
                    self.lowlink[vertex] = self.lowlink[vertex].min(self.lowlink[*dependency]);
                } else if self.on_stack[*dependency] {
                    self.lowlink[vertex] = self.lowlink[vertex]
                        .min(self.indices[*dependency].expect("visited dependency has an index"));
                }
            }
            if self.lowlink[vertex] == self.indices[vertex].unwrap() {
                let mut component = Vec::new();
                loop {
                    let member = self.stack.pop().unwrap();
                    self.on_stack[member] = false;
                    component.push(member);
                    if member == vertex {
                        break;
                    }
                }
                component.sort_unstable();
                self.components.push(component);
            }
        }
    }
    let mut tarjan = Tarjan {
        dependencies: &dependencies,
        next_index: 0,
        indices: vec![None; seeds.len()],
        lowlink: vec![0; seeds.len()],
        stack: Vec::new(),
        on_stack: vec![false; seeds.len()],
        components: Vec::new(),
    };
    for index in 0..seeds.len() {
        if tarjan.indices[index].is_none() {
            tarjan.visit(index);
        }
    }
    tarjan
        .components
        .sort_by_key(|component| component.first().copied());
    let seed_component = tarjan
        .components
        .iter()
        .enumerate()
        .flat_map(|(component_index, component)| {
            component
                .iter()
                .copied()
                .map(move |seed_index| (seed_index, component_index))
        })
        .collect::<BTreeMap<_, _>>();
    let mut collapsed = Vec::with_capacity(tarjan.components.len());
    for component in &tarjan.components {
        let from_ids = component
            .iter()
            .flat_map(|index| seeds[*index].from_ids.iter().cloned())
            .collect::<BTreeSet<_>>();
        let to_ids = component
            .iter()
            .flat_map(|index| seeds[*index].to_ids.iter().cloned())
            .collect::<BTreeSet<_>>();
        bounded(
            from_ids.len(),
            MAX_M6_MAPPING_SIDE_IDS,
            "M6 grouped relation condensation source IDs",
        )?;
        bounded(
            to_ids.len(),
            MAX_M6_MAPPING_SIDE_IDS,
            "M6 grouped relation condensation target IDs",
        )?;
        collapsed.push(ComponentSeedV5 {
            object_kind: ProgramObjectKindV5::Relation,
            candidate_key_kind: CandidateKeyKindV5::MappedDirectedEndpoints,
            from_ids,
            to_ids,
            stage: base_stage,
        });
    }
    let collapsed_dependencies = tarjan
        .components
        .iter()
        .enumerate()
        .map(|(component_index, component)| {
            component
                .iter()
                .flat_map(|seed_index| dependencies[*seed_index].iter())
                .map(|dependency| seed_component[dependency])
                .filter(|dependency| *dependency != component_index)
                .collect::<BTreeSet<_>>()
        })
        .collect::<Vec<_>>();

    fn depth(
        index: usize,
        dependencies: &[BTreeSet<usize>],
        memo: &mut [Option<usize>],
    ) -> M6Result<usize> {
        if let Some(value) = memo[index] {
            return Ok(value);
        }
        let mut value = 0_usize;
        for dependency in &dependencies[index] {
            value = value.max(
                depth(*dependency, dependencies, memo)?
                    .checked_add(1)
                    .ok_or(M6Error::Incomplete {
                        operation: "M6 grouped relation SCC condensation depth",
                        limit: usize::MAX,
                        observed: usize::MAX,
                    })?,
            );
        }
        memo[index] = Some(value);
        Ok(value)
    }

    let mut memo = vec![None; collapsed.len()];
    for (index, seed) in collapsed.iter_mut().enumerate() {
        let seed_depth = depth(index, &collapsed_dependencies, &mut memo)?;
        seed.stage = base_stage
            .checked_add(seed_depth)
            .ok_or(M6Error::Incomplete {
                operation: "M6 grouped relation SCC condensation stage",
                limit: usize::MAX,
                observed: usize::MAX,
            })?;
    }
    Ok(collapsed)
}

fn node_references(node: &ProgramNodeV5) -> BTreeSet<StableId> {
    match &node.data {
        NodeDataV5::Artifact { symbol_ref, .. } => symbol_ref.iter().cloned().collect(),
        NodeDataV5::Relation {
            source_id,
            ordered_target_ids,
            ..
        } => std::iter::once(source_id.clone())
            .chain(ordered_target_ids.iter().cloned())
            .collect(),
        NodeDataV5::Context { members, .. } => members.clone(),
        NodeDataV5::Invariant { scope, .. } => scope.clone(),
        NodeDataV5::Limitation { sources, .. } => sources.clone(),
        NodeDataV5::Repository { .. } | NodeDataV5::Snapshot => BTreeSet::new(),
    }
}

#[derive(Serialize)]
struct LocationWithoutSymbolV5<'a> {
    end_column: Option<u64>,
    end_line: Option<u64>,
    path: &'a str,
    start_column: Option<u64>,
    start_line: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case", tag = "object_kind")]
enum NormalizedProgramBodyV5<'a> {
    Repository {
        canonical_body_hash: &'a ContentHash,
    },
    Snapshot {
        canonical_body_hash: &'a ContentHash,
    },
    Artifact {
        anchor: &'a Option<RustSymbolAnchorV1>,
        base_hash: &'a ContentHash,
        change_fact_ids: &'a BTreeSet<StableId>,
        content_hash: &'a Option<ContentHash>,
        label: &'a str,
        location: Option<LocationWithoutSymbolV5<'a>>,
        provenance_hash: ContentHash,
        symbol_targets: Option<BTreeSet<StableId>>,
    },
    Relation {
        base_hash: &'a ContentHash,
        provenance_hash: ContentHash,
        source_targets: BTreeSet<StableId>,
        target_targets: Vec<BTreeSet<StableId>>,
    },
    Context {
        base_hash: &'a ContentHash,
        member_targets: BTreeSet<StableId>,
        provenance_hash: ContentHash,
    },
    Invariant {
        base_hash: &'a ContentHash,
        provenance_hash: ContentHash,
        scope_targets: BTreeSet<StableId>,
    },
    Limitation {
        base_hash: &'a ContentHash,
        source_targets: BTreeSet<StableId>,
    },
}

fn normalized_targets(
    id: &StableId,
    side: ProgramSideV5,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> BTreeSet<StableId> {
    match side {
        ProgramSideV5::Source => successors.get(id).cloned().unwrap_or_default(),
        ProgramSideV5::Target => BTreeSet::from([id.clone()]),
    }
}

fn normalized_program_body_hash(
    node: &ProgramNodeV5,
    side: ProgramSideV5,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> M6Result<ContentHash> {
    let body = match &node.data {
        NodeDataV5::Repository { .. } => NormalizedProgramBodyV5::Repository {
            canonical_body_hash: &node.body_hash,
        },
        NodeDataV5::Snapshot => NormalizedProgramBodyV5::Snapshot {
            canonical_body_hash: &node.body_hash,
        },
        NodeDataV5::Artifact {
            label,
            location,
            content_hash,
            anchor,
            base_hash,
            provenance,
            change_fact_ids,
            symbol_ref,
            ..
        } => NormalizedProgramBodyV5::Artifact {
            anchor,
            base_hash,
            change_fact_ids,
            content_hash,
            label,
            location: location.as_ref().map(|location| LocationWithoutSymbolV5 {
                end_column: location.end_column,
                end_line: location.end_line,
                path: &location.path,
                start_column: location.start_column,
                start_line: location.start_line,
            }),
            provenance_hash: normalized_provenance_body_hash(provenance, side, successors)?,
            symbol_targets: symbol_ref
                .as_ref()
                .map(|id| normalized_targets(id, side, successors)),
        },
        NodeDataV5::Relation {
            source_id,
            ordered_target_ids,
            base_hash,
            provenance,
            ..
        } => NormalizedProgramBodyV5::Relation {
            base_hash,
            provenance_hash: normalized_provenance_body_hash(provenance, side, successors)?,
            source_targets: normalized_targets(source_id, side, successors),
            target_targets: ordered_target_ids
                .iter()
                .map(|id| normalized_targets(id, side, successors))
                .collect(),
        },
        NodeDataV5::Context {
            members,
            base_hash,
            provenance,
            ..
        } => NormalizedProgramBodyV5::Context {
            base_hash,
            member_targets: members
                .iter()
                .flat_map(|id| normalized_targets(id, side, successors))
                .collect(),
            provenance_hash: normalized_provenance_body_hash(provenance, side, successors)?,
        },
        NodeDataV5::Invariant {
            scope,
            base_hash,
            provenance,
            ..
        } => NormalizedProgramBodyV5::Invariant {
            base_hash,
            provenance_hash: normalized_provenance_body_hash(provenance, side, successors)?,
            scope_targets: scope
                .iter()
                .flat_map(|id| normalized_targets(id, side, successors))
                .collect(),
        },
        NodeDataV5::Limitation {
            sources, base_hash, ..
        } => NormalizedProgramBodyV5::Limitation {
            base_hash,
            source_targets: sources
                .iter()
                .flat_map(|id| normalized_targets(id, side, successors))
                .collect(),
        },
    };
    body_hash(&body)
}

fn location_equal_without_symbol(
    left: &Option<Box<crate::Location>>,
    right: &Option<Box<crate::Location>>,
) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.path == right.path
                && left.start_line == right.start_line
                && left.end_line == right.end_line
                && left.start_column == right.start_column
                && left.end_column == right.end_column
        }
        _ => false,
    }
}

fn git_rename_location_equal(
    left: &Option<Box<crate::Location>>,
    right: &Option<Box<crate::Location>>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            left.path != right.path
                && left.start_line == right.start_line
                && left.end_line == right.end_line
                && left.start_column == right.start_column
                && left.end_column == right.end_column
                && left.symbol_id == right.symbol_id
        }
        _ => false,
    }
}

fn path_derived_label(label: &str, location: &Option<Box<crate::Location>>) -> bool {
    location.as_ref().is_some_and(|location| {
        label == location.path
            || location
                .path
                .rsplit('/')
                .next()
                .is_some_and(|basename| label == basename)
    })
}

fn semantic_equal(
    source: &ProgramNodeV5,
    target: &ProgramNodeV5,
    candidate: CandidateKeyKindV5,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> bool {
    match (&source.data, &target.data) {
        (NodeDataV5::Repository { identity: left }, NodeDataV5::Repository { identity: right }) => {
            left == right
        }
        (NodeDataV5::Snapshot, NodeDataV5::Snapshot) => false,
        (
            NodeDataV5::Artifact {
                label: left_label,
                location: left_location,
                content_hash: left_content,
                anchor: left_anchor,
                base_hash: left_base,
                provenance: left_provenance,
                symbol_ref: left_symbol,
                ..
            },
            NodeDataV5::Artifact {
                label: right_label,
                location: right_location,
                content_hash: right_content,
                anchor: right_anchor,
                base_hash: right_base,
                provenance: right_provenance,
                symbol_ref: right_symbol,
                ..
            },
        ) => {
            if left_base != right_base
                || !normalized_provenance_equal(left_provenance, right_provenance, successors)
            {
                return false;
            }
            if left_anchor.is_some() || right_anchor.is_some() {
                if left_anchor != right_anchor {
                    return false;
                }
                if candidate == CandidateKeyKindV5::RustSymbolAnchorV1 {
                    return true;
                }
                let symbol_equal = match (left_symbol, right_symbol) {
                    (None, None) => true,
                    (Some(left), Some(right)) => successors
                        .get(left)
                        .is_some_and(|ids| ids.len() == 1 && ids.contains(right)),
                    _ => false,
                };
                return left_label == right_label
                    && location_equal_without_symbol(left_location, right_location)
                    && symbol_equal;
            }
            if left_content != right_content {
                return false;
            }
            if candidate == CandidateKeyKindV5::GitRenameSameContent {
                return git_rename_location_equal(left_location, right_location)
                    && path_derived_label(left_label, left_location)
                    && path_derived_label(right_label, right_location);
            }
            let symbol_equal = match (left_symbol, right_symbol) {
                (None, None) => true,
                (Some(left), Some(right)) => successors
                    .get(left)
                    .is_some_and(|ids| ids.len() == 1 && ids.contains(right)),
                _ => false,
            };
            left_label == right_label
                && location_equal_without_symbol(left_location, right_location)
                && symbol_equal
        }
        (
            NodeDataV5::Relation {
                source_id,
                ordered_target_ids,
                base_hash: left_base,
                provenance: left_provenance,
                ..
            },
            NodeDataV5::Relation {
                source_id: target_source,
                ordered_target_ids: target_ids,
                base_hash: right_base,
                provenance: right_provenance,
                ..
            },
        ) => {
            left_base == right_base
                && normalized_provenance_equal(left_provenance, right_provenance, successors)
                && successors
                    .get(source_id)
                    .is_some_and(|ids| ids.len() == 1 && ids.contains(target_source))
                && ordered_target_ids.len() == target_ids.len()
                && ordered_target_ids
                    .iter()
                    .zip(target_ids)
                    .all(|(left, right)| {
                        successors
                            .get(left)
                            .is_some_and(|ids| ids.len() == 1 && ids.contains(right))
                    })
        }
        (
            NodeDataV5::Context {
                members,
                base_hash: left,
                provenance: left_provenance,
                ..
            },
            NodeDataV5::Context {
                members: target_members,
                base_hash: right,
                provenance: right_provenance,
                ..
            },
        ) => {
            left == right
                && normalized_provenance_equal(left_provenance, right_provenance, successors)
                && mapped_set(members, successors).as_ref() == Some(target_members)
        }
        (
            NodeDataV5::Invariant {
                scope,
                base_hash: left,
                provenance: left_provenance,
                ..
            },
            NodeDataV5::Invariant {
                scope: target_scope,
                base_hash: right,
                provenance: right_provenance,
                ..
            },
        ) => {
            left == right
                && normalized_provenance_equal(left_provenance, right_provenance, successors)
                && mapped_set(scope, successors).as_ref() == Some(target_scope)
        }
        (
            NodeDataV5::Limitation {
                sources,
                base_hash: left,
                ..
            },
            NodeDataV5::Limitation {
                sources: target_sources,
                base_hash: right,
                ..
            },
        ) => left == right && mapped_set(sources, successors).as_ref() == Some(target_sources),
        _ => false,
    }
}

fn component_status(
    seed: &ComponentSeedV5,
    source_by_id: &BTreeMap<StableId, &ProgramNodeV5>,
    target_by_id: &BTreeMap<StableId, &ProgramNodeV5>,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> MappingStatusV5 {
    match (seed.from_ids.len(), seed.to_ids.len()) {
        (0, 1) => MappingStatusV5::Added,
        (1, 0) => MappingStatusV5::Removed,
        (1, 1) => {
            let source = source_by_id[seed.from_ids.iter().next().unwrap()];
            let target = target_by_id[seed.to_ids.iter().next().unwrap()];
            if !dependent_references_are_unique(source, target, successors) {
                MappingStatusV5::Unresolved
            } else if semantic_equal(source, target, seed.candidate_key_kind, successors) {
                MappingStatusV5::Preserved
            } else {
                MappingStatusV5::Modified
            }
        }
        (1, _) => MappingStatusV5::Split,
        (_, 1) => MappingStatusV5::Merged,
        _ => MappingStatusV5::Unresolved,
    }
}

#[derive(Clone, Debug)]
pub struct M6MappingPhaseV5 {
    mappings: Vec<ProgramMappingV5>,
    morphism: ChangeMorphismV5,
    working_peak_upper_bound_bytes: usize,
}

impl M6MappingPhaseV5 {
    pub fn mappings(&self) -> &[ProgramMappingV5] {
        &self.mappings
    }
    fn mappings_capacity(&self) -> usize {
        self.mappings.capacity()
    }
    pub fn morphism(&self) -> &ChangeMorphismV5 {
        &self.morphism
    }
    pub fn working_peak_upper_bound_bytes(&self) -> usize {
        self.working_peak_upper_bound_bytes
    }

    /// Rebuilds the minimal structural proposal view needed by the terminal
    /// crash-recovery reducer.  This is deliberately crate-private: callers
    /// cannot turn durable mapping rows into an appendable mapping phase.
    /// The caller must still recompute and compare every row from roots/CAS
    /// before it can use the result for recovery.
    pub(crate) fn from_durable_morphism_for_recovery(morphism: ChangeMorphismV5) -> M6Result<Self> {
        Ok(Self {
            // The durable mapping entries are deliberately not used as an
            // authority source. The terminal recovery immediately derives
            // the complete phase again from accepted program facts.
            mappings: Vec::new(),
            morphism,
            // This temporary value is never admitted as a mapping
            // reservation. The recovery reducer derives a fresh phase before
            // it compares the durable rows.
            working_peak_upper_bound_bytes: 0,
        })
    }

    /// Compares replayed canonical DTOs against this freshly recomputed phase;
    /// replay bytes can never select or alter mappings.
    #[doc(hidden)]
    pub fn validate_replayed_canonical(
        &self,
        mapping_bytes: &[Vec<u8>],
        morphism_bytes: &[u8],
    ) -> M6Result<()> {
        if mapping_bytes.len() != self.mappings.len() {
            return Err(M6Error::InvalidWire(
                "replayed mapping count differs from recomputed phase".to_owned(),
            ));
        }
        for (bytes, expected) in mapping_bytes.iter().zip(&self.mappings) {
            let actual = ProgramMappingV5::from_json_bytes(bytes)?;
            if actual != *expected {
                return Err(M6Error::InvalidWire(
                    "replayed mapping differs from recomputed phase".to_owned(),
                ));
            }
        }
        let actual = ChangeMorphismV5::from_json_bytes(morphism_bytes, &self.morphism)?;
        if actual != self.morphism {
            return Err(M6Error::InvalidWire(
                "replayed morphism differs from recomputed phase".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct ChangeMorphismIdentityV5<'a> {
    source_closure_id: &'a StableId,
    repository_id: &'a StableId,
    source_snapshot_id: &'a StableId,
    target_snapshot_id: &'a StableId,
    mapping_policy_descriptor_id: &'static str,
    semantic_anchor_descriptor_id: &'static str,
    mapping_count: u64,
    mapping_set_digest: &'a ContentHash,
    source_domain_count: u64,
    source_domain_digest: &'a ContentHash,
    target_domain_count: u64,
    target_domain_digest: &'a ContentHash,
    status_counts: &'a MappingStatusCountsV5,
    source_ids: &'a BTreeSet<StableId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ChangeMorphismV5 {
    schema: &'static str,
    id: StableId,
    source_closure_id: StableId,
    repository_id: StableId,
    source_snapshot_id: StableId,
    target_snapshot_id: StableId,
    mapping_policy_descriptor_id: &'static str,
    semantic_anchor_descriptor_id: &'static str,
    mapping_count: u64,
    mapping_set_digest: ContentHash,
    source_domain_count: u64,
    source_domain_digest: ContentHash,
    target_domain_count: u64,
    target_domain_digest: ContentHash,
    status_counts: MappingStatusCountsV5,
    source_ids: BTreeSet<StableId>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ChangeMorphismWireV5 {
    schema: String,
    id: StableId,
    source_closure_id: StableId,
    repository_id: StableId,
    source_snapshot_id: StableId,
    target_snapshot_id: StableId,
    mapping_policy_descriptor_id: String,
    semantic_anchor_descriptor_id: String,
    mapping_count: u64,
    mapping_set_digest: ContentHash,
    source_domain_count: u64,
    source_domain_digest: ContentHash,
    target_domain_count: u64,
    target_domain_digest: ContentHash,
    status_counts: MappingStatusCountsV5,
    source_ids: BTreeSet<StableId>,
}

impl ChangeMorphismV5 {
    pub(crate) fn from_projection_event_json(input: &[u8]) -> M6Result<Self> {
        Self::validate_event_wire(input)?;
        let wire: ChangeMorphismWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        Ok(Self {
            schema: "reviewgraphen.change_morphism.v5",
            id: wire.id,
            source_closure_id: wire.source_closure_id,
            repository_id: wire.repository_id,
            source_snapshot_id: wire.source_snapshot_id,
            target_snapshot_id: wire.target_snapshot_id,
            mapping_policy_descriptor_id: PROGRAM_MAPPING_POLICY_V5,
            semantic_anchor_descriptor_id: RUST_SYMBOL_ANCHOR_V1,
            mapping_count: wire.mapping_count,
            mapping_set_digest: wire.mapping_set_digest,
            source_domain_count: wire.source_domain_count,
            source_domain_digest: wire.source_domain_digest,
            target_domain_count: wire.target_domain_count,
            target_domain_digest: wire.target_domain_digest,
            status_counts: wire.status_counts,
            source_ids: wire.source_ids,
        })
    }

    /// Compares a durable seal body with this freshly recomputed expected
    /// morphism.  The raw body grants no authority by itself.
    pub(crate) fn canonical_body_matches(&self, input: &[u8]) -> M6Result<bool> {
        Ok(crate::canonical_json(self)? == input)
    }

    pub(crate) fn validate_event_wire(input: &[u8]) -> M6Result<()> {
        preflight_event_line(input.len(), 1)?;
        bounded(
            input.len(),
            MAX_M6_MORPHISM_DTO_BYTES,
            "M6 morphism event JSON bytes",
        )?;
        let wire: ChangeMorphismWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        let input_value: serde_json::Value = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        let normalized =
            serde_json::to_value(&wire).map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        if input_value != normalized {
            return Err(M6Error::InvalidWire(
                "morphism event wire contains an unknown, duplicate, or lossy field".to_owned(),
            ));
        }
        if wire.schema != "reviewgraphen.change_morphism.v5"
            || wire.mapping_policy_descriptor_id != PROGRAM_MAPPING_POLICY_V5
            || wire.semantic_anchor_descriptor_id != RUST_SYMBOL_ANCHOR_V1
        {
            return Err(M6Error::InvalidWire(
                "morphism event wire has a wrong schema or fixed descriptor".to_owned(),
            ));
        }
        require_kind(
            &wire.source_closure_id,
            "incremental-source-closure-v5",
            "source_closure_id",
        )?;
        require_kind(&wire.repository_id, "repository", "repository_id")?;
        require_kind(&wire.source_snapshot_id, "snapshot", "source_snapshot_id")?;
        require_kind(&wire.target_snapshot_id, "snapshot", "target_snapshot_id")?;
        for (field, digest) in [
            ("mapping_set_digest", &wire.mapping_set_digest),
            ("source_domain_digest", &wire.source_domain_digest),
            ("target_domain_digest", &wire.target_domain_digest),
        ] {
            full_sha256(field, digest)?;
        }
        let expected_source_ids = BTreeSet::from([wire.source_closure_id.clone()]);
        if wire.mapping_count == 0
            || wire.mapping_count > MAX_M6_MAPPINGS as u64
            || wire.source_domain_count == 0
            || wire.source_domain_count > MAX_M6_PROGRAM_DOMAIN_IDS as u64
            || wire.target_domain_count == 0
            || wire.target_domain_count > MAX_M6_PROGRAM_DOMAIN_IDS as u64
            || wire.status_counts.checked_total() != Some(wire.mapping_count)
            || wire.source_ids != expected_source_ids
        {
            return Err(M6Error::InvalidWire(
                "morphism event wire has inconsistent counts or source IDs".to_owned(),
            ));
        }
        let expected_id = derive(
            "change-morphism-v5",
            &ChangeMorphismIdentityV5 {
                source_closure_id: &wire.source_closure_id,
                repository_id: &wire.repository_id,
                source_snapshot_id: &wire.source_snapshot_id,
                target_snapshot_id: &wire.target_snapshot_id,
                mapping_policy_descriptor_id: PROGRAM_MAPPING_POLICY_V5,
                semantic_anchor_descriptor_id: RUST_SYMBOL_ANCHOR_V1,
                mapping_count: wire.mapping_count,
                mapping_set_digest: &wire.mapping_set_digest,
                source_domain_count: wire.source_domain_count,
                source_domain_digest: &wire.source_domain_digest,
                target_domain_count: wire.target_domain_count,
                target_domain_digest: &wire.target_domain_digest,
                status_counts: &wire.status_counts,
                source_ids: &wire.source_ids,
            },
        )?;
        if wire.id != expected_id {
            return Err(M6Error::InvalidWire(
                "morphism event wire derived ID does not match its complete body".to_owned(),
            ));
        }
        Ok(())
    }

    /// Deterministic reservation admitted by Store before any mapping graph is
    /// allocated. The multiplier covers retained node copies, ownership maps,
    /// successor/predecessor IDs, mapping DTOs and seal domains; candidate
    /// adjacency has its separate exact cardinality bound.
    #[doc(hidden)]
    pub fn mapping_reservation_bytes_from_accepted_program_facts(
        source: &ProgramSpace,
        target: &ProgramSpace,
    ) -> M6Result<usize> {
        if source.accepted_git_revision_closure().is_none()
            || target.accepted_git_revision_closure().is_none()
            || source.accepted_rust_symbol_anchors().is_none()
            || target.accepted_rust_symbol_anchors().is_none()
            || source.accepted_relation_target_order().is_none()
            || target.accepted_relation_target_order().is_none()
        {
            return Err(M6Error::InvalidMapping(
                "mapping reservation requires accepted ProgramSpace v3 incremental facts",
            ));
        }
        Self::mapping_reservation_bytes(source, target)
    }

    fn mapping_reservation_bytes(source: &ProgramSpace, target: &ProgramSpace) -> M6Result<usize> {
        let domain_count = |space: &ProgramSpace| {
            [
                2_usize,
                space.artifacts().len(),
                space.relations().len(),
                space.contexts().len(),
                space.invariants().len(),
                space.extraction().limitations.len(),
            ]
            .into_iter()
            .try_fold(0_usize, usize::checked_add)
            .ok_or(M6Error::Incomplete {
                operation: "M6 mapping reservation domain arithmetic",
                limit: MAX_M6_PROGRAM_DOMAIN_IDS,
                observed: usize::MAX,
            })
        };
        let source_count = domain_count(source)?;
        let target_count = domain_count(target)?;
        bounded(source_count, MAX_M6_PROGRAM_DOMAIN_IDS, "M6 source domain")?;
        bounded(target_count, MAX_M6_PROGRAM_DOMAIN_IDS, "M6 target domain")?;
        crate::canonical::canonical_json_count_bounded(
            &source.streaming_ref(),
            MAX_M6_CANONICAL_BYTES,
            "M6 source ProgramSpace reservation bytes",
        )?;
        crate::canonical::canonical_json_count_bounded(
            &target.streaming_ref(),
            MAX_M6_CANONICAL_BYTES,
            "M6 target ProgramSpace reservation bytes",
        )?;
        let owned = source
            .allocated_bytes()
            .checked_add(target.allocated_bytes())
            .and_then(|bytes| bytes.checked_mul(MAX_M6_MAPPING_SIDE_IDS))
            .ok_or(M6Error::Incomplete {
                operation: "M6 mapping reservation arithmetic",
                limit: MAX_M6_MAPPING_WORKING_BYTES,
                observed: usize::MAX,
            })?;
        let reservation = [
            owned,
            candidate_scratch_upper_bound(source_count, target_count),
            MAX_M6_CANONICAL_BYTES,
            MAX_M6_MAPPING_DTO_BYTES,
            MAX_M6_MORPHISM_DTO_BYTES,
        ]
        .into_iter()
        .try_fold(0_usize, checked_working_add)?;
        bounded(
            reservation,
            MAX_M6_MAPPING_WORKING_BYTES,
            "M6 mapping reservation bytes",
        )?;
        Ok(reservation)
    }

    /// Recomputes the complete deterministic mapping phase from accepted
    /// ProgramSpace facts. Missing anchor/order evidence is a typed refusal.
    #[doc(hidden)]
    pub fn derive_from_accepted_program_facts(
        closure: &IncrementalSourceClosureV5,
        source: &ProgramSpace,
        target: &ProgramSpace,
    ) -> M6Result<M6MappingPhaseV5> {
        let inputs =
            ValidatedIncrementalInputsV5::derive_from_accepted_program_facts(source, target)?;
        Self::build_with_inputs(closure, source, target, &inputs)
    }

    fn allocated_bytes(&self) -> usize {
        [
            std::mem::size_of::<Self>(),
            self.id.allocated_bytes(),
            self.source_closure_id.allocated_bytes(),
            self.repository_id.allocated_bytes(),
            self.source_snapshot_id.allocated_bytes(),
            self.target_snapshot_id.allocated_bytes(),
            self.mapping_set_digest.allocated_bytes(),
            self.source_domain_digest.allocated_bytes(),
            self.target_domain_digest.allocated_bytes(),
            id_set_heap(&self.source_ids),
        ]
        .into_iter()
        .fold(0_usize, usize::saturating_add)
    }

    fn build_with_inputs(
        closure: &IncrementalSourceClosureV5,
        source: &ProgramSpace,
        target: &ProgramSpace,
        inputs: &ValidatedIncrementalInputsV5,
    ) -> M6Result<M6MappingPhaseV5> {
        if source.snapshot_id() != &inputs.source_snapshot_id
            || target.snapshot_id() != &inputs.target_snapshot_id
            || source.snapshot_id() != &closure.input.source_snapshot_id
            || target.snapshot_id() != &closure.input.target_snapshot_id
            || source.repository_id() != target.repository_id()
            || source.repository_id() != &closure.input.repository_id
        {
            return Err(M6Error::InvalidMapping(
                "mapping proof, closure, and ProgramSpaces must bind the same snapshots",
            ));
        }
        let input_working = [
            source.allocated_bytes(),
            target.allocated_bytes(),
            anchor_map_heap(&inputs.rust_source_anchors),
            anchor_map_heap(&inputs.rust_target_anchors),
            relation_order_map_heap(&inputs.source_relation_target_order),
            relation_order_map_heap(&inputs.target_relation_target_order),
            git_change_facts_heap(&inputs.git_change_facts),
        ]
        .into_iter()
        .try_fold(0_usize, checked_working_add)?;
        let (source_nodes, source_working) = program_nodes(
            source,
            ProgramSideV5::Source,
            &inputs.rust_source_anchors,
            &inputs.source_relation_target_order,
            &inputs.git_change_facts,
        )?;
        let (target_nodes, target_working) = program_nodes(
            target,
            ProgramSideV5::Target,
            &inputs.rust_target_anchors,
            &inputs.target_relation_target_order,
            &inputs.git_change_facts,
        )?;
        let node_working = [source_working, target_working]
            .into_iter()
            .try_fold(input_working, checked_working_add)?;
        let source_by_id = source_nodes
            .iter()
            .map(|node| (node.id.clone(), node))
            .collect::<BTreeMap<_, _>>();
        let target_by_id = target_nodes
            .iter()
            .map(|node| (node.id.clone(), node))
            .collect::<BTreeMap<_, _>>();
        let source_kind = |kind| {
            source_nodes
                .iter()
                .filter(|node| node.object_kind == kind)
                .collect::<Vec<_>>()
        };
        let target_kind = |kind| {
            target_nodes
                .iter()
                .filter(|node| node.object_kind == kind)
                .collect::<Vec<_>>()
        };
        let mut consumed_source = BTreeSet::new();
        let mut consumed_target = BTreeSet::new();
        let mut seeds = Vec::new();

        let repositories_source = source_kind(ProgramObjectKindV5::Repository);
        let repositories_target = target_kind(ProgramObjectKindV5::Repository);
        seeds.extend(collect_components(
            &repositories_source, &repositories_target, &mut consumed_source, &mut consumed_target,
            ProgramObjectKindV5::Repository, CandidateKeyKindV5::RepositoryIdentity, 0, node_working,
            |left, right| matches!((&left.data, &right.data),
                (NodeDataV5::Repository { identity: a }, NodeDataV5::Repository { identity: b }) if a == b),
        )?);
        finish_unmatched(
            &repositories_source,
            &repositories_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Repository,
            0,
            &mut seeds,
        );

        let snapshots_source = source_kind(ProgramObjectKindV5::Snapshot);
        let snapshots_target = target_kind(ProgramObjectKindV5::Snapshot);
        seeds.extend(collect_components(
            &snapshots_source,
            &snapshots_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Snapshot,
            CandidateKeyKindV5::SnapshotPair,
            1,
            node_working,
            |_, _| true,
        )?);

        let artifacts_source = source_kind(ProgramObjectKindV5::Artifact);
        let artifacts_target = target_kind(ProgramObjectKindV5::Artifact);
        let copied_target_ids = inputs
            .git_change_facts
            .iter()
            .filter(|fact| fact.kind == GitChangeKindV5::Copied)
            .map(|fact| fact.target_artifact_id.clone())
            .collect::<BTreeSet<_>>();
        seeds.extend(collect_components(
            &artifacts_source,
            &artifacts_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Artifact,
            CandidateKeyKindV5::SamePath,
            2,
            node_working,
            |left, right| {
                !copied_target_ids.contains(&right.id)
                    && matches!((artifact_parts(left), artifact_parts(right)),
                    (Some(("file", _, _, Some(a))), Some(("file", _, _, Some(b)))) if a == b)
            },
        )?);
        seeds.extend(collect_components(
            &artifacts_source,
            &artifacts_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Artifact,
            CandidateKeyKindV5::GitRenameSameContent,
            3,
            node_working,
            |left, right| {
                inputs.git_change_facts.iter().any(|fact| {
                    fact.kind == GitChangeKindV5::Renamed
                        && fact.equal_content_hash.is_some()
                        && fact.source_artifact_id == left.id
                        && fact.target_artifact_id == right.id
                })
            },
        )?);
        seeds.extend(collect_components(
            &artifacts_source,
            &artifacts_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Artifact,
            CandidateKeyKindV5::SamePath,
            4,
            node_working,
            |left, right| match (artifact_parts(left), artifact_parts(right)) {
                (
                    Some((left_kind, left_label, left_language, left_path)),
                    Some((right_kind, right_label, right_language, right_path)),
                ) => {
                    !copied_target_ids.contains(&right.id)
                        && matches!(left_kind, "function" | "method" | "type")
                        && left_language == Some("rust")
                        && right_language == Some("rust")
                        && left_kind == right_kind
                        && left_label == right_label
                        && left_path == right_path
                }
                _ => false,
            },
        )?);
        seeds.extend(collect_components(
            &artifacts_source,
            &artifacts_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Artifact,
            CandidateKeyKindV5::RustSymbolAnchorV1,
            5,
            node_working,
            |left, right| match (&left.data, &right.data) {
                (
                    NodeDataV5::Artifact {
                        anchor: Some(a), ..
                    },
                    NodeDataV5::Artifact {
                        anchor: Some(b), ..
                    },
                ) => !copied_target_ids.contains(&right.id) && a == b,
                _ => false,
            },
        )?);
        seeds.extend(collect_components(
            &artifacts_source,
            &artifacts_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Artifact,
            CandidateKeyKindV5::SameKindLabelLanguageLocation,
            6,
            node_working,
            |left, right| match (&left.data, &right.data) {
                (
                    NodeDataV5::Artifact {
                        kind: a_kind,
                        label: a_label,
                        language: a_language,
                        location: a_location,
                        anchor: None,
                        ..
                    },
                    NodeDataV5::Artifact {
                        kind: b_kind,
                        label: b_label,
                        language: b_language,
                        location: b_location,
                        anchor: None,
                        ..
                    },
                ) => {
                    !copied_target_ids.contains(&right.id)
                        && a_kind != "file"
                        && b_kind != "file"
                        && a_kind == b_kind
                        && a_label == b_label
                        && a_language == b_language
                        && a_location == b_location
                }
                _ => false,
            },
        )?);
        finish_unmatched(
            &artifacts_source,
            &artifacts_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Artifact,
            7,
            &mut seeds,
        );

        let mut successors = successor_map(&seeds);
        let relations_source = source_kind(ProgramObjectKindV5::Relation);
        let relations_target = target_kind(ProgramObjectKindV5::Relation);
        let mut relation_stage = 8_usize;
        loop {
            let wave = collect_components(
                &relations_source,
                &relations_target,
                &mut consumed_source,
                &mut consumed_target,
                ProgramObjectKindV5::Relation,
                CandidateKeyKindV5::MappedDirectedEndpoints,
                relation_stage,
                node_working,
                |left, right| relation_candidate(left, right, &successors),
            )?;
            if wave.is_empty() {
                break;
            }
            seeds.extend(wave);
            successors = successor_map(&seeds);
            relation_stage = relation_stage.checked_add(1).ok_or(M6Error::Incomplete {
                operation: "M6 relation dependency stages",
                limit: usize::MAX,
                observed: usize::MAX,
            })?;
        }
        // A legal relation graph may contain relation-to-relation SCCs. No
        // member of such an SCC can be admitted by the acyclic wave above,
        // because each waits for another member's successor. Build the
        // remaining bipartite fixed-point graph while treating only remaining
        // relation references as unresolved variables. Connected ambiguity is
        // retained as one exact component; it is never degraded into unrelated
        // removed/added records.
        let unresolved_source_relations = relations_source
            .iter()
            .filter(|node| !consumed_source.contains(&node.id))
            .map(|node| node.id.clone())
            .collect::<BTreeSet<_>>();
        let unresolved_target_relations = relations_target
            .iter()
            .filter(|node| !consumed_target.contains(&node.id))
            .map(|node| node.id.clone())
            .collect::<BTreeSet<_>>();
        let source_topology = relation_topology_signatures(
            &relations_source,
            &unresolved_source_relations,
            ProgramSideV5::Source,
            &successors,
        )?;
        let target_topology = relation_topology_signatures(
            &relations_target,
            &unresolved_target_relations,
            ProgramSideV5::Target,
            &successors,
        )?;
        let cyclic_wave = collect_atomic_relation_sccs(
            &relations_source,
            &relations_target,
            &unresolved_source_relations,
            &unresolved_target_relations,
            &source_topology,
            &target_topology,
            &mut consumed_source,
            &mut consumed_target,
            relation_stage,
        )?;
        if !cyclic_wave.is_empty() {
            let last_cyclic_stage = cyclic_wave.iter().map(|seed| seed.stage).max().unwrap();
            seeds.extend(cyclic_wave);
            relation_stage = last_cyclic_stage
                .checked_add(1)
                .ok_or(M6Error::Incomplete {
                    operation: "M6 cyclic relation dependency stage",
                    limit: usize::MAX,
                    observed: usize::MAX,
                })?;
        }
        finish_unmatched(
            &relations_source,
            &relations_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Relation,
            relation_stage,
            &mut seeds,
        );
        successors = successor_map(&seeds);
        let context_stage = relation_stage.checked_add(1).ok_or(M6Error::Incomplete {
            operation: "M6 dependency stages",
            limit: usize::MAX,
            observed: usize::MAX,
        })?;

        let contexts_source = source_kind(ProgramObjectKindV5::Context);
        let contexts_target = target_kind(ProgramObjectKindV5::Context);
        seeds.extend(collect_components(
            &contexts_source,
            &contexts_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Context,
            CandidateKeyKindV5::MappedMembers,
            context_stage,
            node_working,
            |left, right| match (&left.data, &right.data) {
                (
                    NodeDataV5::Context {
                        kind: a, members, ..
                    },
                    NodeDataV5::Context {
                        kind: b,
                        members: target_members,
                        ..
                    },
                ) => a == b && mapped_set(members, &successors).as_ref() == Some(target_members),
                _ => false,
            },
        )?);
        finish_unmatched(
            &contexts_source,
            &contexts_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Context,
            context_stage,
            &mut seeds,
        );
        successors = successor_map(&seeds);
        let invariant_stage = context_stage.checked_add(1).ok_or(M6Error::Incomplete {
            operation: "M6 dependency stages",
            limit: usize::MAX,
            observed: usize::MAX,
        })?;

        let invariants_source = source_kind(ProgramObjectKindV5::Invariant);
        let invariants_target = target_kind(ProgramObjectKindV5::Invariant);
        seeds.extend(collect_components(
            &invariants_source,
            &invariants_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Invariant,
            CandidateKeyKindV5::MappedScope,
            invariant_stage,
            node_working,
            |left, right| match (&left.data, &right.data) {
                (
                    NodeDataV5::Invariant {
                        property_id: a,
                        scope,
                        ..
                    },
                    NodeDataV5::Invariant {
                        property_id: b,
                        scope: target_scope,
                        ..
                    },
                ) => a == b && mapped_set(scope, &successors).as_ref() == Some(target_scope),
                _ => false,
            },
        )?);
        finish_unmatched(
            &invariants_source,
            &invariants_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Invariant,
            invariant_stage,
            &mut seeds,
        );
        successors = successor_map(&seeds);
        let limitation_base_stage = invariant_stage.checked_add(1).ok_or(M6Error::Incomplete {
            operation: "M6 dependency stages",
            limit: usize::MAX,
            observed: usize::MAX,
        })?;

        let limitations_source = source_kind(ProgramObjectKindV5::Limitation);
        let limitations_target = target_kind(ProgramObjectKindV5::Limitation);
        let mut limitation_stage = limitation_base_stage;
        loop {
            let wave = collect_components(
                &limitations_source,
                &limitations_target,
                &mut consumed_source,
                &mut consumed_target,
                ProgramObjectKindV5::Limitation,
                CandidateKeyKindV5::MappedLimitationSources,
                limitation_stage,
                node_working,
                |left, right| match (&left.data, &right.data) {
                    (
                        NodeDataV5::Limitation {
                            base_hash: a,
                            sources,
                            ..
                        },
                        NodeDataV5::Limitation {
                            base_hash: b,
                            sources: target_sources,
                            ..
                        },
                    ) => {
                        a == b && mapped_set(sources, &successors).as_ref() == Some(target_sources)
                    }
                    _ => false,
                },
            )?;
            if wave.is_empty() {
                break;
            }
            seeds.extend(wave);
            successors = successor_map(&seeds);
            limitation_stage = limitation_stage.checked_add(1).ok_or(M6Error::Incomplete {
                operation: "M6 limitation dependency stages",
                limit: usize::MAX,
                observed: usize::MAX,
            })?;
        }
        finish_unmatched(
            &limitations_source,
            &limitations_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Limitation,
            limitation_stage,
            &mut seeds,
        );
        successors = successor_map(&seeds);

        seeds.sort_by(|left, right| {
            left.stage
                .cmp(&right.stage)
                .then_with(|| {
                    left.from_ids
                        .iter()
                        .next()
                        .cmp(&right.from_ids.iter().next())
                })
                .then_with(|| left.to_ids.iter().next().cmp(&right.to_ids.iter().next()))
        });
        bounded(seeds.len(), MAX_M6_MAPPINGS, "M6 program mappings")?;
        let mut mappings = Vec::new();
        mappings
            .try_reserve_exact(seeds.len())
            .map_err(|_| M6Error::Incomplete {
                operation: "M6 mapping allocation",
                limit: MAX_M6_MAPPINGS,
                observed: usize::MAX,
            })?;
        let mut owner = BTreeMap::<(MappingSideV5, StableId), (usize, StableId)>::new();
        for seed in &seeds {
            let status = component_status(seed, &source_by_id, &target_by_id, &successors);
            let mut predecessor_mapping_ids = BTreeSet::new();
            for (side, _id, node) in seed
                .from_ids
                .iter()
                .map(|id| (MappingSideV5::Source, id, source_by_id[id]))
                .chain(
                    seed.to_ids
                        .iter()
                        .map(|id| (MappingSideV5::Target, id, target_by_id[id])),
                )
            {
                for reference in node_references(node) {
                    if let Some((stage, mapping_id)) = owner.get(&(side, reference))
                        && *stage < seed.stage
                    {
                        predecessor_mapping_ids.insert(mapping_id.clone());
                    }
                }
            }
            let change_fact_ids = inputs
                .git_change_facts
                .iter()
                .filter(|fact| {
                    seed.from_ids.contains(&fact.source_artifact_id)
                        || seed.to_ids.contains(&fact.target_artifact_id)
                })
                .map(|fact| fact.fact_id.clone())
                .collect();
            let source_body_hashes = seed
                .from_ids
                .iter()
                .map(|id| {
                    IdBodyHashV5::new(
                        id.clone(),
                        normalized_program_body_hash(
                            source_by_id[id],
                            ProgramSideV5::Source,
                            &successors,
                        )?,
                    )
                })
                .collect::<M6Result<Vec<_>>>()?;
            let target_body_hashes = seed
                .to_ids
                .iter()
                .map(|id| {
                    IdBodyHashV5::new(
                        id.clone(),
                        normalized_program_body_hash(
                            target_by_id[id],
                            ProgramSideV5::Target,
                            &successors,
                        )?,
                    )
                })
                .collect::<M6Result<Vec<_>>>()?;
            let mapping = ProgramMappingV5::from_parts(ProgramMappingPartsV5 {
                source_closure_id: closure.id.clone(),
                source_snapshot_id: source.snapshot_id().clone(),
                target_snapshot_id: target.snapshot_id().clone(),
                object_kind: seed.object_kind,
                from_ids: seed.from_ids.clone(),
                to_ids: seed.to_ids.clone(),
                status,
                candidate_key_kind: seed.candidate_key_kind,
                source_body_hashes,
                target_body_hashes,
                change_fact_ids,
                predecessor_mapping_ids,
            })?;
            for id in &seed.from_ids {
                owner.insert(
                    (MappingSideV5::Source, id.clone()),
                    (seed.stage, mapping.id.clone()),
                );
            }
            for id in &seed.to_ids {
                owner.insert(
                    (MappingSideV5::Target, id.clone()),
                    (seed.stage, mapping.id.clone()),
                );
            }
            mappings.push(mapping);
        }
        mappings.sort_by(|left, right| left.id.cmp(&right.id));
        let source_domain = source_nodes.iter().map(|node| node.id.clone()).collect();
        let target_domain = target_nodes.iter().map(|node| node.id.clone()).collect();
        let morphism = Self::seal_derived(closure, &mappings, &source_domain, &target_domain)?;
        // Recursive logical ownership accounting for every retained working
        // collection that is simultaneously live at the seal boundary. This
        // is deliberately based on the concrete nodes/maps/sets, not wire
        // size multipliers.
        let kind_vector_bytes = [
            repositories_source.capacity(),
            repositories_target.capacity(),
            snapshots_source.capacity(),
            snapshots_target.capacity(),
            artifacts_source.capacity(),
            artifacts_target.capacity(),
            relations_source.capacity(),
            relations_target.capacity(),
            contexts_source.capacity(),
            contexts_target.capacity(),
            invariants_source.capacity(),
            invariants_target.capacity(),
            limitations_source.capacity(),
            limitations_target.capacity(),
        ]
        .into_iter()
        .fold(0_usize, usize::saturating_add)
        .saturating_mul(std::mem::size_of::<&ProgramNodeV5>());
        let observed_working_peak_upper_bound_bytes = [
            node_working,
            id_ref_map_heap(&source_by_id),
            id_ref_map_heap(&target_by_id),
            id_set_heap(&consumed_source),
            id_set_heap(&consumed_target),
            seeds.iter().map(ComponentSeedV5::allocated_bytes).sum(),
            successor_map_heap(&successors),
            owner_map_heap(&owner),
            mappings.iter().map(ProgramMappingV5::allocated_bytes).sum(),
            id_set_heap(&source_domain),
            id_set_heap(&target_domain),
            morphism.allocated_bytes(),
            id_set_heap(&copied_target_ids),
            kind_vector_bytes,
            candidate_scratch_upper_bound(source_nodes.len(), target_nodes.len()),
            MAX_M6_CANONICAL_BYTES.max(MAX_M6_MAPPING_DTO_BYTES),
        ]
        .into_iter()
        .try_fold(0_usize, checked_working_add)?;
        let working_peak_upper_bound_bytes = Self::mapping_reservation_bytes(source, target)?;
        if observed_working_peak_upper_bound_bytes > working_peak_upper_bound_bytes {
            return Err(M6Error::Incomplete {
                operation: "M6 mapping reservation underflow",
                limit: working_peak_upper_bound_bytes,
                observed: observed_working_peak_upper_bound_bytes,
            });
        }
        Ok(M6MappingPhaseV5 {
            mappings,
            morphism,
            working_peak_upper_bound_bytes,
        })
    }

    fn seal_derived(
        closure: &IncrementalSourceClosureV5,
        mappings: &[ProgramMappingV5],
        source_domain_ids: &BTreeSet<StableId>,
        target_domain_ids: &BTreeSet<StableId>,
    ) -> M6Result<Self> {
        bounded(mappings.len(), MAX_M6_MAPPINGS, "M6 program mappings")?;
        bounded(
            source_domain_ids.len(),
            MAX_M6_PROGRAM_DOMAIN_IDS,
            "M6 source domain",
        )?;
        bounded(
            target_domain_ids.len(),
            MAX_M6_PROGRAM_DOMAIN_IDS,
            "M6 target domain",
        )?;
        if mappings.is_empty() || source_domain_ids.is_empty() || target_domain_ids.is_empty() {
            return Err(M6Error::InvalidMapping(
                "mapping seal and both program domains must be nonempty",
            ));
        }
        if mappings.windows(2).any(|pair| pair[0].id >= pair[1].id) {
            return Err(M6Error::InvalidMapping(
                "mapping seal input must be strictly ID ordered",
            ));
        }
        let mut owned_source = BTreeSet::new();
        let mut owned_target = BTreeSet::new();
        let mut status_counts = MappingStatusCountsV5::default();
        for mapping in mappings {
            if mapping.source_closure_id != *closure.id()
                || mapping.source_snapshot_id != closure.input.source_snapshot_id
                || mapping.target_snapshot_id != closure.input.target_snapshot_id
            {
                return Err(M6Error::InvalidMapping("mapping closure/snapshot mismatch"));
            }
            if mapping
                .from_ids
                .iter()
                .any(|id| !owned_source.insert(id.clone()))
                || mapping
                    .to_ids
                    .iter()
                    .any(|id| !owned_target.insert(id.clone()))
            {
                return Err(M6Error::InvalidMapping(
                    "a program object is owned by more than one component",
                ));
            }
            status_counts.record(mapping.status);
        }
        if owned_source != *source_domain_ids || owned_target != *target_domain_ids {
            return Err(M6Error::InvalidMapping(
                "mapping components must exactly cover both program domains",
            ));
        }
        let mapping_set_digest = crate::canonical::compact_json_array_sha256_streaming(
            mappings.iter().map(|mapping| {
                mapping
                    .body_hash()
                    .and_then(|hash| IdBodyHashV5::new(mapping.id.clone(), hash))
                    .map_err(|error| DomainError::Validation(error.to_string()))
            }),
        )?;
        let source_domain_digest = digest_ids(source_domain_ids)?;
        let target_domain_digest = digest_ids(target_domain_ids)?;
        let source_ids = std::iter::once(closure.id.clone()).collect();
        let identity = ChangeMorphismIdentityV5 {
            source_closure_id: closure.id(),
            repository_id: &closure.input.repository_id,
            source_snapshot_id: &closure.input.source_snapshot_id,
            target_snapshot_id: &closure.input.target_snapshot_id,
            mapping_policy_descriptor_id: PROGRAM_MAPPING_POLICY_V5,
            semantic_anchor_descriptor_id: RUST_SYMBOL_ANCHOR_V1,
            mapping_count: mappings.len() as u64,
            mapping_set_digest: &mapping_set_digest,
            source_domain_count: source_domain_ids.len() as u64,
            source_domain_digest: &source_domain_digest,
            target_domain_count: target_domain_ids.len() as u64,
            target_domain_digest: &target_domain_digest,
            status_counts: &status_counts,
            source_ids: &source_ids,
        };
        let id = derive("change-morphism-v5", &identity)?;
        let value = Self {
            schema: "reviewgraphen.change_morphism.v5",
            id,
            source_closure_id: closure.id.clone(),
            repository_id: closure.input.repository_id.clone(),
            source_snapshot_id: closure.input.source_snapshot_id.clone(),
            target_snapshot_id: closure.input.target_snapshot_id.clone(),
            mapping_policy_descriptor_id: PROGRAM_MAPPING_POLICY_V5,
            semantic_anchor_descriptor_id: RUST_SYMBOL_ANCHOR_V1,
            mapping_count: mappings.len() as u64,
            mapping_set_digest,
            source_domain_count: source_domain_ids.len() as u64,
            source_domain_digest,
            target_domain_count: target_domain_ids.len() as u64,
            target_domain_digest,
            status_counts,
            source_ids,
        };
        bounded_event_dto(&value, MAX_M6_MORPHISM_DTO_BYTES, "M6 morphism DTO bytes")?;
        Ok(value)
    }

    pub(crate) fn from_json_bytes(input: &[u8], expected: &Self) -> M6Result<Self> {
        preflight_event_line(input.len(), 1)?;
        bounded(
            input.len(),
            MAX_M6_MORPHISM_DTO_BYTES,
            "M6 morphism JSON bytes",
        )?;
        let wire: ChangeMorphismWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        if wire.schema != "reviewgraphen.change_morphism.v5"
            || wire.mapping_policy_descriptor_id != PROGRAM_MAPPING_POLICY_V5
            || wire.semantic_anchor_descriptor_id != RUST_SYMBOL_ANCHOR_V1
            || wire.id != expected.id
            || wire.source_closure_id != expected.source_closure_id
            || wire.repository_id != expected.repository_id
            || wire.source_snapshot_id != expected.source_snapshot_id
            || wire.target_snapshot_id != expected.target_snapshot_id
            || wire.mapping_count != expected.mapping_count
            || wire.mapping_set_digest != expected.mapping_set_digest
            || wire.source_domain_count != expected.source_domain_count
            || wire.source_domain_digest != expected.source_domain_digest
            || wire.target_domain_count != expected.target_domain_count
            || wire.target_domain_digest != expected.target_domain_digest
            || wire.status_counts != expected.status_counts
            || wire.source_ids != expected.source_ids
            || crate::canonical_json(expected)? != input
        {
            return Err(M6Error::InvalidWire(
                "morphism wire is not exact canonical replay content".to_owned(),
            ));
        }
        Ok(expected.clone())
    }

    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    #[must_use]
    pub fn repository_id(&self) -> &StableId {
        &self.repository_id
    }
    #[must_use]
    pub fn source_closure_id(&self) -> &StableId {
        &self.source_closure_id
    }
    #[must_use]
    pub fn source_snapshot_id(&self) -> &StableId {
        &self.source_snapshot_id
    }
    #[must_use]
    pub fn target_snapshot_id(&self) -> &StableId {
        &self.target_snapshot_id
    }
    #[must_use]
    pub fn mapping_count(&self) -> u64 {
        self.mapping_count
    }
    #[must_use]
    pub fn mapping_set_digest(&self) -> &ContentHash {
        &self.mapping_set_digest
    }
    #[must_use]
    pub fn source_domain_count(&self) -> u64 {
        self.source_domain_count
    }
    #[must_use]
    pub fn source_domain_digest(&self) -> &ContentHash {
        &self.source_domain_digest
    }
    #[must_use]
    pub fn target_domain_count(&self) -> u64 {
        self.target_domain_count
    }
    #[must_use]
    pub fn target_domain_digest(&self) -> &ContentHash {
        &self.target_domain_digest
    }
    #[must_use]
    pub fn status_counts(&self) -> &MappingStatusCountsV5 {
        &self.status_counts
    }
    #[must_use]
    pub fn source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
    pub fn body_hash(&self) -> M6Result<ContentHash> {
        body_hash(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObligationCorrespondenceEntryPartsV5 {
    morphism_id: StableId,
    from_obligation_ids: BTreeSet<StableId>,
    to_obligation_ids: BTreeSet<StableId>,
    status: MappingStatusV5,
    source_mapping_ids: BTreeSet<StableId>,
    predecessor_entry_ids: BTreeSet<StableId>,
    source_body_hashes: Vec<IdBodyHashV5>,
    target_body_hashes: Vec<IdBodyHashV5>,
}

#[derive(Serialize)]
struct ObligationCorrespondenceEntryIdentityV5<'a> {
    morphism_id: &'a StableId,
    from_obligation_ids: &'a BTreeSet<StableId>,
    to_obligation_ids: &'a BTreeSet<StableId>,
    status: MappingStatusV5,
    source_mapping_ids: &'a BTreeSet<StableId>,
    predecessor_entry_ids: &'a BTreeSet<StableId>,
    source_body_hashes: &'a Vec<IdBodyHashV5>,
    target_body_hashes: &'a Vec<IdBodyHashV5>,
}

/// One deterministic, exclusive source/target obligation component.
/// Callers cannot mint accepted-looking entries from inferred JSON fields:
///
/// ```compile_fail
/// use reviewgraphen_core::ObligationCorrespondenceEntryV5;
/// let _forged = ObligationCorrespondenceEntryV5 {};
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ObligationCorrespondenceEntryV5 {
    schema: &'static str,
    id: StableId,
    morphism_id: StableId,
    from_obligation_ids: BTreeSet<StableId>,
    to_obligation_ids: BTreeSet<StableId>,
    status: MappingStatusV5,
    source_mapping_ids: BTreeSet<StableId>,
    predecessor_entry_ids: BTreeSet<StableId>,
    successor_obligation_ids: BTreeSet<StableId>,
    source_body_hashes: Vec<IdBodyHashV5>,
    target_body_hashes: Vec<IdBodyHashV5>,
    source_ids: BTreeSet<StableId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ObligationCorrespondenceEntryWireV5 {
    schema: String,
    id: StableId,
    morphism_id: StableId,
    from_obligation_ids: BTreeSet<StableId>,
    to_obligation_ids: BTreeSet<StableId>,
    status: MappingStatusV5,
    source_mapping_ids: BTreeSet<StableId>,
    predecessor_entry_ids: BTreeSet<StableId>,
    successor_obligation_ids: BTreeSet<StableId>,
    source_body_hashes: Vec<IdBodyHashV5>,
    target_body_hashes: Vec<IdBodyHashV5>,
    source_ids: BTreeSet<StableId>,
}

fn validate_obligation_component_shape(
    from: usize,
    to: usize,
    status: MappingStatusV5,
) -> M6Result<()> {
    let valid = match (from, to, status) {
        (0, 1, MappingStatusV5::Added) | (1, 0, MappingStatusV5::Removed) => true,
        (
            1,
            1,
            MappingStatusV5::Preserved | MappingStatusV5::Modified | MappingStatusV5::Unresolved,
        ) => true,
        (1, target, MappingStatusV5::Split) if target > 1 => true,
        (source, 1, MappingStatusV5::Merged) if source > 1 => true,
        (source, target, MappingStatusV5::Unresolved) if source > 1 && target > 1 => true,
        _ => false,
    };
    if !valid {
        return Err(M6Error::InvalidObligationUniverse(
            "correspondence status does not match exclusive component cardinality",
        ));
    }
    Ok(())
}

impl ObligationCorrespondenceEntryV5 {
    fn from_parts(input: ObligationCorrespondenceEntryPartsV5) -> M6Result<Self> {
        require_kind(&input.morphism_id, "change-morphism-v5", "morphism_id")?;
        bounded(
            input.from_obligation_ids.len(),
            MAX_M6_CORRESPONDENCE_SIDE_IDS,
            "M6 correspondence source obligations",
        )?;
        bounded(
            input.to_obligation_ids.len(),
            MAX_M6_CORRESPONDENCE_SIDE_IDS,
            "M6 correspondence target obligations",
        )?;
        bounded(
            input.source_mapping_ids.len(),
            MAX_M6_CORRESPONDENCE_PREDECESSOR_IDS,
            "M6 correspondence source mappings",
        )?;
        bounded(
            input.predecessor_entry_ids.len(),
            MAX_M6_CORRESPONDENCE_PREDECESSOR_IDS,
            "M6 correspondence predecessor entries",
        )?;
        if input
            .from_obligation_ids
            .iter()
            .chain(&input.to_obligation_ids)
            .any(|id| id.kind() != "obligation")
            || input
                .source_mapping_ids
                .iter()
                .any(|id| id.kind() != "program-mapping-v5")
            || input
                .predecessor_entry_ids
                .iter()
                .any(|id| id.kind() != "obligation-correspondence-entry-v5")
        {
            return Err(M6Error::InvalidObligationUniverse(
                "correspondence member/source IDs use the wrong namespace",
            ));
        }
        validate_obligation_component_shape(
            input.from_obligation_ids.len(),
            input.to_obligation_ids.len(),
            input.status,
        )?;
        validate_body_hash_records(
            "source_body_hashes",
            &input.from_obligation_ids,
            &input.source_body_hashes,
        )?;
        validate_body_hash_records(
            "target_body_hashes",
            &input.to_obligation_ids,
            &input.target_body_hashes,
        )?;
        let identity = ObligationCorrespondenceEntryIdentityV5 {
            morphism_id: &input.morphism_id,
            from_obligation_ids: &input.from_obligation_ids,
            to_obligation_ids: &input.to_obligation_ids,
            status: input.status,
            source_mapping_ids: &input.source_mapping_ids,
            predecessor_entry_ids: &input.predecessor_entry_ids,
            source_body_hashes: &input.source_body_hashes,
            target_body_hashes: &input.target_body_hashes,
        };
        let id = derive("obligation-correspondence-entry-v5", &identity)?;
        let successor_obligation_ids = if input.from_obligation_ids.is_empty() {
            BTreeSet::new()
        } else {
            input.to_obligation_ids.clone()
        };
        let source_ids = std::iter::once(input.morphism_id.clone())
            .chain(input.source_mapping_ids.iter().cloned())
            .chain(input.predecessor_entry_ids.iter().cloned())
            .chain(input.from_obligation_ids.iter().cloned())
            .chain(input.to_obligation_ids.iter().cloned())
            .collect();
        let value = Self {
            schema: "reviewgraphen.obligation_correspondence_entry.v5",
            id,
            morphism_id: input.morphism_id,
            from_obligation_ids: input.from_obligation_ids,
            to_obligation_ids: input.to_obligation_ids,
            status: input.status,
            source_mapping_ids: input.source_mapping_ids,
            predecessor_entry_ids: input.predecessor_entry_ids,
            successor_obligation_ids,
            source_body_hashes: input.source_body_hashes,
            target_body_hashes: input.target_body_hashes,
            source_ids,
        };
        bounded_event_dto(
            &value,
            MAX_M6_CORRESPONDENCE_DTO_BYTES,
            "M6 correspondence entry DTO bytes",
        )?;
        Ok(value)
    }

    pub(crate) fn from_json_bytes(input: &[u8]) -> M6Result<Self> {
        preflight_event_line(input.len(), 1)?;
        bounded(
            input.len(),
            MAX_M6_CORRESPONDENCE_DTO_BYTES,
            "M6 correspondence entry JSON bytes",
        )?;
        let wire: ObligationCorrespondenceEntryWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        if wire.schema != "reviewgraphen.obligation_correspondence_entry.v5" {
            return Err(M6Error::InvalidWire(
                "wrong obligation correspondence entry schema".to_owned(),
            ));
        }
        let expected = Self::from_parts(ObligationCorrespondenceEntryPartsV5 {
            morphism_id: wire.morphism_id,
            from_obligation_ids: wire.from_obligation_ids,
            to_obligation_ids: wire.to_obligation_ids,
            status: wire.status,
            source_mapping_ids: wire.source_mapping_ids,
            predecessor_entry_ids: wire.predecessor_entry_ids,
            source_body_hashes: wire.source_body_hashes,
            target_body_hashes: wire.target_body_hashes,
        })?;
        if expected.id != wire.id
            || expected.successor_obligation_ids != wire.successor_obligation_ids
            || expected.source_ids != wire.source_ids
            || crate::canonical_json(&expected)? != input
        {
            return Err(M6Error::InvalidWire(
                "correspondence entry wire is not exact canonical derived content".to_owned(),
            ));
        }
        Ok(expected)
    }

    fn allocated_bytes(&self) -> usize {
        let records = |values: &Vec<IdBodyHashV5>| {
            values
                .capacity()
                .saturating_mul(std::mem::size_of::<IdBodyHashV5>())
                .saturating_add(
                    values
                        .iter()
                        .map(|record| {
                            record
                                .id
                                .allocated_bytes()
                                .saturating_add(record.body_hash.allocated_bytes())
                        })
                        .sum::<usize>(),
                )
        };
        [
            std::mem::size_of::<Self>(),
            self.id.allocated_bytes(),
            self.morphism_id.allocated_bytes(),
            id_set_heap(&self.from_obligation_ids),
            id_set_heap(&self.to_obligation_ids),
            id_set_heap(&self.source_mapping_ids),
            id_set_heap(&self.predecessor_entry_ids),
            id_set_heap(&self.successor_obligation_ids),
            records(&self.source_body_hashes),
            records(&self.target_body_hashes),
            id_set_heap(&self.source_ids),
        ]
        .into_iter()
        .fold(0_usize, usize::saturating_add)
    }

    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    #[must_use]
    pub fn morphism_id(&self) -> &StableId {
        &self.morphism_id
    }
    #[must_use]
    pub fn from_obligation_ids(&self) -> &BTreeSet<StableId> {
        &self.from_obligation_ids
    }
    #[must_use]
    pub fn to_obligation_ids(&self) -> &BTreeSet<StableId> {
        &self.to_obligation_ids
    }
    #[must_use]
    pub fn status(&self) -> MappingStatusV5 {
        self.status
    }
    #[must_use]
    pub fn source_mapping_ids(&self) -> &BTreeSet<StableId> {
        &self.source_mapping_ids
    }
    #[must_use]
    pub fn predecessor_entry_ids(&self) -> &BTreeSet<StableId> {
        &self.predecessor_entry_ids
    }
    #[must_use]
    pub fn successor_obligation_ids(&self) -> &BTreeSet<StableId> {
        &self.successor_obligation_ids
    }
    #[must_use]
    pub fn source_body_hashes(&self) -> &[IdBodyHashV5] {
        &self.source_body_hashes
    }
    #[must_use]
    pub fn target_body_hashes(&self) -> &[IdBodyHashV5] {
        &self.target_body_hashes
    }
    #[must_use]
    pub fn source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
    pub fn body_hash(&self) -> M6Result<ContentHash> {
        body_hash(self)
    }
}

#[derive(Serialize)]
struct ObligationCorrespondenceIdentityV5<'a> {
    morphism_id: &'a StableId,
    source_universe_id: &'a StableId,
    target_universe_id: &'a StableId,
    policy_descriptor_id: &'static str,
    entry_count: u64,
    entry_set_digest: &'a ContentHash,
    source_domain_count: u64,
    source_domain_digest: &'a ContentHash,
    target_domain_count: u64,
    target_domain_digest: &'a ContentHash,
    status_counts: &'a MappingStatusCountsV5,
    source_ids: &'a BTreeSet<StableId>,
}

/// Seal over the complete, exclusively owned obligation correspondence phase.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ObligationCorrespondenceV5 {
    schema: &'static str,
    id: StableId,
    morphism_id: StableId,
    source_universe_id: StableId,
    target_universe_id: StableId,
    policy_descriptor_id: &'static str,
    entry_count: u64,
    entry_set_digest: ContentHash,
    source_domain_count: u64,
    source_domain_digest: ContentHash,
    target_domain_count: u64,
    target_domain_digest: ContentHash,
    status_counts: MappingStatusCountsV5,
    source_ids: BTreeSet<StableId>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ObligationCorrespondenceWireV5 {
    schema: String,
    id: StableId,
    morphism_id: StableId,
    source_universe_id: StableId,
    target_universe_id: StableId,
    policy_descriptor_id: String,
    entry_count: u64,
    entry_set_digest: ContentHash,
    source_domain_count: u64,
    source_domain_digest: ContentHash,
    target_domain_count: u64,
    target_domain_digest: ContentHash,
    status_counts: MappingStatusCountsV5,
    source_ids: BTreeSet<StableId>,
}

impl ObligationCorrespondenceV5 {
    pub(crate) fn from_projection_event_json(input: &[u8]) -> M6Result<Self> {
        Self::validate_event_wire(input)?;
        let wire: ObligationCorrespondenceWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        Ok(Self {
            schema: "reviewgraphen.obligation_correspondence.v5",
            id: wire.id,
            morphism_id: wire.morphism_id,
            source_universe_id: wire.source_universe_id,
            target_universe_id: wire.target_universe_id,
            policy_descriptor_id: OBLIGATION_CORRESPONDENCE_POLICY_V5,
            entry_count: wire.entry_count,
            entry_set_digest: wire.entry_set_digest,
            source_domain_count: wire.source_domain_count,
            source_domain_digest: wire.source_domain_digest,
            target_domain_count: wire.target_domain_count,
            target_domain_digest: wire.target_domain_digest,
            status_counts: wire.status_counts,
            source_ids: wire.source_ids,
        })
    }

    /// Compares a durable seal body with this freshly recomputed expected
    /// correspondence phase without deserializing it as authority.
    pub(crate) fn canonical_body_matches(&self, input: &[u8]) -> M6Result<bool> {
        Ok(crate::canonical_json(self)? == input)
    }

    pub(crate) fn validate_event_wire(input: &[u8]) -> M6Result<()> {
        preflight_event_line(input.len(), 1)?;
        bounded(
            input.len(),
            MAX_M6_CORRESPONDENCE_DTO_BYTES,
            "M6 correspondence seal event JSON bytes",
        )?;
        let wire: ObligationCorrespondenceWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        let input_value: serde_json::Value = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        let normalized =
            serde_json::to_value(&wire).map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        if input_value != normalized {
            return Err(M6Error::InvalidWire(
                "correspondence seal event wire contains an unknown, duplicate, or lossy field"
                    .to_owned(),
            ));
        }
        if wire.schema != "reviewgraphen.obligation_correspondence.v5"
            || wire.policy_descriptor_id != OBLIGATION_CORRESPONDENCE_POLICY_V5
        {
            return Err(M6Error::InvalidWire(
                "correspondence seal event wire has a wrong schema or fixed descriptor".to_owned(),
            ));
        }
        require_kind(&wire.morphism_id, "change-morphism-v5", "morphism_id")?;
        require_kind(&wire.source_universe_id, "universe", "source_universe_id")?;
        require_kind(&wire.target_universe_id, "universe", "target_universe_id")?;
        for (field, digest) in [
            ("entry_set_digest", &wire.entry_set_digest),
            ("source_domain_digest", &wire.source_domain_digest),
            ("target_domain_digest", &wire.target_domain_digest),
        ] {
            full_sha256(field, digest)?;
        }
        let expected_source_ids = BTreeSet::from([
            wire.morphism_id.clone(),
            wire.source_universe_id.clone(),
            wire.target_universe_id.clone(),
        ]);
        if wire.entry_count > MAX_M6_CORRESPONDENCE_ENTRIES as u64
            || wire.source_domain_count > MAX_M6_OBLIGATIONS_PER_UNIVERSE as u64
            || wire.target_domain_count > MAX_M6_OBLIGATIONS_PER_UNIVERSE as u64
            || wire.status_counts.checked_total() != Some(wire.entry_count)
            || wire.source_ids != expected_source_ids
        {
            return Err(M6Error::InvalidWire(
                "correspondence seal event wire has inconsistent counts or source IDs".to_owned(),
            ));
        }
        let expected_id = derive(
            "obligation-correspondence-v5",
            &ObligationCorrespondenceIdentityV5 {
                morphism_id: &wire.morphism_id,
                source_universe_id: &wire.source_universe_id,
                target_universe_id: &wire.target_universe_id,
                policy_descriptor_id: OBLIGATION_CORRESPONDENCE_POLICY_V5,
                entry_count: wire.entry_count,
                entry_set_digest: &wire.entry_set_digest,
                source_domain_count: wire.source_domain_count,
                source_domain_digest: &wire.source_domain_digest,
                target_domain_count: wire.target_domain_count,
                target_domain_digest: &wire.target_domain_digest,
                status_counts: &wire.status_counts,
                source_ids: &wire.source_ids,
            },
        )?;
        if wire.id != expected_id {
            return Err(M6Error::InvalidWire(
                "correspondence seal event wire derived ID does not match its complete body"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    fn seal_derived(
        morphism: &ChangeMorphismV5,
        source_universe_id: &StableId,
        target_universe_id: &StableId,
        entries: &[ObligationCorrespondenceEntryV5],
        source_domain: &BTreeSet<StableId>,
        target_domain: &BTreeSet<StableId>,
    ) -> M6Result<Self> {
        if source_universe_id.kind() != "universe" || target_universe_id.kind() != "universe" {
            return Err(M6Error::InvalidObligationUniverse(
                "correspondence seal universe IDs use the wrong namespace",
            ));
        }
        bounded(
            entries.len(),
            MAX_M6_CORRESPONDENCE_ENTRIES,
            "M6 correspondence entries",
        )?;
        bounded(
            source_domain.len(),
            MAX_M6_OBLIGATIONS_PER_UNIVERSE,
            "M6 source obligations",
        )?;
        bounded(
            target_domain.len(),
            MAX_M6_OBLIGATIONS_PER_UNIVERSE,
            "M6 target obligations",
        )?;
        if entries.windows(2).any(|pair| pair[0].id >= pair[1].id) {
            return Err(M6Error::InvalidObligationUniverse(
                "correspondence seal entries must be strictly ID ordered",
            ));
        }
        let mut owned_source = BTreeSet::new();
        let mut owned_target = BTreeSet::new();
        let mut status_counts = MappingStatusCountsV5::default();
        for entry in entries {
            if entry.morphism_id != *morphism.id()
                || entry
                    .from_obligation_ids
                    .iter()
                    .any(|id| !owned_source.insert(id.clone()))
                || entry
                    .to_obligation_ids
                    .iter()
                    .any(|id| !owned_target.insert(id.clone()))
            {
                return Err(M6Error::InvalidObligationUniverse(
                    "correspondence entry morphism/ownership mismatch",
                ));
            }
            status_counts.record(entry.status);
        }
        if owned_source != *source_domain || owned_target != *target_domain {
            return Err(M6Error::InvalidObligationUniverse(
                "correspondence entries must exactly cover both obligation domains",
            ));
        }
        let entry_set_digest =
            crate::canonical::compact_json_array_sha256_streaming(entries.iter().map(|entry| {
                entry
                    .body_hash()
                    .and_then(|hash| IdBodyHashV5::new(entry.id.clone(), hash))
                    .map_err(|error| DomainError::Validation(error.to_string()))
            }))?;
        let source_domain_digest = digest_ids(source_domain)?;
        let target_domain_digest = digest_ids(target_domain)?;
        let source_ids = BTreeSet::from([
            morphism.id().clone(),
            source_universe_id.clone(),
            target_universe_id.clone(),
        ]);
        let identity = ObligationCorrespondenceIdentityV5 {
            morphism_id: morphism.id(),
            source_universe_id,
            target_universe_id,
            policy_descriptor_id: OBLIGATION_CORRESPONDENCE_POLICY_V5,
            entry_count: entries.len() as u64,
            entry_set_digest: &entry_set_digest,
            source_domain_count: source_domain.len() as u64,
            source_domain_digest: &source_domain_digest,
            target_domain_count: target_domain.len() as u64,
            target_domain_digest: &target_domain_digest,
            status_counts: &status_counts,
            source_ids: &source_ids,
        };
        let id = derive("obligation-correspondence-v5", &identity)?;
        let value = Self {
            schema: "reviewgraphen.obligation_correspondence.v5",
            id,
            morphism_id: morphism.id().clone(),
            source_universe_id: source_universe_id.clone(),
            target_universe_id: target_universe_id.clone(),
            policy_descriptor_id: OBLIGATION_CORRESPONDENCE_POLICY_V5,
            entry_count: entries.len() as u64,
            entry_set_digest,
            source_domain_count: source_domain.len() as u64,
            source_domain_digest,
            target_domain_count: target_domain.len() as u64,
            target_domain_digest,
            status_counts,
            source_ids,
        };
        bounded_event_dto(
            &value,
            MAX_M6_CORRESPONDENCE_DTO_BYTES,
            "M6 correspondence seal DTO bytes",
        )?;
        Ok(value)
    }

    fn from_json_bytes(input: &[u8], expected: &Self) -> M6Result<Self> {
        preflight_event_line(input.len(), 1)?;
        bounded(
            input.len(),
            MAX_M6_CORRESPONDENCE_DTO_BYTES,
            "M6 correspondence seal JSON bytes",
        )?;
        let wire: ObligationCorrespondenceWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        if wire.schema != "reviewgraphen.obligation_correspondence.v5"
            || wire.policy_descriptor_id != OBLIGATION_CORRESPONDENCE_POLICY_V5
            || wire.id != expected.id
            || wire.morphism_id != expected.morphism_id
            || wire.source_universe_id != expected.source_universe_id
            || wire.target_universe_id != expected.target_universe_id
            || wire.entry_count != expected.entry_count
            || wire.entry_set_digest != expected.entry_set_digest
            || wire.source_domain_count != expected.source_domain_count
            || wire.source_domain_digest != expected.source_domain_digest
            || wire.target_domain_count != expected.target_domain_count
            || wire.target_domain_digest != expected.target_domain_digest
            || wire.status_counts != expected.status_counts
            || wire.source_ids != expected.source_ids
            || crate::canonical_json(expected)? != input
        {
            return Err(M6Error::InvalidWire(
                "correspondence seal wire is not exact canonical replay content".to_owned(),
            ));
        }
        Ok(expected.clone())
    }

    fn allocated_bytes(&self) -> usize {
        [
            std::mem::size_of::<Self>(),
            self.id.allocated_bytes(),
            self.morphism_id.allocated_bytes(),
            self.source_universe_id.allocated_bytes(),
            self.target_universe_id.allocated_bytes(),
            self.entry_set_digest.allocated_bytes(),
            self.source_domain_digest.allocated_bytes(),
            self.target_domain_digest.allocated_bytes(),
            id_set_heap(&self.source_ids),
        ]
        .into_iter()
        .fold(0_usize, usize::saturating_add)
    }

    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    #[must_use]
    pub fn morphism_id(&self) -> &StableId {
        &self.morphism_id
    }
    #[must_use]
    pub fn source_universe_id(&self) -> &StableId {
        &self.source_universe_id
    }
    #[must_use]
    pub fn target_universe_id(&self) -> &StableId {
        &self.target_universe_id
    }
    #[must_use]
    pub fn entry_count(&self) -> u64 {
        self.entry_count
    }
    #[must_use]
    pub fn entry_set_digest(&self) -> &ContentHash {
        &self.entry_set_digest
    }
    #[must_use]
    pub fn source_domain_count(&self) -> u64 {
        self.source_domain_count
    }
    #[must_use]
    pub fn source_domain_digest(&self) -> &ContentHash {
        &self.source_domain_digest
    }
    #[must_use]
    pub fn target_domain_count(&self) -> u64 {
        self.target_domain_count
    }
    #[must_use]
    pub fn target_domain_digest(&self) -> &ContentHash {
        &self.target_domain_digest
    }
    #[must_use]
    pub fn status_counts(&self) -> &MappingStatusCountsV5 {
        &self.status_counts
    }
    #[must_use]
    pub fn source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
    pub fn body_hash(&self) -> M6Result<ContentHash> {
        body_hash(self)
    }
}

/// Closed ADR 0023 historical-record order.  Declaration order is part of
/// `record_set_digest`; it is deliberately independent of append order.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalRecordKindV5 {
    Obligation,
    ReviewPlan,
    ContextEnvelope,
    Execution,
    Claim,
    ClaimAssessment,
    ArtifactRegistrationV3,
    ArtifactRegistrationV4,
    Evidence,
    EvidenceBinding,
    Verification,
    Decision,
    Finding,
    GluingInputDescriptor,
    ContextCover,
    Section,
    Restriction,
    GluingAttempt,
    GlobalCandidate,
    GluingObstruction,
    Coverage,
}

impl HistoricalRecordKindV5 {
    const fn from_internal(value: HistoricalSourceRecordKindV4) -> Self {
        match value {
            HistoricalSourceRecordKindV4::Obligation => Self::Obligation,
            HistoricalSourceRecordKindV4::ReviewPlan => Self::ReviewPlan,
            HistoricalSourceRecordKindV4::ContextEnvelope => Self::ContextEnvelope,
            HistoricalSourceRecordKindV4::Execution => Self::Execution,
            HistoricalSourceRecordKindV4::Claim => Self::Claim,
            HistoricalSourceRecordKindV4::ClaimAssessment => Self::ClaimAssessment,
            HistoricalSourceRecordKindV4::ArtifactRegistrationV3 => Self::ArtifactRegistrationV3,
            HistoricalSourceRecordKindV4::ArtifactRegistrationV4 => Self::ArtifactRegistrationV4,
            HistoricalSourceRecordKindV4::Evidence => Self::Evidence,
            HistoricalSourceRecordKindV4::EvidenceBinding => Self::EvidenceBinding,
            HistoricalSourceRecordKindV4::Verification => Self::Verification,
            HistoricalSourceRecordKindV4::Decision => Self::Decision,
            HistoricalSourceRecordKindV4::Finding => Self::Finding,
            HistoricalSourceRecordKindV4::GluingInputDescriptor => Self::GluingInputDescriptor,
            HistoricalSourceRecordKindV4::ContextCover => Self::ContextCover,
            HistoricalSourceRecordKindV4::Section => Self::Section,
            HistoricalSourceRecordKindV4::Restriction => Self::Restriction,
            HistoricalSourceRecordKindV4::GluingAttempt => Self::GluingAttempt,
            HistoricalSourceRecordKindV4::GlobalCandidate => Self::GlobalCandidate,
            HistoricalSourceRecordKindV4::GluingObstruction => Self::GluingObstruction,
            HistoricalSourceRecordKindV4::Coverage => Self::Coverage,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalAssessmentStatusV5 {
    StructurallyPreserved,
    Stale,
    Superseded,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StalenessDirectnessV5 {
    NotApplicable,
    Indirect,
    Direct,
    DirectAndIndirect,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StaleReasonV5 {
    TargetChanged,
    DependencyChanged,
    ContextChanged,
    EvidenceChanged,
    TestChanged,
    PolicyChanged,
    RuleChanged,
    ExtractorChanged,
    ModelPolicyChanged,
    HumanAuthorityNotCarried,
    MappingUnresolved,
    ObligationChanged,
    GluingInputChanged,
    UnsupportedImpactPolicy,
}

#[derive(Serialize)]
struct HistoricalRecordAssessmentIdentityV5<'a> {
    assessment_id: &'a StableId,
    source_record_kind: HistoricalRecordKindV5,
    source_record_id: &'a StableId,
    source_record_body_hash: &'a ContentHash,
    successor_record_ids: &'a BTreeSet<StableId>,
    status: HistoricalAssessmentStatusV5,
    directness: StalenessDirectnessV5,
    reasons: &'a BTreeSet<StaleReasonV5>,
    dependency_source_ids: &'a BTreeSet<StableId>,
    mapping_ids: &'a BTreeSet<StableId>,
    correspondence_entry_ids: &'a BTreeSet<StableId>,
    source_ids: &'a BTreeSet<StableId>,
}

/// Immutable assessment of one pinned V4 historical record.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HistoricalRecordAssessmentV5 {
    schema: &'static str,
    id: StableId,
    assessment_id: StableId,
    source_record_kind: HistoricalRecordKindV5,
    source_record_id: StableId,
    source_record_body_hash: ContentHash,
    successor_record_ids: BTreeSet<StableId>,
    status: HistoricalAssessmentStatusV5,
    directness: StalenessDirectnessV5,
    reasons: BTreeSet<StaleReasonV5>,
    dependency_source_ids: BTreeSet<StableId>,
    mapping_ids: BTreeSet<StableId>,
    correspondence_entry_ids: BTreeSet<StableId>,
    source_ids: BTreeSet<StableId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalRecordAssessmentWireV5 {
    schema: String,
    id: StableId,
    assessment_id: StableId,
    source_record_kind: HistoricalRecordKindV5,
    source_record_id: StableId,
    source_record_body_hash: ContentHash,
    successor_record_ids: BTreeSet<StableId>,
    status: HistoricalAssessmentStatusV5,
    directness: StalenessDirectnessV5,
    reasons: BTreeSet<StaleReasonV5>,
    dependency_source_ids: BTreeSet<StableId>,
    mapping_ids: BTreeSet<StableId>,
    correspondence_entry_ids: BTreeSet<StableId>,
    source_ids: BTreeSet<StableId>,
}

struct HistoricalRecordAssessmentPartsV5 {
    assessment_id: StableId,
    source_record_kind: HistoricalRecordKindV5,
    source_record_id: StableId,
    source_record_body_hash: ContentHash,
    successor_record_ids: BTreeSet<StableId>,
    status: HistoricalAssessmentStatusV5,
    directness: StalenessDirectnessV5,
    reasons: BTreeSet<StaleReasonV5>,
    dependency_source_ids: BTreeSet<StableId>,
    mapping_ids: BTreeSet<StableId>,
    correspondence_entry_ids: BTreeSet<StableId>,
}

impl HistoricalRecordAssessmentV5 {
    fn from_parts(parts: HistoricalRecordAssessmentPartsV5) -> M6Result<Self> {
        require_kind(
            &parts.assessment_id,
            "staleness-assessment-v5",
            "assessment_id",
        )?;
        for (operation, count) in [
            ("M6 historical successors", parts.successor_record_ids.len()),
            (
                "M6 historical dependency sources",
                parts.dependency_source_ids.len(),
            ),
            ("M6 historical mapping IDs", parts.mapping_ids.len()),
            (
                "M6 historical correspondence IDs",
                parts.correspondence_entry_ids.len(),
            ),
        ] {
            bounded(count, MAX_M6_RECORD_METADATA_IDS, operation)?;
        }
        if parts
            .mapping_ids
            .iter()
            .any(|id| id.kind() != "program-mapping-v5")
            || parts
                .correspondence_entry_ids
                .iter()
                .any(|id| id.kind() != "obligation-correspondence-entry-v5")
        {
            return Err(M6Error::InvalidStalenessAssessment(
                "historical assessment mapping/correspondence namespace mismatch",
            ));
        }
        let status_shape = match parts.status {
            HistoricalAssessmentStatusV5::StructurallyPreserved => {
                parts.directness == StalenessDirectnessV5::NotApplicable
                    && parts.reasons.is_empty()
                    && !parts.successor_record_ids.is_empty()
            }
            HistoricalAssessmentStatusV5::Stale => {
                parts.directness != StalenessDirectnessV5::NotApplicable
                    && !parts.reasons.is_empty()
            }
            HistoricalAssessmentStatusV5::Superseded => {
                parts.directness != StalenessDirectnessV5::NotApplicable
                    && !parts.reasons.is_empty()
                    && parts.successor_record_ids.is_empty()
            }
        };
        if !status_shape {
            return Err(M6Error::InvalidStalenessAssessment(
                "historical assessment status/directness/reason shape mismatch",
            ));
        }
        let source_ids = std::iter::once(parts.assessment_id.clone())
            .chain(std::iter::once(parts.source_record_id.clone()))
            .chain(parts.successor_record_ids.iter().cloned())
            .chain(parts.dependency_source_ids.iter().cloned())
            .chain(parts.mapping_ids.iter().cloned())
            .chain(parts.correspondence_entry_ids.iter().cloned())
            .collect::<BTreeSet<_>>();
        bounded(
            source_ids.len(),
            MAX_M6_RECORD_METADATA_IDS,
            "M6 historical source IDs",
        )?;
        let identity = HistoricalRecordAssessmentIdentityV5 {
            assessment_id: &parts.assessment_id,
            source_record_kind: parts.source_record_kind,
            source_record_id: &parts.source_record_id,
            source_record_body_hash: &parts.source_record_body_hash,
            successor_record_ids: &parts.successor_record_ids,
            status: parts.status,
            directness: parts.directness,
            reasons: &parts.reasons,
            dependency_source_ids: &parts.dependency_source_ids,
            mapping_ids: &parts.mapping_ids,
            correspondence_entry_ids: &parts.correspondence_entry_ids,
            source_ids: &source_ids,
        };
        let value = Self {
            schema: "reviewgraphen.historical_record_assessment.v5",
            id: derive("historical-record-assessment-v5", &identity)?,
            assessment_id: parts.assessment_id,
            source_record_kind: parts.source_record_kind,
            source_record_id: parts.source_record_id,
            source_record_body_hash: parts.source_record_body_hash,
            successor_record_ids: parts.successor_record_ids,
            status: parts.status,
            directness: parts.directness,
            reasons: parts.reasons,
            dependency_source_ids: parts.dependency_source_ids,
            mapping_ids: parts.mapping_ids,
            correspondence_entry_ids: parts.correspondence_entry_ids,
            source_ids,
        };
        bounded_event_dto(
            &value,
            MAX_M6_CANONICAL_BYTES,
            "M6 historical assessment DTO bytes",
        )?;
        Ok(value)
    }

    #[doc(hidden)]
    pub(crate) fn from_json_bytes(input: &[u8]) -> M6Result<Self> {
        bounded(
            input.len(),
            MAX_M6_CANONICAL_BYTES,
            "M6 historical assessment JSON bytes",
        )?;
        preflight_event_line(input.len(), 1)?;
        let wire: HistoricalRecordAssessmentWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        if wire.schema != "reviewgraphen.historical_record_assessment.v5" {
            return Err(M6Error::InvalidWire(
                "wrong historical assessment schema".to_owned(),
            ));
        }
        let expected = Self::from_parts(HistoricalRecordAssessmentPartsV5 {
            assessment_id: wire.assessment_id,
            source_record_kind: wire.source_record_kind,
            source_record_id: wire.source_record_id,
            source_record_body_hash: wire.source_record_body_hash,
            successor_record_ids: wire.successor_record_ids,
            status: wire.status,
            directness: wire.directness,
            reasons: wire.reasons,
            dependency_source_ids: wire.dependency_source_ids,
            mapping_ids: wire.mapping_ids,
            correspondence_entry_ids: wire.correspondence_entry_ids,
        })?;
        if expected.id != wire.id
            || expected.source_ids != wire.source_ids
            || crate::canonical_json(&expected)? != input
        {
            return Err(M6Error::InvalidWire(
                "historical assessment wire is not exact canonical derived content".to_owned(),
            ));
        }
        Ok(expected)
    }

    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    #[must_use]
    pub fn assessment_id(&self) -> &StableId {
        &self.assessment_id
    }
    #[must_use]
    pub const fn source_record_kind(&self) -> HistoricalRecordKindV5 {
        self.source_record_kind
    }
    #[must_use]
    pub fn source_record_id(&self) -> &StableId {
        &self.source_record_id
    }
    #[must_use]
    pub fn source_record_body_hash(&self) -> &ContentHash {
        &self.source_record_body_hash
    }
    #[must_use]
    pub fn successor_record_ids(&self) -> &BTreeSet<StableId> {
        &self.successor_record_ids
    }
    #[must_use]
    pub const fn status(&self) -> HistoricalAssessmentStatusV5 {
        self.status
    }
    #[must_use]
    pub const fn directness(&self) -> StalenessDirectnessV5 {
        self.directness
    }
    #[must_use]
    pub fn reasons(&self) -> &BTreeSet<StaleReasonV5> {
        &self.reasons
    }
    #[must_use]
    pub fn dependency_source_ids(&self) -> &BTreeSet<StableId> {
        &self.dependency_source_ids
    }
    #[must_use]
    pub fn mapping_ids(&self) -> &BTreeSet<StableId> {
        &self.mapping_ids
    }
    #[must_use]
    pub fn correspondence_entry_ids(&self) -> &BTreeSet<StableId> {
        &self.correspondence_entry_ids
    }
    #[must_use]
    pub fn source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
    pub fn body_hash(&self) -> M6Result<ContentHash> {
        body_hash(self)
    }

    fn allocated_bytes(&self) -> usize {
        [
            self.id.allocated_bytes(),
            self.assessment_id.allocated_bytes(),
            self.source_record_id.allocated_bytes(),
            self.source_record_body_hash.allocated_bytes(),
            id_set_heap(&self.successor_record_ids),
            id_set_heap(&self.dependency_source_ids),
            id_set_heap(&self.mapping_ids),
            id_set_heap(&self.correspondence_entry_ids),
            id_set_heap(&self.source_ids),
        ]
        .into_iter()
        .fold(0, usize::saturating_add)
    }
}

#[derive(Serialize)]
struct GluingFreshnessIdentityV5<'a> {
    assessment_id: &'a StableId,
    source_attempt_id: &'a StableId,
    status: HistoricalAssessmentStatusV5,
    reasons: &'a BTreeSet<StaleReasonV5>,
    dependency_mapping_ids: &'a BTreeSet<StableId>,
    successor_target_attempt_ids: &'a BTreeSet<StableId>,
    source_ids: &'a BTreeSet<StableId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GluingFreshnessV5 {
    schema: &'static str,
    id: StableId,
    assessment_id: StableId,
    source_attempt_id: StableId,
    status: HistoricalAssessmentStatusV5,
    reasons: BTreeSet<StaleReasonV5>,
    dependency_mapping_ids: BTreeSet<StableId>,
    successor_target_attempt_ids: BTreeSet<StableId>,
    source_ids: BTreeSet<StableId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GluingFreshnessWireV5 {
    schema: String,
    id: StableId,
    assessment_id: StableId,
    source_attempt_id: StableId,
    status: HistoricalAssessmentStatusV5,
    reasons: BTreeSet<StaleReasonV5>,
    dependency_mapping_ids: BTreeSet<StableId>,
    successor_target_attempt_ids: BTreeSet<StableId>,
    source_ids: BTreeSet<StableId>,
}

impl GluingFreshnessV5 {
    fn derive(
        assessment_id: StableId,
        source_attempt_id: StableId,
        status: HistoricalAssessmentStatusV5,
        reasons: BTreeSet<StaleReasonV5>,
        dependency_mapping_ids: BTreeSet<StableId>,
        successor_target_attempt_ids: BTreeSet<StableId>,
    ) -> M6Result<Self> {
        require_kind(&assessment_id, "staleness-assessment-v5", "assessment_id")?;
        if source_attempt_id.kind() != "gluing-attempt-v4"
            || successor_target_attempt_ids.len() > 1
            || successor_target_attempt_ids
                .iter()
                .any(|id| id.kind() != "gluing-attempt-v4")
            || dependency_mapping_ids
                .iter()
                .any(|id| id.kind() != "program-mapping-v5")
        {
            return Err(M6Error::InvalidStalenessAssessment(
                "gluing freshness ID shape mismatch",
            ));
        }
        bounded(
            dependency_mapping_ids.len(),
            MAX_M6_RECORD_METADATA_IDS,
            "M6 gluing dependency mappings",
        )?;
        let shape = match status {
            HistoricalAssessmentStatusV5::StructurallyPreserved => {
                reasons.is_empty() && successor_target_attempt_ids.len() == 1
            }
            HistoricalAssessmentStatusV5::Stale => !reasons.is_empty(),
            HistoricalAssessmentStatusV5::Superseded => {
                !reasons.is_empty() && successor_target_attempt_ids.is_empty()
            }
        };
        if !shape {
            return Err(M6Error::InvalidStalenessAssessment(
                "gluing freshness status/reason/successor shape mismatch",
            ));
        }
        let source_ids = std::iter::once(assessment_id.clone())
            .chain(std::iter::once(source_attempt_id.clone()))
            .chain(dependency_mapping_ids.iter().cloned())
            .chain(successor_target_attempt_ids.iter().cloned())
            .collect::<BTreeSet<_>>();
        let identity = GluingFreshnessIdentityV5 {
            assessment_id: &assessment_id,
            source_attempt_id: &source_attempt_id,
            status,
            reasons: &reasons,
            dependency_mapping_ids: &dependency_mapping_ids,
            successor_target_attempt_ids: &successor_target_attempt_ids,
            source_ids: &source_ids,
        };
        let value = Self {
            schema: "reviewgraphen.gluing_freshness.v5",
            id: derive("gluing-freshness-v5", &identity)?,
            assessment_id,
            source_attempt_id,
            status,
            reasons,
            dependency_mapping_ids,
            successor_target_attempt_ids,
            source_ids,
        };
        bounded_event_dto(
            &value,
            MAX_M6_CANONICAL_BYTES,
            "M6 gluing freshness DTO bytes",
        )?;
        Ok(value)
    }

    #[doc(hidden)]
    pub(crate) fn from_json_bytes(input: &[u8]) -> M6Result<Self> {
        bounded(
            input.len(),
            MAX_M6_CANONICAL_BYTES,
            "M6 gluing freshness JSON bytes",
        )?;
        preflight_event_line(input.len(), 1)?;
        let wire: GluingFreshnessWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        if wire.schema != "reviewgraphen.gluing_freshness.v5" {
            return Err(M6Error::InvalidWire(
                "wrong gluing freshness schema".to_owned(),
            ));
        }
        let expected = Self::derive(
            wire.assessment_id,
            wire.source_attempt_id,
            wire.status,
            wire.reasons,
            wire.dependency_mapping_ids,
            wire.successor_target_attempt_ids,
        )?;
        if expected.id != wire.id
            || expected.source_ids != wire.source_ids
            || crate::canonical_json(&expected)? != input
        {
            return Err(M6Error::InvalidWire(
                "gluing freshness wire is not exact canonical derived content".to_owned(),
            ));
        }
        Ok(expected)
    }

    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    #[must_use]
    pub fn assessment_id(&self) -> &StableId {
        &self.assessment_id
    }
    #[must_use]
    pub fn source_attempt_id(&self) -> &StableId {
        &self.source_attempt_id
    }
    #[must_use]
    pub const fn status(&self) -> HistoricalAssessmentStatusV5 {
        self.status
    }
    #[must_use]
    pub fn reasons(&self) -> &BTreeSet<StaleReasonV5> {
        &self.reasons
    }
    #[must_use]
    pub fn dependency_mapping_ids(&self) -> &BTreeSet<StableId> {
        &self.dependency_mapping_ids
    }
    #[must_use]
    pub fn successor_target_attempt_ids(&self) -> &BTreeSet<StableId> {
        &self.successor_target_attempt_ids
    }
    #[must_use]
    pub fn source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
    pub fn body_hash(&self) -> M6Result<ContentHash> {
        body_hash(self)
    }
}

fn validate_assessment_time_v5(value: &str) -> M6Result<()> {
    let bytes = value.as_bytes();
    let digit = |index: usize| bytes.get(index).is_some_and(u8::is_ascii_digit);
    if bytes.len() != 20
        || !(digit(0) && digit(1) && digit(2) && digit(3))
        || bytes[4] != b'-'
        || !(digit(5) && digit(6))
        || bytes[7] != b'-'
        || !(digit(8) && digit(9))
        || bytes[10] != b'T'
        || !(digit(11) && digit(12))
        || bytes[13] != b':'
        || !(digit(14) && digit(15))
        || bytes[16] != b':'
        || !(digit(17) && digit(18))
        || bytes[19] != b'Z'
    {
        return Err(M6Error::InvalidStalenessAssessment(
            "assessment_time must be exact 20-byte UTC seconds",
        ));
    }
    let number = |start: usize| -> u8 { (bytes[start] - b'0') * 10 + bytes[start + 1] - b'0' };
    let year = u16::from(bytes[0] - b'0') * 1000
        + u16::from(bytes[1] - b'0') * 100
        + u16::from(bytes[2] - b'0') * 10
        + u16::from(bytes[3] - b'0');
    let month = number(5);
    let day = number(8);
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    if day == 0 || day > max_day || number(11) > 23 || number(14) > 59 || number(17) > 59 {
        return Err(M6Error::InvalidStalenessAssessment(
            "assessment_time contains an invalid UTC calendar second",
        ));
    }
    Ok(())
}

#[derive(Serialize)]
struct StalenessAssessmentIdentityV5<'a> {
    source_closure_id: &'a StableId,
    morphism_id: &'a StableId,
    correspondence_id: &'a StableId,
    impact_policy_descriptor_id: &'static str,
    assessment_time: &'a str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StalenessAssessmentV5 {
    schema: &'static str,
    id: StableId,
    source_closure_id: StableId,
    morphism_id: StableId,
    correspondence_id: StableId,
    impact_policy_descriptor_id: &'static str,
    assessment_time: String,
    record_count: u64,
    record_set_digest: ContentHash,
    gluing_freshness_count: u64,
    gluing_freshness_set_digest: ContentHash,
    stale_source_count: u64,
    stale_source_digest: ContentHash,
    superseded_source_count: u64,
    superseded_source_digest: ContentHash,
    preservation_candidate_count: u64,
    preservation_candidate_digest: ContentHash,
    m5_dependent_successor_count: u64,
    m5_dependent_successor_digest: ContentHash,
    source_ids: BTreeSet<StableId>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StalenessAssessmentWireV5 {
    schema: String,
    id: StableId,
    source_closure_id: StableId,
    morphism_id: StableId,
    correspondence_id: StableId,
    impact_policy_descriptor_id: String,
    assessment_time: String,
    record_count: u64,
    record_set_digest: ContentHash,
    gluing_freshness_count: u64,
    gluing_freshness_set_digest: ContentHash,
    stale_source_count: u64,
    stale_source_digest: ContentHash,
    superseded_source_count: u64,
    superseded_source_digest: ContentHash,
    preservation_candidate_count: u64,
    preservation_candidate_digest: ContentHash,
    m5_dependent_successor_count: u64,
    m5_dependent_successor_digest: ContentHash,
    source_ids: BTreeSet<StableId>,
}

struct StalenessAssessmentSealPartsV5 {
    source_closure_id: StableId,
    morphism_id: StableId,
    correspondence_id: StableId,
    assessment_time: String,
    records: Vec<HistoricalRecordAssessmentV5>,
    gluing: Vec<GluingFreshnessV5>,
    preservation_candidates: BTreeSet<StableId>,
    m5_dependent_successors: BTreeSet<StableId>,
}

impl StalenessAssessmentV5 {
    pub(crate) fn from_projection_event_json(input: &[u8]) -> M6Result<Self> {
        Self::validate_event_wire(input)?;
        let wire: StalenessAssessmentWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        Ok(Self {
            schema: "reviewgraphen.staleness_assessment.v5",
            id: wire.id,
            source_closure_id: wire.source_closure_id,
            morphism_id: wire.morphism_id,
            correspondence_id: wire.correspondence_id,
            impact_policy_descriptor_id: MVP_PROPERTY_IMPACT_POLICY_V5,
            assessment_time: wire.assessment_time,
            record_count: wire.record_count,
            record_set_digest: wire.record_set_digest,
            gluing_freshness_count: wire.gluing_freshness_count,
            gluing_freshness_set_digest: wire.gluing_freshness_set_digest,
            stale_source_count: wire.stale_source_count,
            stale_source_digest: wire.stale_source_digest,
            superseded_source_count: wire.superseded_source_count,
            superseded_source_digest: wire.superseded_source_digest,
            preservation_candidate_count: wire.preservation_candidate_count,
            preservation_candidate_digest: wire.preservation_candidate_digest,
            m5_dependent_successor_count: wire.m5_dependent_successor_count,
            m5_dependent_successor_digest: wire.m5_dependent_successor_digest,
            source_ids: wire.source_ids,
        })
    }

    /// Compares the closed durable staleness seal with the roots/CAS-derived
    /// expected assessment.  Presence alone never establishes freshness.
    pub(crate) fn canonical_body_matches(&self, input: &[u8]) -> M6Result<bool> {
        Ok(crate::canonical_json(self)? == input)
    }

    pub(crate) fn validate_event_wire(input: &[u8]) -> M6Result<()> {
        preflight_event_line(input.len(), 1)?;
        bounded(
            input.len(),
            MAX_M6_CANONICAL_BYTES,
            "M6 staleness seal event JSON bytes",
        )?;
        let wire: StalenessAssessmentWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        let input_value: serde_json::Value = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        let normalized =
            serde_json::to_value(&wire).map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        if input_value != normalized {
            return Err(M6Error::InvalidWire(
                "staleness seal event wire contains an unknown, duplicate, or lossy field"
                    .to_owned(),
            ));
        }
        if wire.schema != "reviewgraphen.staleness_assessment.v5"
            || wire.impact_policy_descriptor_id != MVP_PROPERTY_IMPACT_POLICY_V5
        {
            return Err(M6Error::InvalidWire(
                "staleness seal event wire has a wrong schema or fixed descriptor".to_owned(),
            ));
        }
        require_kind(
            &wire.source_closure_id,
            "incremental-source-closure-v5",
            "source_closure_id",
        )?;
        require_kind(&wire.morphism_id, "change-morphism-v5", "morphism_id")?;
        require_kind(
            &wire.correspondence_id,
            "obligation-correspondence-v5",
            "correspondence_id",
        )?;
        validate_assessment_time_v5(&wire.assessment_time)?;
        for (field, digest) in [
            ("record_set_digest", &wire.record_set_digest),
            (
                "gluing_freshness_set_digest",
                &wire.gluing_freshness_set_digest,
            ),
            ("stale_source_digest", &wire.stale_source_digest),
            ("superseded_source_digest", &wire.superseded_source_digest),
            (
                "preservation_candidate_digest",
                &wire.preservation_candidate_digest,
            ),
            (
                "m5_dependent_successor_digest",
                &wire.m5_dependent_successor_digest,
            ),
        ] {
            full_sha256(field, digest)?;
        }
        let expected_source_ids = BTreeSet::from([
            wire.source_closure_id.clone(),
            wire.morphism_id.clone(),
            wire.correspondence_id.clone(),
        ]);
        if wire.record_count > MAX_M6_HISTORICAL_ASSESSMENTS as u64
            || wire.gluing_freshness_count != 1
            || wire.stale_source_count > wire.record_count
            || wire.superseded_source_count > wire.record_count
            || wire.preservation_candidate_count > MAX_M6_OBLIGATIONS_PER_UNIVERSE as u64
            || wire.m5_dependent_successor_count > MAX_M6_OBLIGATIONS_PER_UNIVERSE as u64
            || wire.source_ids != expected_source_ids
        {
            return Err(M6Error::InvalidWire(
                "staleness seal event wire has inconsistent counts or source IDs".to_owned(),
            ));
        }
        let expected_id = Self::assessment_id(
            &wire.source_closure_id,
            &wire.morphism_id,
            &wire.correspondence_id,
            &wire.assessment_time,
        )?;
        if wire.id != expected_id {
            return Err(M6Error::InvalidWire(
                "staleness seal event wire derived ID does not match its identity preimage"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    fn assessment_id(
        source_closure_id: &StableId,
        morphism_id: &StableId,
        correspondence_id: &StableId,
        assessment_time: &str,
    ) -> M6Result<StableId> {
        validate_assessment_time_v5(assessment_time)?;
        derive(
            "staleness-assessment-v5",
            &StalenessAssessmentIdentityV5 {
                source_closure_id,
                morphism_id,
                correspondence_id,
                impact_policy_descriptor_id: MVP_PROPERTY_IMPACT_POLICY_V5,
                assessment_time,
            },
        )
    }

    fn seal(parts: &StalenessAssessmentSealPartsV5) -> M6Result<Self> {
        bounded(
            parts.records.len(),
            MAX_M6_HISTORICAL_ASSESSMENTS,
            "M6 historical assessments",
        )?;
        bounded(
            parts.gluing.len(),
            MAX_M6_GLUE_FRESHNESS_RECORDS,
            "M6 gluing freshness",
        )?;
        if parts.gluing.len() != 1 {
            return Err(M6Error::InvalidStalenessAssessment(
                "staleness seal requires exactly one gluing freshness member",
            ));
        }
        let assessment_id = Self::assessment_id(
            &parts.source_closure_id,
            &parts.morphism_id,
            &parts.correspondence_id,
            &parts.assessment_time,
        )?;
        if parts
            .records
            .iter()
            .any(|value| value.assessment_id != assessment_id)
            || parts
                .gluing
                .iter()
                .any(|value| value.assessment_id != assessment_id)
            || parts.records.windows(2).any(|pair| {
                (pair[0].source_record_kind, &pair[0].source_record_id)
                    >= (pair[1].source_record_kind, &pair[1].source_record_id)
            })
        {
            return Err(M6Error::InvalidStalenessAssessment(
                "assessment members do not match ID or closed kind/ID order",
            ));
        }
        let record_set_digest = crate::canonical::compact_json_array_sha256_streaming(
            parts.records.iter().map(|value| {
                value
                    .body_hash()
                    .and_then(|hash| IdBodyHashV5::new(value.id.clone(), hash))
                    .map_err(|error| DomainError::Validation(error.to_string()))
            }),
        )?;
        let gluing_freshness_set_digest = crate::canonical::compact_json_array_sha256_streaming(
            parts.gluing.iter().map(|value| {
                value
                    .body_hash()
                    .and_then(|hash| IdBodyHashV5::new(value.id.clone(), hash))
                    .map_err(|error| DomainError::Validation(error.to_string()))
            }),
        )?;
        let stale_sources = parts
            .records
            .iter()
            .filter_map(|value| {
                (value.status == HistoricalAssessmentStatusV5::Stale)
                    .then_some(value.source_record_id.clone())
            })
            .collect::<BTreeSet<_>>();
        let superseded_sources = parts
            .records
            .iter()
            .filter_map(|value| {
                (value.status == HistoricalAssessmentStatusV5::Superseded)
                    .then_some(value.source_record_id.clone())
            })
            .collect::<BTreeSet<_>>();
        let source_ids = BTreeSet::from([
            parts.source_closure_id.clone(),
            parts.morphism_id.clone(),
            parts.correspondence_id.clone(),
        ]);
        let value = Self {
            schema: "reviewgraphen.staleness_assessment.v5",
            id: assessment_id,
            source_closure_id: parts.source_closure_id.clone(),
            morphism_id: parts.morphism_id.clone(),
            correspondence_id: parts.correspondence_id.clone(),
            impact_policy_descriptor_id: MVP_PROPERTY_IMPACT_POLICY_V5,
            assessment_time: parts.assessment_time.clone(),
            record_count: parts.records.len() as u64,
            record_set_digest,
            gluing_freshness_count: parts.gluing.len() as u64,
            gluing_freshness_set_digest,
            stale_source_count: stale_sources.len() as u64,
            stale_source_digest: digest_ids(&stale_sources)?,
            superseded_source_count: superseded_sources.len() as u64,
            superseded_source_digest: digest_ids(&superseded_sources)?,
            preservation_candidate_count: parts.preservation_candidates.len() as u64,
            preservation_candidate_digest: digest_ids(&parts.preservation_candidates)?,
            m5_dependent_successor_count: parts.m5_dependent_successors.len() as u64,
            m5_dependent_successor_digest: digest_ids(&parts.m5_dependent_successors)?,
            source_ids,
        };
        bounded_event_dto(
            &value,
            MAX_M6_CANONICAL_BYTES,
            "M6 staleness seal DTO bytes",
        )?;
        Ok(value)
    }

    #[doc(hidden)]
    pub(crate) fn from_json_bytes(input: &[u8], expected: &Self) -> M6Result<Self> {
        bounded(
            input.len(),
            MAX_M6_CANONICAL_BYTES,
            "M6 staleness seal JSON bytes",
        )?;
        preflight_event_line(input.len(), 1)?;
        let wire: StalenessAssessmentWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        if wire.schema != "reviewgraphen.staleness_assessment.v5"
            || wire.impact_policy_descriptor_id != MVP_PROPERTY_IMPACT_POLICY_V5
            || wire.id != expected.id
            || wire.source_closure_id != expected.source_closure_id
            || wire.morphism_id != expected.morphism_id
            || wire.correspondence_id != expected.correspondence_id
            || wire.assessment_time != expected.assessment_time
            || wire.record_count != expected.record_count
            || wire.record_set_digest != expected.record_set_digest
            || wire.gluing_freshness_count != expected.gluing_freshness_count
            || wire.gluing_freshness_set_digest != expected.gluing_freshness_set_digest
            || wire.stale_source_count != expected.stale_source_count
            || wire.stale_source_digest != expected.stale_source_digest
            || wire.superseded_source_count != expected.superseded_source_count
            || wire.superseded_source_digest != expected.superseded_source_digest
            || wire.preservation_candidate_count != expected.preservation_candidate_count
            || wire.preservation_candidate_digest != expected.preservation_candidate_digest
            || wire.m5_dependent_successor_count != expected.m5_dependent_successor_count
            || wire.m5_dependent_successor_digest != expected.m5_dependent_successor_digest
            || wire.source_ids != expected.source_ids
            || crate::canonical_json(expected)? != input
        {
            return Err(M6Error::InvalidWire(
                "staleness seal wire is not exact canonical derived content".to_owned(),
            ));
        }
        validate_assessment_time_v5(&wire.assessment_time)?;
        Ok(expected.clone())
    }

    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    #[must_use]
    pub fn source_closure_id(&self) -> &StableId {
        &self.source_closure_id
    }
    #[must_use]
    pub fn morphism_id(&self) -> &StableId {
        &self.morphism_id
    }
    #[must_use]
    pub fn correspondence_id(&self) -> &StableId {
        &self.correspondence_id
    }
    #[must_use]
    pub fn assessment_time(&self) -> &str {
        &self.assessment_time
    }
    #[must_use]
    pub const fn record_count(&self) -> u64 {
        self.record_count
    }
    #[must_use]
    pub fn record_set_digest(&self) -> &ContentHash {
        &self.record_set_digest
    }
    #[must_use]
    pub const fn gluing_freshness_count(&self) -> u64 {
        self.gluing_freshness_count
    }
    #[must_use]
    pub fn gluing_freshness_set_digest(&self) -> &ContentHash {
        &self.gluing_freshness_set_digest
    }
    #[must_use]
    pub const fn stale_source_count(&self) -> u64 {
        self.stale_source_count
    }
    #[must_use]
    pub fn stale_source_digest(&self) -> &ContentHash {
        &self.stale_source_digest
    }
    #[must_use]
    pub const fn superseded_source_count(&self) -> u64 {
        self.superseded_source_count
    }
    #[must_use]
    pub fn superseded_source_digest(&self) -> &ContentHash {
        &self.superseded_source_digest
    }
    #[must_use]
    pub const fn preservation_candidate_count(&self) -> u64 {
        self.preservation_candidate_count
    }
    #[must_use]
    pub fn preservation_candidate_digest(&self) -> &ContentHash {
        &self.preservation_candidate_digest
    }
    #[must_use]
    pub const fn m5_dependent_successor_count(&self) -> u64 {
        self.m5_dependent_successor_count
    }
    #[must_use]
    pub fn m5_dependent_successor_digest(&self) -> &ContentHash {
        &self.m5_dependent_successor_digest
    }
    #[must_use]
    pub fn source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
    pub fn body_hash(&self) -> M6Result<ContentHash> {
        body_hash(self)
    }
}

#[derive(Clone)]
struct AssessmentRecordNodeV5 {
    key: OwnedHistoricalRecordKeyV5,
    body_hash: ContentHash,
    pinned_active_or_current: bool,
    body: Value,
    direct_program_ids: BTreeSet<StableId>,
    // `required_records` is the acyclic predecessor graph used to determine
    // deterministic reduction order. `closure_records` additionally contains
    // source-object references which are real audit dependencies but form an
    // ownership back-reference (for example execution <-> raw registration)
    // or a derived M4 cycle (assessment <-> decision/finding).  They must
    // appear in dependency_source_ids and M/C reduction without making the
    // reducer's Kahn ordering cyclic.
    required_records: BTreeSet<OwnedHistoricalRecordKeyV5>,
    closure_records: BTreeSet<OwnedHistoricalRecordKeyV5>,
}

fn assessment_node_map_retained_bytes_v5(
    values: &BTreeMap<OwnedHistoricalRecordKeyV5, AssessmentRecordNodeV5>,
) -> usize {
    values.iter().fold(0_usize, |total, (key, node)| {
        total
            .saturating_add(std::mem::size_of::<(
                OwnedHistoricalRecordKeyV5,
                AssessmentRecordNodeV5,
            )>())
            .saturating_add(256)
            .saturating_add(key.id.allocated_bytes())
            .saturating_add(node.key.id.allocated_bytes())
            .saturating_add(node.body_hash.allocated_bytes())
            .saturating_add(json_retained_bytes_v5(&node.body))
            .saturating_add(id_set_heap(&node.direct_program_ids))
            .saturating_add(
                node.required_records
                    .iter()
                    .map(|key| {
                        std::mem::size_of::<OwnedHistoricalRecordKeyV5>()
                            .saturating_add(key.id.allocated_bytes())
                            .saturating_add(128)
                    })
                    .sum::<usize>(),
            )
            .saturating_add(
                node.closure_records
                    .iter()
                    .map(|key| {
                        std::mem::size_of::<OwnedHistoricalRecordKeyV5>()
                            .saturating_add(key.id.allocated_bytes())
                            .saturating_add(128)
                    })
                    .sum::<usize>(),
            )
    })
}

fn historical_typed_json_v5(value: HistoricalSourceRecordValueV4<'_>) -> M6Result<Value> {
    macro_rules! typed {
        ($value:expr) => {
            serde_json::to_value($value).map_err(|error| M6Error::Canonical(error.to_string()))
        };
    }
    match value {
        HistoricalSourceRecordValueV4::Obligation(value) => typed!(value),
        HistoricalSourceRecordValueV4::ReviewPlan(value) => typed!(value),
        HistoricalSourceRecordValueV4::ContextEnvelope(value) => typed!(value),
        HistoricalSourceRecordValueV4::Execution(value) => typed!(value),
        HistoricalSourceRecordValueV4::Claim(value) => typed!(value),
        HistoricalSourceRecordValueV4::ClaimAssessment(value) => typed!(value),
        HistoricalSourceRecordValueV4::ArtifactRegistrationV3(value) => typed!(value),
        HistoricalSourceRecordValueV4::ArtifactRegistrationV4(value) => typed!(value),
        HistoricalSourceRecordValueV4::Evidence(value) => typed!(value),
        HistoricalSourceRecordValueV4::EvidenceBinding(value) => typed!(value),
        HistoricalSourceRecordValueV4::Verification(value) => typed!(value),
        HistoricalSourceRecordValueV4::Decision(value) => typed!(value),
        HistoricalSourceRecordValueV4::Finding(value) => typed!(value),
        HistoricalSourceRecordValueV4::GluingInputDescriptor(value) => typed!(value),
        HistoricalSourceRecordValueV4::ContextCover(value) => typed!(value),
        HistoricalSourceRecordValueV4::Section(value) => typed!(value),
        HistoricalSourceRecordValueV4::Restriction(value) => typed!(value),
        HistoricalSourceRecordValueV4::GluingAttempt(value) => typed!(value),
        HistoricalSourceRecordValueV4::GlobalCandidate(value) => typed!(value),
        HistoricalSourceRecordValueV4::GluingObstruction(value) => typed!(value),
        HistoricalSourceRecordValueV4::Coverage(value) => typed!(value),
    }
}

fn historical_typed_canonical_len_v5(value: HistoricalSourceRecordValueV4<'_>) -> M6Result<usize> {
    macro_rules! count {
        ($value:expr) => {
            usize::try_from(crate::canonical::canonical_json_count_bounded(
                $value,
                MAX_M6_CANONICAL_BYTES,
                "M6 historical typed body bytes",
            )?)
            .map_err(|_| M6Error::Incomplete {
                operation: "M6 historical typed body bytes",
                limit: MAX_M6_CANONICAL_BYTES,
                observed: usize::MAX,
            })
        };
    }
    match value {
        HistoricalSourceRecordValueV4::Obligation(value) => count!(value),
        HistoricalSourceRecordValueV4::ReviewPlan(value) => count!(value),
        HistoricalSourceRecordValueV4::ContextEnvelope(value) => count!(value),
        HistoricalSourceRecordValueV4::Execution(value) => count!(value),
        HistoricalSourceRecordValueV4::Claim(value) => count!(value),
        HistoricalSourceRecordValueV4::ClaimAssessment(value) => count!(value),
        HistoricalSourceRecordValueV4::ArtifactRegistrationV3(value) => count!(value),
        HistoricalSourceRecordValueV4::ArtifactRegistrationV4(value) => count!(value),
        HistoricalSourceRecordValueV4::Evidence(value) => count!(value),
        HistoricalSourceRecordValueV4::EvidenceBinding(value) => count!(value),
        HistoricalSourceRecordValueV4::Verification(value) => count!(value),
        HistoricalSourceRecordValueV4::Decision(value) => count!(value),
        HistoricalSourceRecordValueV4::Finding(value) => count!(value),
        HistoricalSourceRecordValueV4::GluingInputDescriptor(value) => count!(value),
        HistoricalSourceRecordValueV4::ContextCover(value) => count!(value),
        HistoricalSourceRecordValueV4::Section(value) => count!(value),
        HistoricalSourceRecordValueV4::Restriction(value) => count!(value),
        HistoricalSourceRecordValueV4::GluingAttempt(value) => count!(value),
        HistoricalSourceRecordValueV4::GlobalCandidate(value) => count!(value),
        HistoricalSourceRecordValueV4::GluingObstruction(value) => count!(value),
        HistoricalSourceRecordValueV4::Coverage(value) => count!(value),
    }
}

fn collect_assessment_node_v5(
    descriptor: &HistoricalRecordDescriptorV5<'_>,
    inventory: &impl HistoricalInventoryViewV5,
) -> M6Result<AssessmentRecordNodeV5> {
    let mut direct_program_ids = BTreeSet::new();
    let mut required_records = BTreeSet::new();
    let mut closure_records = BTreeSet::new();
    let mut refusal = None;
    descriptor.visit_direct_program_ids(inventory, |id| {
        if direct_program_ids.len() == MAX_M6_RECORD_METADATA_IDS
            && !direct_program_ids.contains(id)
        {
            refusal = Some(M6Error::Incomplete {
                operation: "M6 direct Program dependencies",
                limit: MAX_M6_RECORD_METADATA_IDS,
                observed: MAX_M6_RECORD_METADATA_IDS + 1,
            });
        } else if refusal.is_none() {
            direct_program_ids.insert(id.clone());
        }
    });
    descriptor.visit_required_records(inventory, |kind, id| {
        let key = OwnedHistoricalRecordKeyV5 {
            kind,
            id: id.clone(),
        };
        if required_records.len() == MAX_M6_RECORD_METADATA_IDS && !required_records.contains(&key)
        {
            refusal = Some(M6Error::Incomplete {
                operation: "M6 direct required records",
                limit: MAX_M6_RECORD_METADATA_IDS,
                observed: MAX_M6_RECORD_METADATA_IDS + 1,
            });
        } else if refusal.is_none() {
            required_records.insert(key);
        }
    });
    descriptor.visit_closure_records(inventory, |kind, id| {
        let key = OwnedHistoricalRecordKeyV5 {
            kind,
            id: id.clone(),
        };
        if closure_records.len() == MAX_M6_RECORD_METADATA_IDS && !closure_records.contains(&key) {
            refusal = Some(M6Error::Incomplete {
                operation: "M6 transitive closure records",
                limit: MAX_M6_RECORD_METADATA_IDS,
                observed: MAX_M6_RECORD_METADATA_IDS + 1,
            });
        } else if refusal.is_none() {
            closure_records.insert(key);
        }
    });
    if let Some(error) = refusal {
        return Err(error);
    }
    Ok(AssessmentRecordNodeV5 {
        key: OwnedHistoricalRecordKeyV5 {
            kind: descriptor.key().kind,
            id: descriptor.key().id.clone(),
        },
        body_hash: descriptor.body_hash().clone(),
        pinned_active_or_current: descriptor.pinned_active_or_current(),
        body: historical_typed_json_v5(descriptor.typed_body())?,
        direct_program_ids,
        required_records,
        closure_records,
    })
}

#[derive(Clone, Copy)]
enum ImpactDirectionV5 {
    In,
    Out,
    Both,
}

#[derive(Clone, Copy)]
struct ImpactEdgeV5 {
    kind: &'static str,
    direction: ImpactDirectionV5,
    depth: usize,
}

fn impact_edges_v5(rule: &str, property: &str) -> Option<&'static [ImpactEdgeV5]> {
    const NODE: &[ImpactEdgeV5] = &[
        ImpactEdgeV5 {
            kind: "handled_by",
            direction: ImpactDirectionV5::Out,
            depth: 2,
        },
        ImpactEdgeV5 {
            kind: "awaits",
            direction: ImpactDirectionV5::Out,
            depth: 2,
        },
        ImpactEdgeV5 {
            kind: "writes",
            direction: ImpactDirectionV5::Out,
            depth: 2,
        },
    ];
    const RELATION_REENTRY: &[ImpactEdgeV5] = &[
        ImpactEdgeV5 {
            kind: "handled_by",
            direction: ImpactDirectionV5::Both,
            depth: 2,
        },
        ImpactEdgeV5 {
            kind: "awaits",
            direction: ImpactDirectionV5::Out,
            depth: 2,
        },
        ImpactEdgeV5 {
            kind: "writes",
            direction: ImpactDirectionV5::Out,
            depth: 2,
        },
    ];
    const CALL: &[ImpactEdgeV5] = &[
        ImpactEdgeV5 {
            kind: "calls",
            direction: ImpactDirectionV5::Both,
            depth: 2,
        },
        ImpactEdgeV5 {
            kind: "covers",
            direction: ImpactDirectionV5::In,
            depth: 2,
        },
        ImpactEdgeV5 {
            kind: "reads",
            direction: ImpactDirectionV5::Out,
            depth: 2,
        },
    ];
    const PATH: &[ImpactEdgeV5] = &[
        ImpactEdgeV5 {
            kind: "handled_by",
            direction: ImpactDirectionV5::Both,
            depth: 4,
        },
        ImpactEdgeV5 {
            kind: "calls",
            direction: ImpactDirectionV5::Both,
            depth: 4,
        },
        ImpactEdgeV5 {
            kind: "covers",
            direction: ImpactDirectionV5::In,
            depth: 4,
        },
    ];
    const INVARIANT: &[ImpactEdgeV5] = &[
        ImpactEdgeV5 {
            kind: "handled_by",
            direction: ImpactDirectionV5::Both,
            depth: 4,
        },
        ImpactEdgeV5 {
            kind: "calls",
            direction: ImpactDirectionV5::Both,
            depth: 4,
        },
        ImpactEdgeV5 {
            kind: "covers",
            direction: ImpactDirectionV5::In,
            depth: 4,
        },
        ImpactEdgeV5 {
            kind: "constrains",
            direction: ImpactDirectionV5::Both,
            depth: 4,
        },
    ];
    const NONE: &[ImpactEdgeV5] = &[];
    match (rule, property) {
        ("node.changed_public_symbol@1", "async.concurrent_reentry") => Some(NODE),
        ("relation.concurrent_reentry@1", "async.concurrent_reentry") => Some(RELATION_REENTRY),
        ("relation.changed_call_contract@1", "payment.idempotency_contract") => Some(CALL),
        ("path.external_side_effect@1", "payment.at_most_once") => Some(PATH),
        ("invariant.payment_at_most_once@1", "payment.at_most_once") => Some(INVARIANT),
        ("capability_gap.origin_rule@1", "reviewgraphen.capability_gap") => Some(NONE),
        _ => None,
    }
}

#[derive(Clone)]
struct ObligationImpactRowV5 {
    direct: BTreeSet<StableId>,
    indirect: BTreeSet<StableId>,
    supported: bool,
}

fn exact_target_candidates_for_row_v5(
    key: &OwnedHistoricalRecordKeyV5,
    obligation_id: Option<&StableId>,
    correspondence: &M6ObligationCorrespondencePhaseV5,
    target_nodes: &BTreeMap<OwnedHistoricalRecordKeyV5, AssessmentRecordNodeV5>,
) -> BTreeSet<OwnedHistoricalRecordKeyV5> {
    if key.kind == HistoricalSourceRecordKindV4::Obligation {
        return correspondence
            .entries()
            .iter()
            .filter(|entry| entry.from_obligation_ids().contains(&key.id))
            .flat_map(|entry| entry.to_obligation_ids().iter())
            .filter_map(|id| {
                let target = OwnedHistoricalRecordKeyV5 {
                    kind: key.kind,
                    id: id.clone(),
                };
                target_nodes.contains_key(&target).then_some(target)
            })
            .collect();
    }
    if key.kind == HistoricalSourceRecordKindV4::ReviewPlan {
        return target_nodes
            .keys()
            .filter(|candidate| candidate.kind == HistoricalSourceRecordKindV4::ReviewPlan)
            .cloned()
            .collect();
    }
    if let Some(obligation_id) = obligation_id {
        let reaches = |candidate: &OwnedHistoricalRecordKeyV5, target_obligation: &StableId| {
            let wanted = OwnedHistoricalRecordKeyV5 {
                kind: HistoricalSourceRecordKindV4::Obligation,
                id: target_obligation.clone(),
            };
            let mut frontier = target_nodes
                .get(candidate)
                .map(|node| node.required_records.clone())
                .unwrap_or_default();
            let mut visited = BTreeSet::new();
            while let Some(current) = frontier.pop_first() {
                if current == wanted {
                    return true;
                }
                if visited.insert(current.clone())
                    && let Some(node) = target_nodes.get(&current)
                {
                    frontier.extend(node.required_records.iter().cloned());
                }
            }
            false
        };
        return correspondence
            .entries()
            .iter()
            .filter(|entry| entry.from_obligation_ids().contains(obligation_id))
            .flat_map(|entry| entry.to_obligation_ids().iter())
            .flat_map(|target_obligation| {
                target_nodes
                    .iter()
                    .filter(move |(candidate, _)| {
                        candidate.kind == key.kind
                            && (reaches(candidate, target_obligation)
                                || candidate.id == *target_obligation)
                    })
                    .map(|(candidate, _)| candidate.clone())
            })
            .collect();
    }
    // Records outside an obligation closure (for example run-genesis or
    // snapshot-ingest registrations) still have actual target successors.
    // They are only candidates here; the closed per-kind structural
    // predicate below must prove exact equality after permitted ID mapping.
    target_nodes
        .keys()
        .filter(|candidate| candidate.kind == key.kind)
        .cloned()
        .collect()
}

fn row_successors_v5(
    key: &OwnedHistoricalRecordKeyV5,
    node: &AssessmentRecordNodeV5,
    obligation_id: Option<&StableId>,
    correspondence: &M6ObligationCorrespondencePhaseV5,
    target_nodes: &BTreeMap<OwnedHistoricalRecordKeyV5, AssessmentRecordNodeV5>,
    replacements: &BTreeMap<String, String>,
    successor_map: &BTreeMap<OwnedHistoricalRecordKeyV5, BTreeSet<StableId>>,
) -> M6Result<BTreeSet<StableId>> {
    let candidates =
        exact_target_candidates_for_row_v5(key, obligation_id, correspondence, target_nodes);
    let mut result = BTreeSet::new();
    for candidate_key in candidates {
        let candidate =
            target_nodes
                .get(&candidate_key)
                .ok_or(M6Error::InvalidHistoricalTopology(
                    "target successor candidate disappeared",
                ))?;
        let (local_replacements, removals) = row_replacements_v5(
            replacements,
            key,
            &candidate_key,
            &node.required_records,
            successor_map,
        );
        let exact = exact_substituted_successor_v5(
            key.kind,
            &node.body,
            &candidate.body,
            &local_replacements,
            &removals,
        );
        if exact {
            if result.len() == MAX_M6_RECORD_METADATA_IDS {
                return Err(M6Error::Incomplete {
                    operation: "M6 row successor record IDs",
                    limit: MAX_M6_RECORD_METADATA_IDS,
                    observed: MAX_M6_RECORD_METADATA_IDS + 1,
                });
            }
            result.insert(candidate_key.id);
        }
    }
    Ok(result)
}

fn row_replacements_v5(
    base: &BTreeMap<String, String>,
    key: &OwnedHistoricalRecordKeyV5,
    candidate: &OwnedHistoricalRecordKeyV5,
    required_records: &BTreeSet<OwnedHistoricalRecordKeyV5>,
    successor_map: &BTreeMap<OwnedHistoricalRecordKeyV5, BTreeSet<StableId>>,
) -> (BTreeMap<String, String>, BTreeSet<String>) {
    let mut replacements = base.clone();
    replacements.insert(key.id.as_str().to_owned(), candidate.id.as_str().to_owned());
    let removals = BTreeSet::new();
    for dependency in required_records {
        match successor_map.get(dependency) {
            Some(successors) if successors.len() == 1 => {
                replacements.insert(
                    dependency.id.as_str().to_owned(),
                    successors
                        .first()
                        .expect("one successor")
                        .as_str()
                        .to_owned(),
                );
            }
            // A removed or stale predecessor never disappears from an
            // otherwise preserved record.  Semantic removal is reduced by
            // the obligation/correspondence row, not by editing arbitrary
            // record lists until they happen to compare equal.
            Some(successors) if successors.is_empty() => {}
            _ => {}
        }
    }
    (replacements, removals)
}

fn exact_substituted_successor_v5(
    kind: HistoricalSourceRecordKindV4,
    source: &Value,
    target: &Value,
    replacements: &BTreeMap<String, String>,
    removals: &BTreeSet<String>,
) -> bool {
    let mut substituted = source.clone();
    // Every arm below is a closed, DTO-specific root-path projection.  This
    // must not recurse by field name: `id`, `artifact_id`, or `source_ids`
    // inside tool arguments, model output, policy details, or a future nested
    // extension is not an ADR 0023 structural reference.
    let normalized = match kind {
        HistoricalSourceRecordKindV4::Obligation => normalize_successor_root_fields_v5(
            kind,
            &mut substituted,
            replacements,
            removals,
            &[
                "id",
                "context_ids",
                "generator_ids",
                "normalized_context_ids",
                "normalized_source_ids",
                "normalized_target_refs",
                "qualification_ids",
                "source_ids",
                "target_refs",
                "depends_on",
                "normalized_depends_on",
            ],
        ),
        HistoricalSourceRecordKindV4::ReviewPlan => normalize_successor_root_fields_v5(
            kind,
            &mut substituted,
            replacements,
            removals,
            &[
                "id",
                "snapshot_id",
                "universe_id",
                "obligation_ids",
                "source_ids",
                "wave_id",
            ],
        ),
        HistoricalSourceRecordKindV4::ContextEnvelope => normalize_successor_root_fields_v5(
            kind,
            &mut substituted,
            replacements,
            removals,
            &[
                "id",
                "snapshot_id",
                "obligation_ids",
                "candidate_source_ids",
                "normalized_included_source_ids",
                "registration_id",
                "artifact_id",
                "source_ids",
            ],
        ),
        HistoricalSourceRecordKindV4::Execution => normalize_successor_root_fields_v5(
            kind,
            &mut substituted,
            replacements,
            removals,
            &[
                "id",
                "run_id",
                "snapshot_id",
                "plan_id",
                "envelope_id",
                "wave_id",
                "obligation_ids",
                "raw_artifact_registration_id",
                "parsed_claim_ids",
                "source_ids",
            ],
        ),
        HistoricalSourceRecordKindV4::Claim => normalize_successor_root_fields_v5(
            kind,
            &mut substituted,
            replacements,
            removals,
            &[
                "id",
                "execution_id",
                "obligation_ids",
                "target_refs",
                "source_ids",
            ],
        ),
        HistoricalSourceRecordKindV4::ClaimAssessment => normalize_successor_root_fields_v5(
            kind,
            &mut substituted,
            replacements,
            removals,
            &[
                "claim_id",
                "active_decision_id",
                "current_finding_id",
                "last_finding_id",
                "binding_ids",
                "evidence_ids",
                "verification_ids",
                "decision_ids",
                "finding_ids",
                "source_ids",
            ],
        ),
        HistoricalSourceRecordKindV4::ArtifactRegistrationV3 => normalize_successor_root_fields_v5(
            kind,
            &mut substituted,
            replacements,
            removals,
            &[
                "id",
                "registration_id",
                "run_id",
                "repository_id",
                "snapshot_id",
                "universe_id",
                "plan_id",
                "context_id",
                "descriptor_id",
                "execution_id",
                "artifact_id",
                "claim_id",
                "verification_id",
                "witness_registration_id",
                "source_ids",
            ],
        ),
        HistoricalSourceRecordKindV4::ArtifactRegistrationV4 => normalize_successor_root_fields_v5(
            kind,
            &mut substituted,
            replacements,
            removals,
            &[
                "id",
                "registration_id",
                "run_id",
                "repository_id",
                "snapshot_id",
                "universe_id",
                "plan_id",
                "context_id",
                "descriptor_id",
                "execution_id",
                "artifact_id",
                "claim_id",
                "verification_id",
                "witness_registration_id",
                "source_ids",
            ],
        ),
        HistoricalSourceRecordKindV4::Evidence => normalize_successor_root_fields_v5(
            kind,
            &mut substituted,
            replacements,
            removals,
            &[
                "id",
                "run_id",
                "claim_id",
                "artifact_id",
                "snapshot_id",
                "input_registration_id",
                "output_registration_id",
                "source_ids",
                "subject_ids",
            ],
        ),
        HistoricalSourceRecordKindV4::EvidenceBinding => normalize_successor_root_fields_v5(
            kind,
            &mut substituted,
            replacements,
            removals,
            &["id", "run_id", "claim_id", "evidence_id", "source_ids"],
        ),
        HistoricalSourceRecordKindV4::Verification => normalize_successor_root_fields_v5(
            kind,
            &mut substituted,
            replacements,
            removals,
            &[
                "id",
                "run_id",
                "claim_id",
                "evidence_ids",
                "input_registration_id",
                "output_registration_id",
                "source_ids",
                "artifact_id",
            ],
        ),
        HistoricalSourceRecordKindV4::Decision => normalize_successor_root_fields_v5(
            kind,
            &mut substituted,
            replacements,
            removals,
            &[
                "id",
                "run_id",
                "claim_id",
                "snapshot_id",
                "universe_id",
                "evidence_ids",
                "verification_ids",
                "source_ids",
            ],
        ),
        HistoricalSourceRecordKindV4::Finding => normalize_successor_root_fields_v5(
            kind,
            &mut substituted,
            replacements,
            removals,
            &[
                "id",
                "run_id",
                "claim_id",
                "decision_id",
                "evidence_ids",
                "verification_ids",
                "supersedes_finding_id",
                "source_ids",
            ],
        ),
        HistoricalSourceRecordKindV4::GluingInputDescriptor => normalize_successor_root_fields_v5(
            kind,
            &mut substituted,
            replacements,
            removals,
            &[
                "id",
                "run_id",
                "snapshot_id",
                "universe_id",
                "plan_id",
                "context_id",
                "qualification_source_ids",
            ],
        ),
        HistoricalSourceRecordKindV4::ContextCover => normalize_successor_root_fields_v5(
            kind,
            &mut substituted,
            replacements,
            removals,
            &[
                "id",
                "run_id",
                "snapshot_id",
                "universe_id",
                "plan_id",
                "selected_obligation_ids",
                "required_context_ids",
                "cover_domain_ids",
                "covered_domain_ids",
                "uncovered_domain_ids",
                "source_ids",
            ],
        ),
        HistoricalSourceRecordKindV4::Section => normalize_successor_root_fields_v5(
            kind,
            &mut substituted,
            replacements,
            removals,
            &[
                "id",
                "cover_id",
                "input_descriptor_id",
                "input_registration_id",
                "context_id",
                "snapshot_id",
                "invariant_id",
                "obligation_id",
                "claim_id",
                "claim_assessment_id",
                "binding_ids",
                "evidence_ids",
                "verification_ids",
                "decision_ids",
                "finding_ids",
                "qualification_source_ids",
                "source_ids",
            ],
        ),
        HistoricalSourceRecordKindV4::Restriction => normalize_successor_root_fields_v5(
            kind,
            &mut substituted,
            replacements,
            removals,
            &[
                "id",
                "section_id",
                "context_pair",
                "overlap_member_ids",
                "claim_ids",
                "qualification_source_ids",
                "evidence_ids",
                "verification_ids",
                "decision_ids",
                "finding_ids",
                "source_ids",
            ],
        ),
        HistoricalSourceRecordKindV4::GluingAttempt => normalize_successor_root_fields_v5(
            kind,
            &mut substituted,
            replacements,
            removals,
            &[
                "id",
                "cover_id",
                "snapshot_id",
                "invariant_id",
                "input_descriptor_ids",
                "section_ids",
                "restriction_ids",
                "claim_ids",
                "global_candidate_id",
                "obstruction_id",
                "evidence_ids",
                "verification_ids",
                "decision_ids",
                "finding_ids",
                "source_ids",
            ],
        ),
        HistoricalSourceRecordKindV4::GlobalCandidate => normalize_successor_root_fields_v5(
            kind,
            &mut substituted,
            replacements,
            removals,
            &[
                "id",
                "attempt_id",
                "cover_id",
                "claim_ids",
                "invariant_id",
                "required_section_ids",
                "restriction_ids",
                "qualification_source_ids",
                "evidence_ids",
                "verification_ids",
                "decision_ids",
                "finding_ids",
                "source_ids",
            ],
        ),
        HistoricalSourceRecordKindV4::GluingObstruction => normalize_successor_root_fields_v5(
            kind,
            &mut substituted,
            replacements,
            removals,
            &[
                "id",
                "attempt_id",
                "conflicting_context_ids",
                "section_ids",
                "overlap_member_ids",
                "claim_ids",
                "evidence_ids",
                "verification_ids",
                "decision_ids",
                "finding_ids",
                "affected_invariant_id",
                "blocks",
                "source_ids",
            ],
        ),
        HistoricalSourceRecordKindV4::Coverage => return false,
    };
    if !normalized {
        return false;
    }
    if !normalize_successor_nested_paths_v5(kind, &mut substituted, replacements, removals) {
        return false;
    }
    if !normalize_successor_object_array_containers_v5(kind, &mut substituted) {
        return false;
    }
    match kind {
        HistoricalSourceRecordKindV4::Obligation => substituted == *target,
        HistoricalSourceRecordKindV4::ReviewPlan => substituted == *target,
        HistoricalSourceRecordKindV4::ContextEnvelope => substituted == *target,
        HistoricalSourceRecordKindV4::Execution => substituted == *target,
        HistoricalSourceRecordKindV4::Claim => substituted == *target,
        HistoricalSourceRecordKindV4::ClaimAssessment => substituted == *target,
        HistoricalSourceRecordKindV4::ArtifactRegistrationV3 => substituted == *target,
        HistoricalSourceRecordKindV4::ArtifactRegistrationV4 => substituted == *target,
        HistoricalSourceRecordKindV4::Evidence => substituted == *target,
        HistoricalSourceRecordKindV4::EvidenceBinding => substituted == *target,
        HistoricalSourceRecordKindV4::Verification => substituted == *target,
        HistoricalSourceRecordKindV4::Decision => substituted == *target,
        HistoricalSourceRecordKindV4::Finding => substituted == *target,
        HistoricalSourceRecordKindV4::GluingInputDescriptor => substituted == *target,
        HistoricalSourceRecordKindV4::ContextCover => substituted == *target,
        HistoricalSourceRecordKindV4::Section => substituted == *target,
        HistoricalSourceRecordKindV4::Restriction => substituted == *target,
        HistoricalSourceRecordKindV4::GluingAttempt => substituted == *target,
        HistoricalSourceRecordKindV4::GlobalCandidate => substituted == *target,
        HistoricalSourceRecordKindV4::GluingObstruction => substituted == *target,
        HistoricalSourceRecordKindV4::Coverage => unreachable!(),
    }
}

/// Rewrites only declared, top-level DTO reference leaves. The actual DTO is
/// serialized into `Value` solely because the historical inventory carries an
/// enum of twenty concrete borrowed types; the enum dispatch above fixes the
/// DTO and each root path before this helper runs. It intentionally never
/// descends into an object or array element.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SuccessorReferenceArraySemanticsV5 {
    Set,
    Ordered,
}

fn successor_root_array_semantics_v5(
    kind: HistoricalSourceRecordKindV4,
    field: &str,
) -> Option<SuccessorReferenceArraySemanticsV5> {
    use HistoricalSourceRecordKindV4 as Kind;
    use SuccessorReferenceArraySemanticsV5::{Ordered, Set};
    match (kind, field) {
        (Kind::Obligation, "target_refs" | "context_ids" | "depends_on" | "source_ids") => {
            Some(Ordered)
        }
        (
            Kind::Obligation,
            "normalized_target_refs"
            | "normalized_context_ids"
            | "normalized_depends_on"
            | "normalized_source_ids"
            | "generator_ids"
            | "qualification_ids",
        ) => Some(Set),
        (Kind::ReviewPlan, "obligation_ids" | "source_ids")
        | (
            Kind::ContextEnvelope,
            "obligation_ids"
            | "candidate_source_ids"
            | "normalized_included_source_ids"
            | "source_ids",
        )
        | (Kind::Execution, "obligation_ids" | "parsed_claim_ids" | "source_ids")
        | (Kind::Claim, "obligation_ids" | "target_refs" | "source_ids")
        | (
            Kind::ClaimAssessment,
            "binding_ids" | "evidence_ids" | "verification_ids" | "decision_ids" | "finding_ids"
            | "source_ids",
        )
        | (Kind::ArtifactRegistrationV3 | Kind::ArtifactRegistrationV4, "source_ids")
        | (Kind::Evidence, "source_ids" | "subject_ids")
        | (Kind::EvidenceBinding, "source_ids")
        | (Kind::Verification, "evidence_ids" | "source_ids")
        | (Kind::Decision, "evidence_ids" | "verification_ids" | "source_ids")
        | (Kind::Finding, "evidence_ids" | "verification_ids" | "source_ids")
        | (Kind::GluingInputDescriptor, "qualification_source_ids")
        | (
            Kind::ContextCover,
            "selected_obligation_ids"
            | "cover_domain_ids"
            | "covered_domain_ids"
            | "uncovered_domain_ids"
            | "source_ids",
        )
        | (
            Kind::Section,
            "binding_ids"
            | "evidence_ids"
            | "verification_ids"
            | "decision_ids"
            | "finding_ids"
            | "qualification_source_ids"
            | "source_ids",
        )
        | (
            Kind::Restriction,
            "overlap_member_ids"
            | "claim_ids"
            | "qualification_source_ids"
            | "evidence_ids"
            | "verification_ids"
            | "decision_ids"
            | "finding_ids"
            | "source_ids",
        )
        | (
            Kind::GluingAttempt,
            "claim_ids" | "evidence_ids" | "verification_ids" | "decision_ids" | "finding_ids"
            | "source_ids",
        )
        | (
            Kind::GlobalCandidate,
            "claim_ids"
            | "qualification_source_ids"
            | "evidence_ids"
            | "verification_ids"
            | "decision_ids"
            | "finding_ids"
            | "source_ids",
        )
        | (
            Kind::GluingObstruction,
            "overlap_member_ids" | "claim_ids" | "evidence_ids" | "verification_ids"
            | "decision_ids" | "finding_ids" | "blocks" | "source_ids",
        ) => Some(Set),
        (Kind::ContextCover, "required_context_ids")
        | (Kind::Restriction, "context_pair")
        | (Kind::GluingAttempt, "input_descriptor_ids" | "section_ids" | "restriction_ids")
        | (Kind::GlobalCandidate, "required_section_ids" | "restriction_ids")
        | (Kind::GluingObstruction, "conflicting_context_ids" | "section_ids") => Some(Ordered),
        _ => None,
    }
}

fn normalize_successor_root_fields_v5(
    kind: HistoricalSourceRecordKindV4,
    value: &mut Value,
    replacements: &BTreeMap<String, String>,
    removals: &BTreeSet<String>,
    fields: &[&str],
) -> bool {
    let Some(object) = value.as_object_mut() else {
        return false;
    };
    fields.iter().all(|field| {
        let Some(reference) = object.get_mut(*field) else {
            return true;
        };
        match reference {
            Value::Null => true,
            Value::String(id) => {
                if let Some(replacement) = replacements.get(id) {
                    *id = replacement.clone();
                }
                true
            }
            Value::Array(ids) => {
                if ids.iter().any(|id| !id.is_string()) {
                    return false;
                }
                ids.retain(|id| !id.as_str().is_some_and(|id| removals.contains(id)));
                for id in ids.iter_mut() {
                    let Value::String(id) = id else {
                        return false;
                    };
                    if let Some(replacement) = replacements.get(id) {
                        *id = replacement.clone();
                    }
                }
                let Some(semantics) = successor_root_array_semantics_v5(kind, field) else {
                    return false;
                };
                if semantics == SuccessorReferenceArraySemanticsV5::Set {
                    ids.sort_by(|left, right| {
                        left.as_str()
                            .expect("string array checked above")
                            .cmp(right.as_str().expect("string array checked above"))
                    });
                }
                true
            }
            Value::Bool(_) | Value::Number(_) | Value::Object(_) => false,
        }
    })
}

/// Closed nested paths for the few canonical DTOs whose semantic references
/// are intentionally nested. `*` is legal only at a known collection field.
/// It is never a recursive field-name search, so extensions and tool/model
/// payloads stay byte-equal.
fn normalize_successor_nested_paths_v5(
    kind: HistoricalSourceRecordKindV4,
    value: &mut Value,
    replacements: &BTreeMap<String, String>,
    removals: &BTreeSet<String>,
) -> bool {
    use SuccessorReferenceArraySemanticsV5::Set;
    type NestedPath = (
        &'static [&'static str],
        Option<SuccessorReferenceArraySemanticsV5>,
    );
    const NONE: &[NestedPath] = &[];
    const REVIEW_PLAN: &[NestedPath] = &[
        (&["waves", "*", "obligation_ids"], Some(Set)),
        (&["risk_breakdown", "*", "id"], None),
        (&["deferred", "*", "id"], None),
    ];
    const CONTEXT_ENVELOPE: &[NestedPath] = &[
        (&["included_sources", "*", "registration_id"], None),
        (&["included_sources", "*", "artifact_id"], None),
        (&["excluded_sources", "*", "artifact_id"], None),
        (&["unknowns", "*", "source_ids"], Some(Set)),
        (&["losses", "*", "source_ids"], Some(Set)),
    ];
    const OBLIGATION: &[NestedPath] = &[
        // `ObligationVersion::snapshot` is a semantic snapshot reference, but
        // it is nested beneath the closed version object rather than stored at
        // the obligation root.
        (&["version", "snapshot"], None),
    ];
    const REGISTRATION: &[NestedPath] = &[
        (&["source", "run_id"], None),
        (&["source", "snapshot_id"], None),
        (&["source", "execution_id"], None),
        (&["source", "claim_id"], None),
        (&["source", "repository_id"], None),
        (&["source", "test_artifact_id"], None),
        (&["source", "universe_id"], None),
        (&["source", "context_id"], None),
        (&["source", "descriptor_id"], None),
        (&["source", "plan_id"], None),
    ];
    let paths = match kind {
        HistoricalSourceRecordKindV4::ReviewPlan => REVIEW_PLAN,
        HistoricalSourceRecordKindV4::ContextEnvelope => CONTEXT_ENVELOPE,
        HistoricalSourceRecordKindV4::ArtifactRegistrationV3
        | HistoricalSourceRecordKindV4::ArtifactRegistrationV4 => REGISTRATION,
        HistoricalSourceRecordKindV4::Obligation => OBLIGATION,
        HistoricalSourceRecordKindV4::Execution
        | HistoricalSourceRecordKindV4::Claim
        | HistoricalSourceRecordKindV4::ClaimAssessment
        | HistoricalSourceRecordKindV4::Evidence
        | HistoricalSourceRecordKindV4::EvidenceBinding
        | HistoricalSourceRecordKindV4::Verification
        | HistoricalSourceRecordKindV4::Decision
        | HistoricalSourceRecordKindV4::Finding
        | HistoricalSourceRecordKindV4::GluingInputDescriptor
        | HistoricalSourceRecordKindV4::ContextCover
        | HistoricalSourceRecordKindV4::Section
        | HistoricalSourceRecordKindV4::Restriction
        | HistoricalSourceRecordKindV4::GluingAttempt
        | HistoricalSourceRecordKindV4::GlobalCandidate
        | HistoricalSourceRecordKindV4::GluingObstruction
        | HistoricalSourceRecordKindV4::Coverage => NONE,
    };
    paths.iter().all(|(path, semantics)| {
        normalize_successor_path_v5(value, path, *semantics, replacements, removals)
    })
}

/// Re-establishes canonical ordering only for closed object arrays whose
/// container order is defined by a substituted ID key. Other object arrays
/// (`waves`, `unknowns`, `losses`, and any future extension) retain their
/// semantic order byte-for-byte.
fn normalize_successor_object_array_containers_v5(
    kind: HistoricalSourceRecordKindV4,
    value: &mut Value,
) -> bool {
    use HistoricalSourceRecordKindV4 as Kind;
    let specifications: &[(&str, &str)] = match kind {
        Kind::ReviewPlan => &[("risk_breakdown", "id"), ("deferred", "id")],
        Kind::ContextEnvelope => &[
            ("included_sources", "artifact_id"),
            ("excluded_sources", "artifact_id"),
        ],
        Kind::Obligation
        | Kind::Execution
        | Kind::Claim
        | Kind::ClaimAssessment
        | Kind::ArtifactRegistrationV3
        | Kind::ArtifactRegistrationV4
        | Kind::Evidence
        | Kind::EvidenceBinding
        | Kind::Verification
        | Kind::Decision
        | Kind::Finding
        | Kind::GluingInputDescriptor
        | Kind::ContextCover
        | Kind::Section
        | Kind::Restriction
        | Kind::GluingAttempt
        | Kind::GlobalCandidate
        | Kind::GluingObstruction
        | Kind::Coverage => &[],
    };
    let Some(object) = value.as_object_mut() else {
        return false;
    };
    specifications.iter().all(|(field, key)| {
        let Some(container) = object.get_mut(*field) else {
            return true;
        };
        let Some(values) = container.as_array_mut() else {
            return false;
        };
        if values.iter().any(|member| {
            member
                .as_object()
                .and_then(|member| member.get(*key))
                .and_then(Value::as_str)
                .is_none()
        }) {
            return false;
        }
        values.sort_by(|left, right| {
            left[*key]
                .as_str()
                .expect("object-array sort key checked above")
                .cmp(
                    right[*key]
                        .as_str()
                        .expect("object-array sort key checked above"),
                )
        });
        true
    })
}

fn normalize_successor_path_v5(
    value: &mut Value,
    path: &[&str],
    semantics: Option<SuccessorReferenceArraySemanticsV5>,
    replacements: &BTreeMap<String, String>,
    removals: &BTreeSet<String>,
) -> bool {
    let Some((segment, rest)) = path.split_first() else {
        return normalize_successor_reference_leaf_v5(value, semantics, replacements, removals);
    };
    match *segment {
        "*" => match value {
            Value::Array(values) => values.iter_mut().all(|value| {
                normalize_successor_path_v5(value, rest, semantics, replacements, removals)
            }),
            _ => false,
        },
        field => match value {
            Value::Object(values) => values.get_mut(field).is_none_or(|value| {
                normalize_successor_path_v5(value, rest, semantics, replacements, removals)
            }),
            _ => false,
        },
    }
}

fn normalize_successor_reference_leaf_v5(
    value: &mut Value,
    semantics: Option<SuccessorReferenceArraySemanticsV5>,
    replacements: &BTreeMap<String, String>,
    removals: &BTreeSet<String>,
) -> bool {
    match value {
        Value::Null => true,
        Value::String(id) => {
            if let Some(replacement) = replacements.get(id) {
                *id = replacement.clone();
            }
            true
        }
        Value::Array(ids) => {
            let Some(semantics) = semantics else {
                return false;
            };
            if ids.iter().any(|id| !id.is_string()) {
                return false;
            }
            ids.retain(|id| !id.as_str().is_some_and(|id| removals.contains(id)));
            for id in ids.iter_mut() {
                let Value::String(id) = id else {
                    return false;
                };
                if let Some(replacement) = replacements.get(id) {
                    *id = replacement.clone();
                }
            }
            if semantics == SuccessorReferenceArraySemanticsV5::Set {
                ids.sort_by(|left, right| {
                    left.as_str()
                        .expect("string array checked above")
                        .cmp(right.as_str().expect("string array checked above"))
                });
            }
            true
        }
        Value::Bool(_) | Value::Number(_) | Value::Object(_) => false,
    }
}

fn obligation_impact_row_v5(
    obligation: &Obligation,
    program: &ProgramSpace,
) -> M6Result<ObligationImpactRowV5> {
    let mut direct = obligation
        .normalized_target_refs()
        .iter()
        .chain(obligation.normalized_source_ids())
        .chain(obligation.qualification_ids())
        .chain(obligation.normalized_context_ids())
        .chain(obligation.generator_ids())
        .cloned()
        .collect::<BTreeSet<_>>();
    // Relation/path direct classes include their exact ordered endpoints.
    let initial = direct.clone();
    for relation in program.relations() {
        if initial.contains(&relation.id) {
            direct.insert(relation.source_id.clone());
            direct.extend(relation.target_ids.iter().cloned());
        }
    }
    let Some(edges) = impact_edges_v5(obligation.version().rule(), obligation.property_id()) else {
        return Ok(ObligationImpactRowV5 {
            direct,
            indirect: BTreeSet::new(),
            supported: false,
        });
    };
    let mut indirect = BTreeSet::new();
    let mut frontier = direct
        .iter()
        .cloned()
        .map(|id| (0_usize, id))
        .collect::<BTreeSet<_>>();
    let mut visited_relations = BTreeSet::new();
    while let Some((depth, current)) = frontier.pop_first() {
        for relation in program.relations() {
            let Some(policy) = edges.iter().find(|edge| edge.kind == relation.kind) else {
                continue;
            };
            if depth >= policy.depth || visited_relations.contains(&relation.id) {
                continue;
            }
            let from_source = relation.source_id == current;
            let from_target = relation.target_ids.contains(&current);
            let traversable = match policy.direction {
                ImpactDirectionV5::Out => from_source,
                ImpactDirectionV5::In => from_target,
                ImpactDirectionV5::Both => from_source || from_target,
            };
            if !traversable {
                continue;
            }
            if visited_relations.len() == MAX_M6_RELATION_VISITS {
                return Err(M6Error::Incomplete {
                    operation: "M6 impact relation visits",
                    limit: MAX_M6_RELATION_VISITS,
                    observed: MAX_M6_RELATION_VISITS + 1,
                });
            }
            visited_relations.insert(relation.id.clone());
            indirect.insert(relation.id.clone());
            let mut next = BTreeSet::new();
            match policy.direction {
                ImpactDirectionV5::Out => next.extend(relation.target_ids.iter().cloned()),
                ImpactDirectionV5::In => {
                    next.insert(relation.source_id.clone());
                }
                ImpactDirectionV5::Both => {
                    next.insert(relation.source_id.clone());
                    next.extend(relation.target_ids.iter().cloned());
                }
            }
            for id in next {
                indirect.insert(id.clone());
                frontier.insert((depth + 1, id));
            }
        }
    }
    Ok(ObligationImpactRowV5 {
        direct,
        indirect,
        supported: true,
    })
}

fn json_retained_bytes_v5(value: &Value) -> usize {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => std::mem::size_of::<Value>(),
        Value::String(text) => std::mem::size_of::<Value>().saturating_add(text.capacity()),
        Value::Array(values) => std::mem::size_of::<Value>()
            .saturating_add(
                values
                    .capacity()
                    .saturating_mul(std::mem::size_of::<Value>()),
            )
            .saturating_add(values.iter().map(json_retained_bytes_v5).sum::<usize>()),
        Value::Object(values) => std::mem::size_of::<Value>().saturating_add(
            values
                .iter()
                .map(|(key, value)| {
                    std::mem::size_of::<(String, Value)>()
                        .saturating_add(key.capacity())
                        .saturating_add(json_retained_bytes_v5(value))
                        .saturating_add(128)
                })
                .sum::<usize>(),
        ),
    }
}

fn correspondence_direct_reason_v5(
    entry: &ObligationCorrespondenceEntryV5,
    source_nodes: &BTreeMap<OwnedHistoricalRecordKeyV5, AssessmentRecordNodeV5>,
    target_nodes: &BTreeMap<OwnedHistoricalRecordKeyV5, AssessmentRecordNodeV5>,
) -> StaleReasonV5 {
    if entry.status() != MappingStatusV5::Modified
        || entry.from_obligation_ids().len() != 1
        || entry.to_obligation_ids().len() != 1
    {
        return StaleReasonV5::ObligationChanged;
    }
    let source_key = OwnedHistoricalRecordKeyV5 {
        kind: HistoricalSourceRecordKindV4::Obligation,
        id: entry
            .from_obligation_ids()
            .first()
            .expect("one source obligation")
            .clone(),
    };
    let target_key = OwnedHistoricalRecordKeyV5 {
        kind: HistoricalSourceRecordKindV4::Obligation,
        id: entry
            .to_obligation_ids()
            .first()
            .expect("one target obligation")
            .clone(),
    };
    let field = |node: Option<&AssessmentRecordNodeV5>, path: &[&str]| -> Option<String> {
        let mut value = &node?.body;
        for key in path {
            value = value.get(*key)?;
        }
        value.as_str().map(str::to_owned)
    };
    let source = source_nodes.get(&source_key);
    let target = target_nodes.get(&target_key);
    if field(source, &["version", "rule"]) != field(target, &["version", "rule"])
        || field(source, &["property_version"]) != field(target, &["property_version"])
    {
        StaleReasonV5::RuleChanged
    } else if field(source, &["version", "profile"]) != field(target, &["version", "profile"]) {
        StaleReasonV5::PolicyChanged
    } else if field(source, &["version", "extractor_set"])
        != field(target, &["version", "extractor_set"])
    {
        StaleReasonV5::ExtractorChanged
    } else {
        StaleReasonV5::ObligationChanged
    }
}

fn record_class_reason_v5(kind: HistoricalSourceRecordKindV4) -> StaleReasonV5 {
    match kind {
        HistoricalSourceRecordKindV4::ReviewPlan => StaleReasonV5::PolicyChanged,
        HistoricalSourceRecordKindV4::ContextEnvelope => StaleReasonV5::ContextChanged,
        HistoricalSourceRecordKindV4::ArtifactRegistrationV3
        | HistoricalSourceRecordKindV4::ArtifactRegistrationV4
        | HistoricalSourceRecordKindV4::Evidence
        | HistoricalSourceRecordKindV4::EvidenceBinding
        | HistoricalSourceRecordKindV4::Verification => StaleReasonV5::EvidenceChanged,
        HistoricalSourceRecordKindV4::Execution => StaleReasonV5::ModelPolicyChanged,
        HistoricalSourceRecordKindV4::Obligation => StaleReasonV5::ObligationChanged,
        HistoricalSourceRecordKindV4::GluingInputDescriptor
        | HistoricalSourceRecordKindV4::ContextCover
        | HistoricalSourceRecordKindV4::Section
        | HistoricalSourceRecordKindV4::Restriction
        | HistoricalSourceRecordKindV4::GluingAttempt
        | HistoricalSourceRecordKindV4::GlobalCandidate
        | HistoricalSourceRecordKindV4::GluingObstruction => StaleReasonV5::GluingInputChanged,
        _ => StaleReasonV5::TargetChanged,
    }
}

fn mapping_direct_reason_v5(
    mapping: &ProgramMappingV5,
    source_program: &ProgramSpace,
) -> StaleReasonV5 {
    match mapping.object_kind() {
        ProgramObjectKindV5::Context => StaleReasonV5::ContextChanged,
        ProgramObjectKindV5::Artifact
            if mapping.from_ids().iter().any(|id| {
                source_program
                    .artifacts()
                    .iter()
                    .any(|artifact| &artifact.id == id && artifact.kind == "test")
            }) =>
        {
            StaleReasonV5::TestChanged
        }
        ProgramObjectKindV5::Repository
        | ProgramObjectKindV5::Snapshot
        | ProgramObjectKindV5::Artifact
        | ProgramObjectKindV5::Relation
        | ProgramObjectKindV5::Invariant
        | ProgramObjectKindV5::Limitation => StaleReasonV5::TargetChanged,
    }
}

fn directness_v5(direct: bool, indirect: bool) -> StalenessDirectnessV5 {
    match (direct, indirect) {
        (true, true) => StalenessDirectnessV5::DirectAndIndirect,
        (true, false) => StalenessDirectnessV5::Direct,
        (false, true) => StalenessDirectnessV5::Indirect,
        (false, false) => StalenessDirectnessV5::NotApplicable,
    }
}

fn reduce_mapping_position_v5(
    status: MappingStatusV5,
    direct_position: bool,
    indirect_position: bool,
    direct_reason: StaleReasonV5,
) -> (BTreeSet<StaleReasonV5>, StalenessDirectnessV5) {
    let mut reasons = BTreeSet::new();
    if status == MappingStatusV5::Preserved {
        return (reasons, StalenessDirectnessV5::NotApplicable);
    }
    if direct_position {
        reasons.insert(match status {
            MappingStatusV5::Modified => direct_reason,
            MappingStatusV5::Added | MappingStatusV5::Removed => StaleReasonV5::TargetChanged,
            MappingStatusV5::Split | MappingStatusV5::Merged | MappingStatusV5::Unresolved => {
                StaleReasonV5::MappingUnresolved
            }
            MappingStatusV5::Preserved => unreachable!(),
        });
    }
    if indirect_position {
        reasons.insert(match status {
            MappingStatusV5::Modified | MappingStatusV5::Added | MappingStatusV5::Removed => {
                StaleReasonV5::DependencyChanged
            }
            MappingStatusV5::Split | MappingStatusV5::Merged | MappingStatusV5::Unresolved => {
                StaleReasonV5::MappingUnresolved
            }
            MappingStatusV5::Preserved => unreachable!(),
        });
    }
    (reasons, directness_v5(direct_position, indirect_position))
}

fn transitive_record_closure_v5(
    key: &OwnedHistoricalRecordKeyV5,
    nodes: &BTreeMap<OwnedHistoricalRecordKeyV5, AssessmentRecordNodeV5>,
) -> M6Result<BTreeSet<OwnedHistoricalRecordKeyV5>> {
    let mut result = BTreeSet::new();
    let mut frontier = nodes
        .get(key)
        .ok_or(M6Error::InvalidHistoricalTopology(
            "missing source record node",
        ))?
        .closure_records
        .clone();
    while let Some(current) = frontier.pop_first() {
        if current == *key || result.contains(&current) {
            continue;
        }
        if result.len() == MAX_M6_RECORD_METADATA_IDS {
            return Err(M6Error::Incomplete {
                operation: "M6 transitive dependency_source_ids",
                limit: MAX_M6_RECORD_METADATA_IDS,
                observed: MAX_M6_RECORD_METADATA_IDS + 1,
            });
        }
        let node = nodes
            .get(&current)
            .ok_or(M6Error::InvalidHistoricalTopology(
                "transitive source dependency is absent",
            ))?;
        result.insert(current);
        frontier.extend(node.closure_records.iter().cloned());
    }
    Ok(result)
}

fn active_human_obligation_ids_v5(
    node: &AssessmentRecordNodeV5,
    nodes: &BTreeMap<OwnedHistoricalRecordKeyV5, AssessmentRecordNodeV5>,
) -> M6Result<BTreeSet<StableId>> {
    let claim_keys = node
        .required_records
        .iter()
        .filter(|key| key.kind == HistoricalSourceRecordKindV4::Claim)
        .collect::<Vec<_>>();
    if claim_keys.len() != 1 {
        return Err(M6Error::InvalidHistoricalTopology(
            "active decision/finding must reference exactly one claim",
        ));
    }
    let claim = nodes
        .get(claim_keys[0])
        .ok_or(M6Error::InvalidHistoricalTopology(
            "active decision/finding claim is absent",
        ))?;
    let obligation_ids = claim
        .required_records
        .iter()
        .filter(|key| key.kind == HistoricalSourceRecordKindV4::Obligation)
        .map(|key| key.id.clone())
        .collect::<BTreeSet<_>>();
    if obligation_ids.is_empty() {
        return Err(M6Error::InvalidHistoricalTopology(
            "active decision/finding claim has no obligation",
        ));
    }
    Ok(obligation_ids)
}

#[derive(Clone, Debug, Default)]
struct PartialRerunActionSeedV5 {
    require_native_pipeline: bool,
    require_human_resolution: bool,
    stale_source_record_ids: BTreeSet<StableId>,
    reasons: BTreeSet<StaleReasonV5>,
}

fn existing_target_prerequisite_v5(
    witness: &TargetSuppressionRecordWitnessV5,
) -> ActionPrerequisiteV5 {
    ActionPrerequisiteV5::ExistingTargetRecord {
        record_id: witness.record_id().clone(),
        body_hash: witness.body_hash().clone(),
        event_id: witness.event_id().clone(),
    }
}

fn historical_kind_requires_native_rerun_v5(kind: HistoricalRecordKindV5) -> bool {
    !matches!(
        kind,
        HistoricalRecordKindV5::GluingInputDescriptor
            | HistoricalRecordKindV5::ContextCover
            | HistoricalRecordKindV5::Section
            | HistoricalRecordKindV5::Restriction
            | HistoricalRecordKindV5::GluingAttempt
            | HistoricalRecordKindV5::GlobalCandidate
            | HistoricalRecordKindV5::GluingObstruction
            | HistoricalRecordKindV5::Coverage
    )
}

#[allow(clippy::too_many_arguments)]
fn partial_rerun_row_v5(
    source_kind: HistoricalSourceRecordKindV4,
    stale_source_record_ids: &BTreeSet<StableId>,
    obligation_id: &StableId,
    row: &ObligationImpactRowV5,
    mapping_ids: &BTreeSet<StableId>,
    row_successor_empty: bool,
    dependency_changed: bool,
    active_human: bool,
    mappings: &M6MappingPhaseV5,
    correspondence: &M6ObligationCorrespondencePhaseV5,
    source_program: &ProgramSpace,
    source_nodes: &BTreeMap<OwnedHistoricalRecordKeyV5, AssessmentRecordNodeV5>,
    target_nodes: &BTreeMap<OwnedHistoricalRecordKeyV5, AssessmentRecordNodeV5>,
) -> M6Result<Vec<(StableId, PartialRerunActionSeedV5)>> {
    let entries = correspondence
        .entries()
        .iter()
        .filter(|entry| entry.from_obligation_ids().contains(obligation_id))
        .collect::<Vec<_>>();
    let targets = entries
        .iter()
        .flat_map(|entry| entry.to_obligation_ids().iter().cloned())
        .collect::<BTreeSet<_>>();
    if targets.is_empty() {
        return Ok(Vec::new());
    }
    let mut reasons = BTreeSet::new();
    if !row.supported {
        reasons.insert(StaleReasonV5::UnsupportedImpactPolicy);
    }
    if entries.is_empty() {
        reasons.insert(StaleReasonV5::ObligationChanged);
    }
    for entry in entries {
        match entry.status() {
            MappingStatusV5::Preserved => {}
            MappingStatusV5::Modified => {
                reasons.insert(correspondence_direct_reason_v5(
                    entry,
                    source_nodes,
                    target_nodes,
                ));
            }
            MappingStatusV5::Added | MappingStatusV5::Removed => {
                reasons.insert(StaleReasonV5::TargetChanged);
            }
            MappingStatusV5::Split | MappingStatusV5::Merged | MappingStatusV5::Unresolved => {
                reasons.insert(StaleReasonV5::MappingUnresolved);
            }
        }
    }
    for mapping in mappings
        .mappings()
        .iter()
        .filter(|mapping| mapping_ids.contains(mapping.id()))
    {
        let direct_position = mapping.from_ids().iter().any(|id| row.direct.contains(id));
        let indirect_position = mapping
            .from_ids()
            .iter()
            .any(|id| row.indirect.contains(id));
        let mapping_reason = mapping_direct_reason_v5(mapping, source_program);
        let direct_reason = if mapping_reason == StaleReasonV5::TargetChanged {
            record_class_reason_v5(source_kind)
        } else {
            mapping_reason
        };
        reasons.extend(
            reduce_mapping_position_v5(
                mapping.status(),
                direct_position,
                indirect_position,
                direct_reason,
            )
            .0,
        );
    }
    if dependency_changed {
        reasons.insert(StaleReasonV5::DependencyChanged);
    }
    if row_successor_empty {
        reasons.insert(StaleReasonV5::TargetChanged);
    }
    if active_human {
        reasons.insert(StaleReasonV5::HumanAuthorityNotCarried);
    }
    if reasons.is_empty() {
        return Ok(Vec::new());
    }
    let kind = HistoricalRecordKindV5::from_internal(source_kind);
    if !historical_kind_requires_native_rerun_v5(kind) {
        return Ok(Vec::new());
    }
    Ok(targets
        .into_iter()
        .map(|target| {
            (
                target,
                PartialRerunActionSeedV5 {
                    require_native_pipeline: true,
                    // An unsupported impact row cannot prove that the source
                    // human authority applies to this successor obligation.
                    // Keep the conservative native rerun, but never launder
                    // that source decision into a target human-action demand.
                    require_human_resolution: active_human && row.supported,
                    stale_source_record_ids: stale_source_record_ids.clone(),
                    reasons: reasons.clone(),
                },
            )
        })
        .collect())
}

fn add_correspondence_only_partial_seeds_v5(
    correspondence: &M6ObligationCorrespondencePhaseV5,
    seeds: &mut BTreeMap<StableId, PartialRerunActionSeedV5>,
) {
    for entry in correspondence.entries().iter().filter(|entry| {
        entry.from_obligation_ids().is_empty()
            && matches!(
                entry.status(),
                MappingStatusV5::Added | MappingStatusV5::Unresolved
            )
    }) {
        let reason = if entry.status() == MappingStatusV5::Added {
            StaleReasonV5::TargetChanged
        } else {
            StaleReasonV5::MappingUnresolved
        };
        for target in entry.to_obligation_ids() {
            let seed = seeds.entry(target.clone()).or_default();
            seed.require_native_pipeline = true;
            seed.reasons.insert(reason);
        }
    }
}

#[derive(Clone, Debug)]
pub struct M6StalenessPhaseV5 {
    records: Vec<HistoricalRecordAssessmentV5>,
    gluing_freshness: Vec<GluingFreshnessV5>,
    assessment: StalenessAssessmentV5,
    preservation_candidate_obligation_ids: BTreeSet<StableId>,
    target_gluing_required: bool,
    m5_dependent_successor_obligation_ids: BTreeSet<StableId>,
    partial_rerun_seeds: BTreeMap<StableId, PartialRerunActionSeedV5>,
    target_plan_id: StableId,
    target_snapshot_id: StableId,
    target_run_id: StableId,
    target_genesis_hash: ContentHash,
    target_tail_hash: ContentHash,
    target_event_count: u64,
    target_policy_revision_hash: ContentHash,
    target_pre_incremental_basis_digest: ContentHash,
    working_bytes: usize,
}

impl M6StalenessPhaseV5 {
    #[must_use]
    pub fn records(&self) -> &[HistoricalRecordAssessmentV5] {
        &self.records
    }
    #[must_use]
    pub fn gluing_freshness(&self) -> &[GluingFreshnessV5] {
        &self.gluing_freshness
    }
    #[must_use]
    pub fn assessment(&self) -> &StalenessAssessmentV5 {
        &self.assessment
    }
    #[must_use]
    pub fn preservation_candidate_obligation_ids(&self) -> &BTreeSet<StableId> {
        &self.preservation_candidate_obligation_ids
    }

    /// Exact target prefix pinned by this reduction.  The V5 persistence
    /// session must replay this prefix from its caller-supplied journal,
    /// never substitute a fixture's retained terminal log.
    pub(crate) const fn target_predecessor_event_count(&self) -> u64 {
        self.target_event_count
    }

    pub(crate) fn target_predecessor_tail_hash(&self) -> &ContentHash {
        &self.target_tail_hash
    }

    /// Deterministically reduces the sealed staleness/preservation facts into
    /// the initial no-suppression partial-rerun DAG.  It performs no journal,
    /// CAS, lifecycle, verifier, human, or gluing mutation.
    #[cfg(test)]
    pub(crate) fn plan_partial_rerun_without_target_suppression_v5(
        &self,
        preservation: &M6PreservationPhaseV5,
    ) -> M6Result<(Vec<PartialRerunActionV5>, PartialRerunPlanV5)> {
        self.plan_partial_rerun_with_limits_v5(
            preservation,
            None,
            MAX_M6_PARTIAL_RERUN_ACTIONS,
            MAX_M6_PARTIAL_RERUN_WORKING_BYTES,
        )
    }

    /// Produces a test-only planning projection of an already sealed
    /// staleness result.  The assessment, historical records, gluing facts,
    /// target coordinates, and their canonical seal are retained verbatim;
    /// this merely narrows the otherwise-unsealed action-seed reduction to
    /// one actual target obligation.  It exists so Event can exercise a
    /// single normal M4 action without hand-constructing a partial plan.
    #[cfg(test)]
    pub(crate) fn with_only_partial_rerun_subject_for_test(
        &self,
        target_obligation_id: &StableId,
    ) -> M6Result<Self> {
        let seed = self
            .partial_rerun_seeds
            .get(target_obligation_id)
            .filter(|seed| seed.require_native_pipeline)
            .cloned()
            .ok_or(M6Error::InvalidStalenessAssessment(
                "test planning projection target has no sealed native rerun seed",
            ))?;
        Ok(Self {
            records: self.records.clone(),
            gluing_freshness: self.gluing_freshness.clone(),
            assessment: self.assessment.clone(),
            preservation_candidate_obligation_ids: self
                .preservation_candidate_obligation_ids
                .clone(),
            target_gluing_required: self.target_gluing_required,
            m5_dependent_successor_obligation_ids: self
                .m5_dependent_successor_obligation_ids
                .clone(),
            partial_rerun_seeds: BTreeMap::from([(target_obligation_id.clone(), seed)]),
            target_plan_id: self.target_plan_id.clone(),
            target_snapshot_id: self.target_snapshot_id.clone(),
            target_run_id: self.target_run_id.clone(),
            target_genesis_hash: self.target_genesis_hash.clone(),
            target_tail_hash: self.target_tail_hash.clone(),
            target_event_count: self.target_event_count,
            target_policy_revision_hash: self.target_policy_revision_hash.clone(),
            target_pre_incremental_basis_digest: self.target_pre_incremental_basis_digest.clone(),
            working_bytes: self.working_bytes,
        })
    }

    pub(crate) fn plan_partial_rerun_with_target_suppression_v5(
        &self,
        preservation: &M6PreservationPhaseV5,
        suppression: &TargetPredecessorSuppressionProjectionV5,
    ) -> M6Result<(Vec<PartialRerunActionV5>, PartialRerunPlanV5)> {
        self.plan_partial_rerun_with_limits_v5(
            preservation,
            Some(suppression),
            MAX_M6_PARTIAL_RERUN_ACTIONS,
            MAX_M6_PARTIAL_RERUN_WORKING_BYTES,
        )
    }

    pub(crate) fn plan_partial_rerun_with_target_suppression_and_limit_v5(
        &self,
        preservation: &M6PreservationPhaseV5,
        suppression: &TargetPredecessorSuppressionProjectionV5,
        working_limit: usize,
    ) -> M6Result<(Vec<PartialRerunActionV5>, PartialRerunPlanV5)> {
        self.plan_partial_rerun_with_limits_v5(
            preservation,
            Some(suppression),
            MAX_M6_PARTIAL_RERUN_ACTIONS,
            working_limit,
        )
    }

    fn plan_partial_rerun_with_limits_v5(
        &self,
        preservation: &M6PreservationPhaseV5,
        suppression: Option<&TargetPredecessorSuppressionProjectionV5>,
        action_limit: usize,
        working_limit: usize,
    ) -> M6Result<(Vec<PartialRerunActionV5>, PartialRerunPlanV5)> {
        if let Some(suppression) = suppression {
            suppression.validate_seal().map_err(M6Error::from)?;
        }
        if preservation.source_closure_id != self.assessment.source_closure_id
            || preservation.morphism_id != self.assessment.morphism_id
            || preservation.correspondence_id != self.assessment.correspondence_id
            || preservation.staleness_assessment_id != self.assessment.id
            || preservation.target_snapshot_id != self.target_snapshot_id
            || preservation.evidence.len() != preservation.verifications.len()
        {
            return Err(M6Error::PreservationUnsupported(
                "preservation phase is not bound to this sealed staleness phase",
            ));
        }
        if suppression.is_some_and(|value| {
            value.target_run_id() != &self.target_run_id
                || value.target_genesis_hash() != &self.target_genesis_hash
                || value.target_tail_hash() != &self.target_tail_hash
                || value.target_event_count() != self.target_event_count
                || value.policy_revision_hash() != &self.target_policy_revision_hash
                || value.pre_incremental_basis_digest() != &self.target_pre_incremental_basis_digest
                || value.target_plan_id() != &self.target_plan_id
                || value.target_snapshot_id() != &self.target_snapshot_id
        }) {
            return Err(M6Error::InvalidStalenessAssessment(
                "target suppression projection is not bound to the sealed predecessor",
            ));
        }
        bounded(
            preservation.verifications.len(),
            MAX_M6_PRESERVATION_RECORDS,
            "M6 preservation verifications",
        )?;
        let mut verifications = preservation.verifications.clone();
        verifications.sort_by(|left, right| left.id.cmp(&right.id));
        let mut seeds = self.partial_rerun_seeds.clone();
        for verification in &verifications {
            verification.validate()?;
            if !self.is_sealed_preservation_candidate(&verification.target_obligation_id)? {
                return Err(M6Error::InvalidStalenessAssessment(
                    "preservation verification is not in the sealed candidate set",
                ));
            }
            let seed = seeds
                .entry(verification.target_obligation_id.clone())
                .or_default();
            seed.require_native_pipeline = true;
            // ADR 0023's MVP preservation admission is issue-present-only.
            // The source decision itself carries no authority, so this exact
            // preserved verification still requires a fresh target human
            // resolution.
            seed.require_human_resolution = true;
        }
        seeds.retain(|_, seed| seed.require_native_pipeline || seed.require_human_resolution);
        let selected_target_ids = seeds.keys().cloned().collect::<BTreeSet<_>>();
        bounded(
            selected_target_ids.len(),
            MAX_M6_OBLIGATIONS_PER_UNIVERSE,
            "M6 selected rerun targets",
        )?;
        #[derive(Clone, Default)]
        struct SuppressionDecision {
            context: Option<TargetSuppressionRecordWitnessV5>,
            reviewer: Option<(
                TargetSuppressionRecordWitnessV5,
                TargetSuppressionRecordWitnessV5,
            )>,
            verification: Option<TargetSuppressionRecordWitnessV5>,
            human: bool,
            reused_cardinality_unsupported: Option<(StableId, usize)>,
        }
        let mut decisions = BTreeMap::new();
        let mut action_count = 0_usize;
        for (target, seed) in &seeds {
            let mut decision = SuppressionDecision::default();
            if let Some(suppression) = suppression {
                let lifecycle = suppression.lifecycle(target);
                let reviewer = suppression.reviewer_closure(target);
                let context = suppression.context_envelope(target).cloned();
                if let Some(reviewer) = reviewer {
                    let observed = reviewer.claims().len();
                    if observed != 1 {
                        // A reused Completed execution is immutable history.
                        // Seal an actionless requirement and classify it
                        // eventlessly later; never reopen lifecycle or mint
                        // verifier/human authority from a caller-selected
                        // claim. Zero-claim Completed is rejected by frozen D2
                        // replay before a suppression projection can exist.
                        if lifecycle == Some(ObligationLifecycle::Completed)
                            && reviewer.closure_exact()
                            && context.is_some()
                        {
                            decision.context = context.clone();
                            decision.reused_cardinality_unsupported =
                                Some((reviewer.execution().record_id().clone(), observed));
                        } else {
                            return Err(M6Error::CompletedReviewerClosureMismatch {
                                obligation_id: target.clone(),
                            });
                        }
                    }
                    if observed == 1 && reviewer.closure_exact() && context.is_some() {
                        decision.reviewer =
                            Some((reviewer.execution().clone(), reviewer.claims()[0].clone()));
                    }
                }
                if lifecycle == Some(ObligationLifecycle::Completed)
                    && decision.reviewer.is_none()
                    && decision.reused_cardinality_unsupported.is_none()
                {
                    return Err(M6Error::CompletedReviewerClosureMismatch {
                        obligation_id: target.clone(),
                    });
                }
                if decision.reviewer.is_some() || lifecycle == Some(ObligationLifecycle::InProgress)
                {
                    decision.context = context;
                }
                if decision.reviewer.is_some() {
                    decision.verification = suppression.native_verification(target).cloned();
                }
                decision.human = decision.verification.is_some()
                    && suppression.human_exact(target)
                    && seed.require_human_resolution;
            }
            let subject_actions = if decision.reused_cardinality_unsupported.is_some() {
                0
            } else {
                usize::from(decision.context.is_none())
                    .checked_add(usize::from(decision.reviewer.is_none()))
                    .and_then(|count| {
                        count.checked_add(usize::from(decision.verification.is_none()))
                    })
                    .and_then(|count| {
                        count.checked_add(usize::from(
                            seed.require_human_resolution && !decision.human,
                        ))
                    })
                    .ok_or(M6Error::Incomplete {
                        operation: "M6 partial rerun action count",
                        limit: action_limit,
                        observed: usize::MAX,
                    })?
            };
            action_count =
                action_count
                    .checked_add(subject_actions)
                    .ok_or(M6Error::Incomplete {
                        operation: "M6 partial rerun action count",
                        limit: action_limit,
                        observed: usize::MAX,
                    })?;
            decisions.insert(target.clone(), decision);
        }
        bounded(action_count, action_limit, "M6 partial rerun actions")?;
        let metadata_ids = seeds.values().try_fold(0_usize, |total, seed| {
            total
                .checked_add(seed.stale_source_record_ids.len())
                .and_then(|value| value.checked_add(seed.reasons.len()))
                .ok_or(M6Error::Incomplete {
                    operation: "M6 partial rerun working bytes",
                    limit: working_limit,
                    observed: usize::MAX,
                })
        })?;
        let working_bytes =
            partial_rerun_working_bytes_v5(action_count, metadata_ids, working_limit)?;
        bounded(
            working_bytes,
            working_limit,
            "M6 partial rerun working bytes",
        )?;

        let mut actions = Vec::with_capacity(action_count);
        let mut required_human_resolution_ids = BTreeSet::new();
        for (target, seed) in &seeds {
            let decision = &decisions[target];
            if seed.require_human_resolution {
                required_human_resolution_ids.insert(target.clone());
            }
            if decision.reused_cardinality_unsupported.is_some() {
                continue;
            }
            let context = if decision.context.is_none() {
                Some(PartialRerunActionV5::derive(
                    self.assessment.id.clone(),
                    target.clone(),
                    PartialRerunActionKindV5::ReprojectContext,
                    Vec::new(),
                    seed.stale_source_record_ids.clone(),
                    seed.reasons.clone(),
                )?)
            } else {
                None
            };
            if let Some(context) = &context {
                actions.push(context.clone());
            }
            let reviewer = if decision.reviewer.is_none() {
                let prerequisites = if let Some(context) = &context {
                    vec![ActionPrerequisiteV5::scheduled(context.id.clone())?]
                } else {
                    vec![existing_target_prerequisite_v5(
                        decision
                            .context
                            .as_ref()
                            .expect("suppressed context witness"),
                    )]
                };
                Some(PartialRerunActionV5::derive(
                    self.assessment.id.clone(),
                    target.clone(),
                    PartialRerunActionKindV5::RerunReviewer,
                    prerequisites,
                    seed.stale_source_record_ids.clone(),
                    seed.reasons.clone(),
                )?)
            } else {
                None
            };
            if let Some(reviewer) = &reviewer {
                actions.push(reviewer.clone());
            }
            let verifier = if decision.verification.is_none() {
                let mut prerequisites = if let Some(reviewer) = &reviewer {
                    vec![ActionPrerequisiteV5::scheduled(reviewer.id.clone())?]
                } else {
                    let (execution, claim) = decision
                        .reviewer
                        .as_ref()
                        .expect("suppressed reviewer witnesses");
                    vec![
                        existing_target_prerequisite_v5(execution),
                        existing_target_prerequisite_v5(claim),
                    ]
                };
                prerequisites.sort();
                Some(PartialRerunActionV5::derive(
                    self.assessment.id.clone(),
                    target.clone(),
                    PartialRerunActionKindV5::RerunVerifier,
                    prerequisites,
                    seed.stale_source_record_ids.clone(),
                    seed.reasons.clone(),
                )?)
            } else {
                None
            };
            if let Some(verifier) = &verifier {
                actions.push(verifier.clone());
            }
            if seed.require_human_resolution && !decision.human {
                let prerequisites = if let Some(verifier) = &verifier {
                    vec![ActionPrerequisiteV5::scheduled(verifier.id.clone())?]
                } else {
                    vec![existing_target_prerequisite_v5(
                        decision
                            .verification
                            .as_ref()
                            .expect("suppressed native verification witness"),
                    )]
                };
                actions.push(PartialRerunActionV5::derive(
                    self.assessment.id.clone(),
                    target.clone(),
                    PartialRerunActionKindV5::RerunHumanDecision,
                    prerequisites,
                    seed.stale_source_record_ids.clone(),
                    seed.reasons.clone(),
                )?);
            }
        }
        actions.sort_by(|left, right| left.id.cmp(&right.id));
        let plan = PartialRerunPlanV5::seal(PartialRerunPlanPartsV5 {
            source_closure_id: self.assessment.source_closure_id.clone(),
            morphism_id: self.assessment.morphism_id.clone(),
            correspondence_id: self.assessment.correspondence_id.clone(),
            staleness_assessment_id: self.assessment.id.clone(),
            target_plan_id: self.target_plan_id.clone(),
            target_gluing_required: self.target_gluing_required,
            selected_target_ids: &selected_target_ids,
            actions: &actions,
            preservation_verifications: &verifications,
            required_human_resolution_ids: &required_human_resolution_ids,
        })?;
        Ok((actions, plan))
    }

    #[cfg(test)]
    fn plan_partial_rerun_with_limits_for_test(
        &self,
        preservation: &M6PreservationPhaseV5,
        action_limit: usize,
        working_limit: usize,
    ) -> M6Result<(Vec<PartialRerunActionV5>, PartialRerunPlanV5)> {
        self.plan_partial_rerun_with_limits_v5(preservation, None, action_limit, working_limit)
    }

    pub(crate) fn is_sealed_preservation_candidate(
        &self,
        target_obligation_id: &StableId,
    ) -> M6Result<bool> {
        Ok(self.assessment.preservation_candidate_count()
            == self.preservation_candidate_obligation_ids.len() as u64
            && self.assessment.preservation_candidate_digest()
                == &digest_ids(&self.preservation_candidate_obligation_ids)?
            && self
                .preservation_candidate_obligation_ids
                .contains(target_obligation_id))
    }

    #[cfg(test)]
    pub(crate) fn corrupt_obligation_assessment_stale_for_test(
        &mut self,
        source_obligation_id: &StableId,
    ) {
        let record = self
            .records
            .iter_mut()
            .find(|record| {
                record.source_record_kind == HistoricalRecordKindV5::Obligation
                    && &record.source_record_id == source_obligation_id
            })
            .expect("test obligation assessment");
        record.status = HistoricalAssessmentStatusV5::Stale;
        record.reasons.insert(StaleReasonV5::TargetChanged);
    }
    #[must_use]
    pub const fn target_gluing_required(&self) -> bool {
        self.target_gluing_required
    }
    #[must_use]
    pub fn m5_dependent_successor_obligation_ids(&self) -> &BTreeSet<StableId> {
        &self.m5_dependent_successor_obligation_ids
    }
    #[must_use]
    pub const fn working_bytes(&self) -> usize {
        self.working_bytes
    }

    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        let record_bytes = self
            .records
            .capacity()
            .saturating_mul(std::mem::size_of::<HistoricalRecordAssessmentV5>())
            .saturating_add(
                self.records
                    .iter()
                    .map(HistoricalRecordAssessmentV5::allocated_bytes)
                    .sum::<usize>(),
            );
        let gluing_bytes = self
            .gluing_freshness
            .capacity()
            .saturating_mul(std::mem::size_of::<GluingFreshnessV5>())
            .saturating_add(
                self.gluing_freshness
                    .iter()
                    .map(|value| {
                        value
                            .id
                            .allocated_bytes()
                            .saturating_add(value.assessment_id.allocated_bytes())
                            .saturating_add(value.source_attempt_id.allocated_bytes())
                            .saturating_add(id_set_heap(&value.dependency_mapping_ids))
                            .saturating_add(id_set_heap(&value.successor_target_attempt_ids))
                            .saturating_add(id_set_heap(&value.source_ids))
                    })
                    .sum::<usize>(),
            );
        std::mem::size_of::<Self>()
            .saturating_add(record_bytes)
            .saturating_add(gluing_bytes)
            .saturating_add(self.assessment.id.allocated_bytes())
            .saturating_add(self.assessment.source_closure_id.allocated_bytes())
            .saturating_add(self.assessment.morphism_id.allocated_bytes())
            .saturating_add(self.assessment.correspondence_id.allocated_bytes())
            .saturating_add(self.assessment.assessment_time.capacity())
            .saturating_add(self.assessment.record_set_digest.allocated_bytes())
            .saturating_add(
                self.assessment
                    .gluing_freshness_set_digest
                    .allocated_bytes(),
            )
            .saturating_add(self.assessment.stale_source_digest.allocated_bytes())
            .saturating_add(self.assessment.superseded_source_digest.allocated_bytes())
            .saturating_add(
                self.assessment
                    .preservation_candidate_digest
                    .allocated_bytes(),
            )
            .saturating_add(
                self.assessment
                    .m5_dependent_successor_digest
                    .allocated_bytes(),
            )
            .saturating_add(id_set_heap(&self.assessment.source_ids))
            .saturating_add(id_set_heap(&self.preservation_candidate_obligation_ids))
            .saturating_add(id_set_heap(&self.m5_dependent_successor_obligation_ids))
            .saturating_add(self.target_plan_id.allocated_bytes())
            .saturating_add(self.target_snapshot_id.allocated_bytes())
            .saturating_add(self.target_run_id.allocated_bytes())
            .saturating_add(self.target_genesis_hash.allocated_bytes())
            .saturating_add(self.target_tail_hash.allocated_bytes())
            .saturating_add(self.target_policy_revision_hash.allocated_bytes())
            .saturating_add(self.target_pre_incremental_basis_digest.allocated_bytes())
            .saturating_add(
                self.partial_rerun_seeds
                    .iter()
                    .fold(0_usize, |total, (id, seed)| {
                        total
                            .saturating_add(
                                std::mem::size_of::<(StableId, PartialRerunActionSeedV5)>(),
                            )
                            .saturating_add(128)
                            .saturating_add(id.allocated_bytes())
                            .saturating_add(id_set_heap(&seed.stale_source_record_ids))
                    }),
            )
    }
}

fn impact_rows_retained_bytes_v5(values: &BTreeMap<StableId, ObligationImpactRowV5>) -> usize {
    values.iter().fold(0_usize, |total, (id, row)| {
        total
            .saturating_add(std::mem::size_of::<(StableId, ObligationImpactRowV5)>())
            .saturating_add(128)
            .saturating_add(id.allocated_bytes())
            .saturating_add(id_set_heap(&row.direct))
            .saturating_add(id_set_heap(&row.indirect))
    })
}

fn replacements_retained_bytes_v5(values: &BTreeMap<String, String>) -> usize {
    values.iter().fold(0_usize, |total, (source, target)| {
        total
            .saturating_add(std::mem::size_of::<(String, String)>())
            .saturating_add(128)
            .saturating_add(source.capacity())
            .saturating_add(target.capacity())
    })
}

fn key_count_map_retained_bytes_v5(values: &BTreeMap<OwnedHistoricalRecordKeyV5, usize>) -> usize {
    values.iter().fold(0_usize, |total, (key, _)| {
        total
            .saturating_add(std::mem::size_of::<(OwnedHistoricalRecordKeyV5, usize)>())
            .saturating_add(128)
            .saturating_add(key.id.allocated_bytes())
    })
}

fn key_key_set_map_retained_bytes_v5(
    values: &BTreeMap<OwnedHistoricalRecordKeyV5, BTreeSet<OwnedHistoricalRecordKeyV5>>,
) -> usize {
    values.iter().fold(0_usize, |total, (key, members)| {
        total
            .saturating_add(std::mem::size_of::<(
                OwnedHistoricalRecordKeyV5,
                BTreeSet<OwnedHistoricalRecordKeyV5>,
            )>())
            .saturating_add(128)
            .saturating_add(key.id.allocated_bytes())
            .saturating_add(members.iter().fold(0_usize, |member_total, member| {
                member_total
                    .saturating_add(std::mem::size_of::<OwnedHistoricalRecordKeyV5>())
                    .saturating_add(128)
                    .saturating_add(member.id.allocated_bytes())
            }))
    })
}

fn key_id_set_map_retained_bytes_v5(
    values: &BTreeMap<OwnedHistoricalRecordKeyV5, BTreeSet<StableId>>,
) -> usize {
    values.iter().fold(0_usize, |total, (key, members)| {
        total
            .saturating_add(std::mem::size_of::<(
                OwnedHistoricalRecordKeyV5,
                BTreeSet<StableId>,
            )>())
            .saturating_add(128)
            .saturating_add(key.id.allocated_bytes())
            .saturating_add(id_set_heap(members))
    })
}

#[derive(Clone, Copy, Debug)]
struct StalenessResourcePhasesV5 {
    external: usize,
    impact_walk: usize,
    dependency_closure: usize,
    per_row_unions: usize,
    members: usize,
    gluing: usize,
    streaming_seal: usize,
}

impl StalenessResourcePhasesV5 {
    fn peak(self) -> M6Result<usize> {
        // These structures are simultaneously live in the reducer.  Summing
        // is intentionally conservative; taking their maximum would pretend
        // that the node maps disappear before row/topology/member reduction.
        let dynamic = [
            self.impact_walk,
            self.dependency_closure,
            self.per_row_unions,
            self.members,
            self.gluing,
            self.streaming_seal,
        ]
        .into_iter()
        .try_fold(0_usize, usize::checked_add)
        .ok_or(M6Error::Incomplete {
            operation: "M6 staleness simultaneous working bytes",
            limit: MAX_M6_STALENESS_WORKING_BYTES,
            observed: usize::MAX,
        })?;
        self.external
            .checked_add(dynamic)
            .ok_or(M6Error::Incomplete {
                operation: "M6 staleness phase working bytes",
                limit: MAX_M6_STALENESS_WORKING_BYTES,
                observed: usize::MAX,
            })
    }
}

impl IncrementalStalenessInputV5<'_> {
    #[cfg(test)]
    pub(crate) fn external_component_sum_for_test(
        &self,
        target_actual: &TargetActualRecordInventoryV5<'_>,
    ) -> M6Result<(usize, [usize; 5])> {
        let (mapping, correspondence) =
            staleness_mapping_correspondence_retained_bytes(self.mapping, self.correspondence)?;
        let components = [
            self.source.working_reservation_bytes(),
            self.inventory.target_retained_bytes(),
            mapping,
            correspondence,
            usize::try_from(target_actual.working_reservation_bytes()).map_err(|_| {
                M6Error::Incomplete {
                    operation: "M6 staleness external retained bytes",
                    limit: MAX_M6_STALENESS_WORKING_BYTES,
                    observed: usize::MAX,
                }
            })?,
        ];
        let expected = components
            .into_iter()
            .try_fold(0_usize, usize::checked_add)
            .ok_or(M6Error::Incomplete {
                operation: "M6 staleness external component sum",
                limit: MAX_M6_STALENESS_WORKING_BYTES,
                observed: usize::MAX,
            })?;
        let actual = self.sparse_resource_preflight_v5(target_actual)?.external;
        debug_assert_eq!(actual, expected);
        Ok((actual, components))
    }

    fn sparse_resource_preflight_v5(
        &self,
        target_actual: &TargetActualRecordInventoryV5<'_>,
    ) -> M6Result<StalenessResourcePhasesV5> {
        let mut records = 0_usize;
        let mut edges = 0_usize;
        let mut direct_ids = 0_usize;
        let mut obligation_rows = 0_usize;
        let mut source_body_bytes = 0_usize;
        let mut target_body_bytes = 0_usize;
        let mut largest_body_bytes = 0_usize;
        self.visit_source_descriptors(|descriptor| {
            records = records.checked_add(1).ok_or(M6Error::Incomplete {
                operation: "M6 staleness source record count",
                limit: MAX_M6_HISTORICAL_ASSESSMENTS,
                observed: usize::MAX,
            })?;
            descriptor
                .visit_closure_records(&self.inventory, |_, _| edges = edges.saturating_add(1));
            descriptor.visit_direct_program_ids(&self.inventory, |_| {
                direct_ids = direct_ids.saturating_add(1)
            });
            if matches!(
                descriptor.typed_body(),
                HistoricalSourceRecordValueV4::Obligation(_)
            ) {
                obligation_rows = obligation_rows.saturating_add(1);
            }
            let bytes = historical_typed_canonical_len_v5(descriptor.typed_body())?;
            source_body_bytes =
                source_body_bytes
                    .checked_add(bytes)
                    .ok_or(M6Error::Incomplete {
                        operation: "M6 source typed body bytes",
                        limit: MAX_M6_STALENESS_WORKING_BYTES,
                        observed: usize::MAX,
                    })?;
            largest_body_bytes = largest_body_bytes.max(bytes);
            Ok(())
        })?;
        bounded(
            records,
            MAX_M6_HISTORICAL_ASSESSMENTS,
            "M6 historical assessments",
        )?;
        let mut target_records = 0_usize;
        target_actual
            .try_visit_records(|record| {
                target_records = target_records.saturating_add(1);
                let bytes = historical_typed_canonical_len_v5(record.value().historical_value())
                    .map_err(|error| DomainError::Validation(error.to_string()))?;
                target_body_bytes =
                    target_body_bytes
                        .checked_add(bytes)
                        .ok_or(DomainError::Incomplete {
                            operation: "M6 target typed body bytes",
                            limit: MAX_M6_STALENESS_WORKING_BYTES,
                            observed: usize::MAX,
                        })?;
                largest_body_bytes = largest_body_bytes.max(bytes);
                Ok(())
            })
            .map_err(M6Error::from)?;
        let slot = std::mem::size_of::<StableId>() + 128;
        let checked = |left: usize, right: usize, operation| {
            left.checked_mul(right).ok_or(M6Error::Incomplete {
                operation,
                limit: MAX_M6_STALENESS_WORKING_BYTES,
                observed: usize::MAX,
            })
        };
        let (mapping_retained, correspondence_retained) =
            staleness_mapping_correspondence_retained_bytes(self.mapping, self.correspondence)?;
        // This sealed reservation is Event's capacity-aware upper image of
        // the roots-validated terminal replay, its authority basis and the
        // borrowed actual-record inventory.  Charge it once as one resident
        // backing graph: adding `retained_bytes()` separately would omit the
        // terminal/basis, while adding both would double-count the inventory.
        let target_actual_backing = usize::try_from(target_actual.working_reservation_bytes())
            .map_err(|_| M6Error::Incomplete {
                operation: "M6 staleness external retained bytes",
                limit: MAX_M6_STALENESS_WORKING_BYTES,
                observed: usize::MAX,
            })?;
        let external = self
            .source
            .working_reservation_bytes()
            .checked_add(self.inventory.target_retained_bytes())
            .and_then(|value| value.checked_add(mapping_retained))
            .and_then(|value| value.checked_add(correspondence_retained))
            .and_then(|value| value.checked_add(target_actual_backing))
            .ok_or(M6Error::Incomplete {
                operation: "M6 staleness external retained bytes",
                limit: MAX_M6_STALENESS_WORKING_BYTES,
                observed: usize::MAX,
            })?;
        Ok(StalenessResourcePhasesV5 {
            external,
            impact_walk: checked(
                obligation_rows.max(1),
                self.source
                    .program_space()
                    .relations()
                    .len()
                    .saturating_add(direct_ids)
                    .saturating_mul(slot),
                "M6 impact walk working bytes",
            )?,
            dependency_closure: checked(
                records.saturating_add(edges),
                slot * 3,
                "M6 dependency closure working bytes",
            )?,
            per_row_unions: checked(
                edges.saturating_add(direct_ids).saturating_add(records),
                slot * 6,
                "M6 per-row union working bytes",
            )?
            .checked_add(largest_body_bytes.saturating_mul(3))
            .and_then(|value| {
                value.checked_add(
                    self.mapping
                        .mappings()
                        .iter()
                        .map(ProgramMappingV5::allocated_bytes)
                        .sum::<usize>(),
                )
            })
            .ok_or(M6Error::Incomplete {
                operation: "M6 per-row union working bytes",
                limit: MAX_M6_STALENESS_WORKING_BYTES,
                observed: usize::MAX,
            })?,
            members: checked(
                records.saturating_add(target_records),
                std::mem::size_of::<AssessmentRecordNodeV5>().saturating_add(slot * 4),
                "M6 assessment member working bytes",
            )?
            .checked_add(
                source_body_bytes
                    .checked_add(target_body_bytes)
                    .and_then(|value| value.checked_mul(16))
                    .ok_or(M6Error::Incomplete {
                        operation: "M6 typed Value body working bytes",
                        limit: MAX_M6_STALENESS_WORKING_BYTES,
                        observed: usize::MAX,
                    })?,
            )
            .and_then(|value| {
                value.checked_add(
                    checked(records, 16_384, "M6 sealed assessment ownership bytes").ok()?,
                )
            })
            .ok_or(M6Error::Incomplete {
                operation: "M6 assessment member working bytes",
                limit: MAX_M6_STALENESS_WORKING_BYTES,
                observed: usize::MAX,
            })?,
            gluing: checked(
                1,
                std::mem::size_of::<GluingFreshnessV5>().saturating_add(slot * 4),
                "M6 gluing working bytes",
            )?,
            streaming_seal: MAX_M6_CANONICAL_BYTES,
        })
    }

    pub(crate) fn reduce_v5(
        &self,
        target_actual: &TargetActualRecordInventoryV5<'_>,
        assessment_time: &str,
    ) -> M6Result<M6StalenessPhaseV5> {
        // The target actual inventory is minted only by a roots-validated
        // terminal replay.  Bind every terminal coordinate and its actual
        // authority basis before it is materialized into reducer state; a
        // merely structural target prefix cannot stand in for this proof.
        if target_actual.target_run_id() != &self.closure.input.target_run_id
            || target_actual.target_genesis_hash() != &self.closure.input.target_genesis_hash
            || target_actual.target_tail_hash() != self.closure.target_predecessor_tail_hash()
            || target_actual.target_event_count()
                != self.closure.input.target_predecessor_event_count
            || target_actual.authority_policy_revision_hash()
                != self.closure.target_authority_policy_revision_hash()
            || target_actual.authority_replay_basis_digest()
                != self
                    .closure
                    .target_pre_incremental_authority_replay_basis_digest()
        {
            return Err(M6Error::InvalidHistoricalTopology(
                "target actual inventory is not the closure-bound terminal authority replay",
            ));
        }
        self.reduce_v5_with_working_limit(
            target_actual,
            assessment_time,
            MAX_M6_STALENESS_WORKING_BYTES,
        )
    }

    fn reduce_v5_with_working_limit(
        &self,
        target_actual: &TargetActualRecordInventoryV5<'_>,
        assessment_time: &str,
        working_limit: usize,
    ) -> M6Result<M6StalenessPhaseV5> {
        validate_assessment_time_v5(assessment_time)?;
        let resource = self.sparse_resource_preflight_v5(target_actual)?;
        let preflight_peak = resource.peak()?;
        bounded(
            preflight_peak,
            working_limit,
            "M6 staleness phase working bytes",
        )?;

        let mut source_nodes = BTreeMap::new();
        self.visit_source_descriptors(|descriptor| {
            let node = collect_assessment_node_v5(&descriptor, &self.inventory)?;
            if source_nodes.insert(node.key.clone(), node).is_some() {
                return Err(M6Error::InvalidHistoricalTopology(
                    "duplicate source assessment node",
                ));
            }
            Ok(())
        })?;
        let target_inventory =
            HistoricalSourceInventoryV5::new_target(target_actual, self.target.program_space())?;
        let mut target_nodes = BTreeMap::new();
        let mut target_error = None;
        target_actual
            .try_visit_records(|record| {
                let descriptor = HistoricalRecordDescriptorV5::from_target(&record);
                match collect_assessment_node_v5(&descriptor, &target_inventory) {
                    Ok(node) => {
                        if target_nodes.insert(node.key.clone(), node).is_some() {
                            target_error = Some(M6Error::InvalidHistoricalTopology(
                                "duplicate target assessment node",
                            ));
                        }
                    }
                    Err(error) => target_error = Some(error),
                }
                if target_error.is_some() {
                    Err(DomainError::HistoricalPrefixMismatch(
                        "target assessment collection failed",
                    ))
                } else {
                    Ok(())
                }
            })
            .map_err(M6Error::from)?;
        if let Some(error) = target_error {
            return Err(error);
        }

        let mut obligation_rows = BTreeMap::<StableId, ObligationImpactRowV5>::new();
        self.source.try_visit_records(|record| {
            if let HistoricalSourceRecordValueV4::Obligation(obligation) = record.value() {
                obligation_rows.insert(
                    obligation.id().clone(),
                    obligation_impact_row_v5(obligation, self.source.program_space())?,
                );
            }
            Ok::<(), M6Error>(())
        })?;

        let mut replacements = BTreeMap::<String, String>::new();
        replacements.insert(
            self.closure.source_snapshot_id().as_str().to_owned(),
            self.closure.target_snapshot_id().as_str().to_owned(),
        );
        replacements.insert(
            self.closure.input.source_run_id.as_str().to_owned(),
            self.closure.input.target_run_id.as_str().to_owned(),
        );
        replacements.insert(
            self.closure.source_universe_id().as_str().to_owned(),
            self.closure.target_universe_id().as_str().to_owned(),
        );
        for mapping in self.mapping.mappings() {
            if mapping.status() == MappingStatusV5::Preserved
                && mapping.from_ids().len() == 1
                && mapping.to_ids().len() == 1
            {
                replacements.insert(
                    mapping
                        .from_ids()
                        .first()
                        .expect("one source")
                        .as_str()
                        .to_owned(),
                    mapping
                        .to_ids()
                        .first()
                        .expect("one target")
                        .as_str()
                        .to_owned(),
                );
            }
        }
        for entry in self.correspondence.entries() {
            if entry.status() == MappingStatusV5::Preserved
                && entry.from_obligation_ids().len() == 1
                && entry.to_obligation_ids().len() == 1
            {
                replacements.insert(
                    entry
                        .from_obligation_ids()
                        .first()
                        .expect("one source")
                        .as_str()
                        .to_owned(),
                    entry
                        .to_obligation_ids()
                        .first()
                        .expect("one target")
                        .as_str()
                        .to_owned(),
                );
            }
        }

        // Canonical Kahn order guarantees that every required predecessor's
        // exact successor set is known before the dependent body comparison.
        let mut indegree = source_nodes
            .iter()
            .map(|(key, node)| (key.clone(), node.required_records.len()))
            .collect::<BTreeMap<_, _>>();
        let mut dependents =
            BTreeMap::<OwnedHistoricalRecordKeyV5, BTreeSet<OwnedHistoricalRecordKeyV5>>::new();
        for (key, node) in &source_nodes {
            for dependency in &node.required_records {
                dependents
                    .entry(dependency.clone())
                    .or_default()
                    .insert(key.clone());
            }
        }
        let mut ready = indegree
            .iter()
            .filter_map(|(key, count)| (*count == 0).then_some(key.clone()))
            .collect::<BTreeSet<_>>();
        let assessment_id = StalenessAssessmentV5::assessment_id(
            self.closure.id(),
            self.mapping.morphism().id(),
            self.correspondence.correspondence().id(),
            assessment_time,
        )?;
        let mut completed =
            BTreeMap::<OwnedHistoricalRecordKeyV5, HistoricalRecordAssessmentV5>::new();
        let mut successor_map = BTreeMap::<OwnedHistoricalRecordKeyV5, BTreeSet<StableId>>::new();
        let mut partial_rerun_seeds = BTreeMap::<StableId, PartialRerunActionSeedV5>::new();
        let mut partial_row_causality =
            BTreeMap::<(OwnedHistoricalRecordKeyV5, StableId), BTreeSet<StableId>>::new();

        while let Some(key) = ready.pop_first() {
            let node = &source_nodes[&key];
            let dependency_keys = transitive_record_closure_v5(&key, &source_nodes)?;
            let dependency_source_ids = dependency_keys
                .iter()
                .map(|key| key.id.clone())
                .collect::<BTreeSet<_>>();
            let mut program_ids = node.direct_program_ids.clone();
            for dependency in &dependency_keys {
                program_ids.extend(source_nodes[dependency].direct_program_ids.iter().cloned());
            }
            bounded(
                program_ids.len(),
                MAX_M6_RECORD_METADATA_IDS,
                "M6 Program dependency union",
            )?;
            let obligation_ids = std::iter::once(&key)
                .chain(dependency_keys.iter())
                .filter(|key| key.kind == HistoricalSourceRecordKindV4::Obligation)
                .map(|key| key.id.clone())
                .collect::<BTreeSet<_>>();
            let mapping_ids = self
                .mapping
                .mappings()
                .iter()
                .filter(|mapping| {
                    mapping
                        .from_ids()
                        .iter()
                        .chain(mapping.to_ids())
                        .any(|id| program_ids.contains(id))
                })
                .map(|mapping| mapping.id().clone())
                .collect::<BTreeSet<_>>();
            let correspondence_entry_ids = self
                .correspondence
                .entries()
                .iter()
                .filter(|entry| {
                    entry
                        .from_obligation_ids()
                        .iter()
                        .chain(entry.to_obligation_ids())
                        .any(|id| obligation_ids.contains(id))
                })
                .map(|entry| entry.id().clone())
                .collect::<BTreeSet<_>>();
            for (operation, count) in [
                ("M6 mapping ID union", mapping_ids.len()),
                ("M6 correspondence ID union", correspondence_entry_ids.len()),
                ("M6 dependency source union", dependency_source_ids.len()),
            ] {
                bounded(count, MAX_M6_RECORD_METADATA_IDS, operation)?;
            }

            let mut reasons = BTreeSet::new();
            let mut direct = false;
            let mut indirect = false;
            let mut all_rows_removed = !obligation_ids.is_empty();
            let mut successor_record_ids = BTreeSet::new();
            let record_dependency_changed = dependency_keys.iter().any(|dependency| {
                completed.get(dependency).is_some_and(|value| {
                    value.status != HistoricalAssessmentStatusV5::StructurallyPreserved
                })
            });
            let active_human = node.pinned_active_or_current
                && matches!(
                    key.kind,
                    HistoricalSourceRecordKindV4::Decision | HistoricalSourceRecordKindV4::Finding
                );
            // A decision/finding's audit closure reaches its execution and
            // plan, and therefore may contain unrelated plan obligations.
            // Human authority is narrower: it follows only the exact claim's
            // explicit obligation IDs.
            let active_human_obligation_ids = if active_human {
                active_human_obligation_ids_v5(node, &source_nodes)?
            } else {
                BTreeSet::new()
            };
            for obligation_id in &obligation_ids {
                let Some(row) = obligation_rows.get(obligation_id) else {
                    reasons.insert(StaleReasonV5::UnsupportedImpactPolicy);
                    direct = true;
                    all_rows_removed = false;
                    continue;
                };
                if !row.supported {
                    reasons.insert(StaleReasonV5::UnsupportedImpactPolicy);
                    direct = true;
                    all_rows_removed = false;
                }
                let matching_entries = self
                    .correspondence
                    .entries()
                    .iter()
                    .filter(|entry| entry.from_obligation_ids().contains(obligation_id))
                    .collect::<Vec<_>>();
                let mut row_removed = matching_entries.len() == 1;
                if matching_entries.is_empty() {
                    reasons.insert(StaleReasonV5::ObligationChanged);
                    direct = true;
                }
                for entry in matching_entries {
                    match entry.status() {
                        MappingStatusV5::Preserved => row_removed = false,
                        MappingStatusV5::Modified => {
                            reasons.insert(correspondence_direct_reason_v5(
                                entry,
                                &source_nodes,
                                &target_nodes,
                            ));
                            direct = true;
                            row_removed = false;
                        }
                        MappingStatusV5::Removed if entry.to_obligation_ids().is_empty() => {
                            reasons.insert(StaleReasonV5::TargetChanged);
                            direct = true;
                        }
                        MappingStatusV5::Added | MappingStatusV5::Removed => {
                            reasons.insert(StaleReasonV5::TargetChanged);
                            direct = true;
                            row_removed = false;
                        }
                        MappingStatusV5::Split
                        | MappingStatusV5::Merged
                        | MappingStatusV5::Unresolved => {
                            reasons.insert(StaleReasonV5::MappingUnresolved);
                            direct = true;
                            row_removed = false;
                        }
                    }
                }
                all_rows_removed &= row_removed;
                let row_dependency_witness_ids = dependency_keys
                    .iter()
                    .filter_map(|dependency| {
                        partial_row_causality.get(&(dependency.clone(), obligation_id.clone()))
                    })
                    .flatten()
                    .cloned()
                    .collect::<BTreeSet<_>>();
                let row_dependency_changed = !row_dependency_witness_ids.is_empty();
                let stale_row_witness_ids = std::iter::once(key.id.clone())
                    .chain(row_dependency_witness_ids.iter().cloned())
                    .collect::<BTreeSet<_>>();
                let row_successors = row_successors_v5(
                    &key,
                    node,
                    Some(obligation_id),
                    self.correspondence,
                    &target_nodes,
                    &replacements,
                    &successor_map,
                )?;
                let no_actual_row_candidate = exact_target_candidates_for_row_v5(
                    &key,
                    Some(obligation_id),
                    self.correspondence,
                    &target_nodes,
                )
                .is_empty();
                let row_target_missing = no_actual_row_candidate
                    || (obligation_ids.len() == 1 && row_successors.is_empty());
                let emitted_rows = partial_rerun_row_v5(
                    key.kind,
                    &stale_row_witness_ids,
                    obligation_id,
                    row,
                    &mapping_ids,
                    row_target_missing,
                    row_dependency_changed,
                    active_human && active_human_obligation_ids.contains(obligation_id),
                    self.mapping,
                    self.correspondence,
                    self.source.program_space(),
                    &source_nodes,
                    &target_nodes,
                )?;
                if !emitted_rows.is_empty() {
                    partial_row_causality
                        .insert((key.clone(), obligation_id.clone()), stale_row_witness_ids);
                }
                for (target, seed) in emitted_rows {
                    let target_seed = partial_rerun_seeds.entry(target).or_default();
                    target_seed.require_native_pipeline |= seed.require_native_pipeline;
                    target_seed.require_human_resolution |= seed.require_human_resolution;
                    target_seed
                        .stale_source_record_ids
                        .extend(seed.stale_source_record_ids);
                    target_seed.reasons.extend(seed.reasons);
                    bounded(
                        target_seed.stale_source_record_ids.len(),
                        MAX_M6_ACTION_METADATA_IDS,
                        "M6 action-seed stale records",
                    )?;
                }
                successor_record_ids.extend(row_successors);
                for mapping in self
                    .mapping
                    .mappings()
                    .iter()
                    .filter(|mapping| mapping_ids.contains(mapping.id()))
                {
                    let direct_position =
                        mapping.from_ids().iter().any(|id| row.direct.contains(id));
                    let indirect_position = mapping
                        .from_ids()
                        .iter()
                        .any(|id| row.indirect.contains(id));
                    let mapping_reason =
                        mapping_direct_reason_v5(mapping, self.source.program_space());
                    let direct_reason = if mapping_reason == StaleReasonV5::TargetChanged {
                        record_class_reason_v5(key.kind)
                    } else {
                        mapping_reason
                    };
                    let (mapping_reasons, mapping_directness) = reduce_mapping_position_v5(
                        mapping.status(),
                        direct_position,
                        indirect_position,
                        direct_reason,
                    );
                    reasons.extend(mapping_reasons);
                    direct |= matches!(
                        mapping_directness,
                        StalenessDirectnessV5::Direct | StalenessDirectnessV5::DirectAndIndirect
                    );
                    indirect |= matches!(
                        mapping_directness,
                        StalenessDirectnessV5::Indirect | StalenessDirectnessV5::DirectAndIndirect
                    );
                }
            }
            if obligation_ids.is_empty() {
                all_rows_removed = !mapping_ids.is_empty()
                    && self
                        .mapping
                        .mappings()
                        .iter()
                        .filter(|mapping| mapping_ids.contains(mapping.id()))
                        .all(|mapping| mapping.status() == MappingStatusV5::Removed);
                successor_record_ids.extend(row_successors_v5(
                    &key,
                    node,
                    None,
                    self.correspondence,
                    &target_nodes,
                    &replacements,
                    &successor_map,
                )?);
            }
            if record_dependency_changed {
                reasons.insert(StaleReasonV5::DependencyChanged);
                indirect = true;
            }
            if active_human {
                reasons.insert(StaleReasonV5::HumanAuthorityNotCarried);
                direct = true;
            }

            bounded(
                successor_record_ids.len(),
                MAX_M6_RECORD_METADATA_IDS,
                "M6 successor record union",
            )?;
            let (status, directness) = if key.kind == HistoricalSourceRecordKindV4::Coverage {
                reasons.insert(StaleReasonV5::TargetChanged);
                direct = true;
                (
                    HistoricalAssessmentStatusV5::Superseded,
                    directness_v5(direct, indirect),
                )
            } else if successor_record_ids.is_empty()
                && all_rows_removed
                && !node.pinned_active_or_current
            {
                if reasons.is_empty() {
                    reasons.insert(StaleReasonV5::TargetChanged);
                    direct = true;
                }
                (
                    HistoricalAssessmentStatusV5::Superseded,
                    directness_v5(direct, indirect),
                )
            } else if !successor_record_ids.is_empty() && reasons.is_empty() {
                (
                    HistoricalAssessmentStatusV5::StructurallyPreserved,
                    StalenessDirectnessV5::NotApplicable,
                )
            } else {
                if successor_record_ids.is_empty() {
                    reasons.insert(StaleReasonV5::TargetChanged);
                    direct = true;
                }
                (
                    HistoricalAssessmentStatusV5::Stale,
                    directness_v5(direct, indirect),
                )
            };
            let assessment =
                HistoricalRecordAssessmentV5::from_parts(HistoricalRecordAssessmentPartsV5 {
                    assessment_id: assessment_id.clone(),
                    source_record_kind: HistoricalRecordKindV5::from_internal(key.kind),
                    source_record_id: key.id.clone(),
                    source_record_body_hash: node.body_hash.clone(),
                    successor_record_ids: successor_record_ids.clone(),
                    status,
                    directness,
                    reasons,
                    dependency_source_ids,
                    mapping_ids,
                    correspondence_entry_ids,
                })?;
            successor_map.insert(key.clone(), successor_record_ids);
            completed.insert(key.clone(), assessment);
            for dependent in dependents.get(&key).into_iter().flatten() {
                let count =
                    indegree
                        .get_mut(dependent)
                        .ok_or(M6Error::InvalidHistoricalTopology(
                            "dependent has no indegree",
                        ))?;
                *count = count
                    .checked_sub(1)
                    .ok_or(M6Error::InvalidHistoricalTopology(
                        "assessment indegree underflow",
                    ))?;
                if *count == 0 {
                    ready.insert(dependent.clone());
                }
            }
        }
        if completed.len() != source_nodes.len() {
            return Err(M6Error::InvalidHistoricalTopology(
                "assessment dependency graph did not close",
            ));
        }
        // Added target obligations have no historical source record from
        // which the row loop above could emit a seed.  Source-less unresolved
        // components have the same shape and must remain conservative.
        add_correspondence_only_partial_seeds_v5(self.correspondence, &mut partial_rerun_seeds);
        let records = completed.into_values().collect::<Vec<_>>();
        let source_attempt = records
            .iter()
            .find(|value| value.source_record_kind == HistoricalRecordKindV5::GluingAttempt)
            .ok_or(M6Error::InvalidStalenessAssessment(
                "source has no sole gluing attempt",
            ))?;
        if records
            .iter()
            .filter(|value| value.source_record_kind == HistoricalRecordKindV5::GluingAttempt)
            .count()
            != 1
        {
            return Err(M6Error::InvalidStalenessAssessment(
                "source must have exactly one gluing attempt",
            ));
        }
        let gluing_status =
            if source_attempt.status == HistoricalAssessmentStatusV5::StructurallyPreserved {
                HistoricalAssessmentStatusV5::StructurallyPreserved
            } else if source_attempt.status == HistoricalAssessmentStatusV5::Superseded {
                HistoricalAssessmentStatusV5::Superseded
            } else {
                HistoricalAssessmentStatusV5::Stale
            };
        let gluing_reasons = source_attempt.reasons.clone();
        let gluing = vec![GluingFreshnessV5::derive(
            assessment_id.clone(),
            source_attempt.source_record_id.clone(),
            gluing_status,
            gluing_reasons,
            source_attempt.mapping_ids.clone(),
            source_attempt.successor_record_ids.clone(),
        )?];

        let selected_target = self
            .target
            .plan()
            .waves()
            .iter()
            .flat_map(|wave| wave.obligation_ids())
            .cloned()
            .collect::<BTreeSet<_>>();
        let target_payment = target_nodes
            .values()
            .filter_map(|node| {
                if node.key.kind != HistoricalSourceRecordKindV4::Obligation {
                    return None;
                }
                let value = node.body.as_object()?;
                (value.get("property_id")?.as_str()? == "payment.at_most_once"
                    && selected_target.contains(&node.key.id))
                .then_some(node.key.id.clone())
            })
            .collect::<BTreeSet<_>>();
        let invariant_present = self
            .target
            .program_space()
            .invariants()
            .iter()
            .any(|invariant| invariant.id.as_str() == "invariant:payment-at-most-once");
        let target_gluing_required = !target_payment.is_empty() && invariant_present;
        let m5_dependent_successors = if target_gluing_required {
            target_payment
        } else {
            BTreeSet::new()
        };
        let preservation_candidates = records
            .iter()
            .filter(|record| {
                record.source_record_kind == HistoricalRecordKindV5::Obligation
                    && record.status == HistoricalAssessmentStatusV5::StructurallyPreserved
                    && source_nodes
                        .get(&OwnedHistoricalRecordKeyV5 {
                            kind: HistoricalSourceRecordKindV4::Obligation,
                            id: record.source_record_id.clone(),
                        })
                        .and_then(|node| match &node.body {
                            Value::Object(fields) => {
                                fields.get("property_id").and_then(Value::as_str)
                            }
                            _ => None,
                        })
                        == Some("payment.at_most_once")
            })
            .flat_map(|record| record.successor_record_ids.iter().cloned())
            .collect::<BTreeSet<_>>();
        let seal_parts = StalenessAssessmentSealPartsV5 {
            source_closure_id: self.closure.id().clone(),
            morphism_id: self.mapping.morphism().id().clone(),
            correspondence_id: self.correspondence.correspondence().id().clone(),
            assessment_time: assessment_time.to_owned(),
            records,
            gluing,
            preservation_candidates,
            m5_dependent_successors: m5_dependent_successors.clone(),
        };
        let assessment = StalenessAssessmentV5::seal(&seal_parts)?;
        let phase = M6StalenessPhaseV5 {
            records: seal_parts.records,
            gluing_freshness: seal_parts.gluing,
            assessment,
            preservation_candidate_obligation_ids: seal_parts.preservation_candidates,
            target_gluing_required,
            m5_dependent_successor_obligation_ids: m5_dependent_successors,
            partial_rerun_seeds,
            target_plan_id: self.target.plan().id().clone(),
            target_snapshot_id: self.target.program_space().snapshot_id().clone(),
            target_run_id: target_actual.target_run_id().clone(),
            target_genesis_hash: target_actual.target_genesis_hash().clone(),
            target_tail_hash: target_actual.target_tail_hash().clone(),
            target_event_count: target_actual.target_event_count(),
            target_policy_revision_hash: target_actual.authority_policy_revision_hash().clone(),
            target_pre_incremental_basis_digest: target_actual
                .authority_replay_basis_digest()
                .clone(),
            working_bytes: preflight_peak,
        };
        // Capacity-aware second gate covers the actual retained result before
        // it can escape as a sealed phase.
        let realized = resource
            .external
            .checked_add(assessment_node_map_retained_bytes_v5(&source_nodes))
            .and_then(|value| {
                value.checked_add(assessment_node_map_retained_bytes_v5(&target_nodes))
            })
            .and_then(|value| value.checked_add(impact_rows_retained_bytes_v5(&obligation_rows)))
            .and_then(|value| value.checked_add(replacements_retained_bytes_v5(&replacements)))
            .and_then(|value| value.checked_add(key_count_map_retained_bytes_v5(&indegree)))
            .and_then(|value| value.checked_add(key_key_set_map_retained_bytes_v5(&dependents)))
            .and_then(|value| value.checked_add(key_id_set_map_retained_bytes_v5(&successor_map)))
            .and_then(|value| value.checked_add(phase.retained_bytes()))
            .ok_or(M6Error::Incomplete {
                operation: "M6 staleness realized retained bytes",
                limit: working_limit,
                observed: usize::MAX,
            })?;
        bounded(
            realized,
            working_limit,
            "M6 staleness realized retained bytes",
        )?;
        if realized > preflight_peak {
            return Err(M6Error::Incomplete {
                operation: "M6 staleness reservation underflow",
                limit: preflight_peak,
                observed: realized,
            });
        }
        Ok(phase)
    }

    /// Test-only boundary seam for proving that the sealed resource oracle is
    /// inclusive and that one byte below it yields no phase value.
    #[cfg(test)]
    pub(crate) fn reduce_v5_with_working_limit_for_test(
        &self,
        target_actual: &TargetActualRecordInventoryV5<'_>,
        assessment_time: &str,
        working_limit: usize,
    ) -> M6Result<M6StalenessPhaseV5> {
        self.reduce_v5_with_working_limit(target_actual, assessment_time, working_limit)
    }
}

/// A `(kind, id)` key is required because claim assessments deliberately use
/// their claim ID; collapsing it with a claim would lose an ADR 0023 source
/// record.  This is input-only and carries no event or append authority.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct HistoricalRecordKeyV5<'a> {
    pub(crate) kind: HistoricalSourceRecordKindV4,
    pub(crate) id: &'a StableId,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct OwnedHistoricalRecordKeyV5 {
    kind: HistoricalSourceRecordKindV4,
    id: StableId,
}

/// Typed source body retained for an actual-successor predicate.  The reducer
/// must compare this concrete DTO, never a generic JSON rendering, prose, or
/// inferred StableId set.
#[derive(Clone)]
pub(crate) struct HistoricalRecordDescriptorV5<'a> {
    key: HistoricalRecordKeyV5<'a>,
    body_hash: Option<&'a ContentHash>,
    pinned_active_or_current: bool,
    value: HistoricalSourceRecordValueV4<'a>,
}

/// Inventory-wide, typed reverse indexes over the one pinned historical
/// prefix.  This is deliberately not a generic `StableId -> JSON` map: every
/// entry records the source record kind that owns the referenced ID, which is
/// essential for the claim/claim-assessment same-ID case.
struct HistoricalSourceInventoryV5 {
    program_ids: BTreeSet<StableId>,
    record_keys: BTreeSet<OwnedHistoricalRecordKeyV5>,
    plans_for_obligation: BTreeMap<StableId, BTreeSet<OwnedHistoricalRecordKeyV5>>,
    claims_for_evidence: BTreeMap<StableId, BTreeSet<OwnedHistoricalRecordKeyV5>>,
    reproducing_claims_for_evidence: BTreeMap<StableId, BTreeSet<OwnedHistoricalRecordKeyV5>>,
    bindings_for_evidence: BTreeMap<StableId, BTreeSet<OwnedHistoricalRecordKeyV5>>,
    v4_registrations_for_descriptor: BTreeMap<StableId, BTreeSet<OwnedHistoricalRecordKeyV5>>,
    section_traces_for_descriptor: BTreeMap<StableId, BTreeSet<OwnedHistoricalRecordKeyV5>>,
    coverage_contributors: BTreeMap<StableId, BTreeSet<OwnedHistoricalRecordKeyV5>>,
    coverage_obligation_for_claim: BTreeMap<StableId, StableId>,
}

/// Borrow-only lookup surface shared by the real retained inventory and the
/// allocation-free admission scanner.  It keeps the descriptor's explicit
/// predecessor vocabulary in one place while ensuring the first pass never
/// clones a StableId merely to count it.
trait HistoricalInventoryViewV5 {
    fn is_program_id(&self, id: &StableId) -> bool;
    fn visit_plans_for_obligations(
        &self,
        ids: &BTreeSet<StableId>,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    );
    fn visit_claims_for_evidence(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    );
    fn visit_bindings_for_evidence(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    );
    fn visit_v4_registrations_for_descriptor(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    );
    fn visit_section_traces_for_descriptor(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    );
    fn visit_typed_trace_sources(
        &self,
        ids: &[StableId],
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    );
    fn visit_coverage_contributors(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    );
}

fn target_metadata_inventory_reservation_v5(
    target: &TargetActualRecordInventoryV5<'_>,
    target_program: &ProgramSpace,
) -> M6Result<usize> {
    let mut occurrences = 0_usize;
    let mut id_bytes = 0_usize;
    let mut add_id = |id: &StableId| -> M6Result<()> {
        occurrences = occurrences.checked_add(1).ok_or(M6Error::Incomplete {
            operation: "M6 target metadata inventory occurrences",
            limit: MAX_M6_STALENESS_WORKING_BYTES,
            observed: usize::MAX,
        })?;
        id_bytes = id_bytes
            .checked_add(id.allocated_bytes())
            .ok_or(M6Error::Incomplete {
                operation: "M6 target metadata inventory ID bytes",
                limit: MAX_M6_STALENESS_WORKING_BYTES,
                observed: usize::MAX,
            })?;
        Ok(())
    };
    let mut error = None;
    target_program.visit_known_ids(|id| {
        if error.is_none() {
            error = add_id(id).err();
        }
    });
    if let Some(error) = error.take() {
        return Err(error);
    }
    target.try_visit_records(|record| {
        let mut visit = |id: &StableId| {
            if error.is_none() {
                error = add_id(id).err();
            }
        };
        visit(record.id()); // one canonical record key
        match record.value().historical_value() {
            HistoricalSourceRecordValueV4::ReviewPlan(plan) => {
                for wave in plan.waves() {
                    for id in wave.obligation_ids() {
                        visit(id);
                    }
                }
            }
            HistoricalSourceRecordValueV4::Claim(claim) if claim.obligation_ids().len() == 1 => {
                visit(claim.id()); // claim-to-obligation owner
                visit(claim.id()); // possible coverage contributor
            }
            HistoricalSourceRecordValueV4::ClaimAssessment(value) => visit(value.claim_id()),
            HistoricalSourceRecordValueV4::Evidence(value) => visit(value.id()),
            HistoricalSourceRecordValueV4::EvidenceBinding(binding) => {
                // all-relation claim/evidence reverse indexes plus the
                // reproduces-only coverage reverse index upper image.
                for id in [
                    binding.evidence_id(),
                    binding.claim_id(),
                    binding.evidence_id(),
                    binding.id(),
                    binding.evidence_id(),
                    binding.claim_id(),
                ] {
                    visit(id);
                }
            }
            HistoricalSourceRecordValueV4::Verification(value) => visit(value.claim_id()),
            HistoricalSourceRecordValueV4::Decision(value) => visit(value.claim_id()),
            HistoricalSourceRecordValueV4::Finding(value) => visit(value.claim_id()),
            HistoricalSourceRecordValueV4::ArtifactRegistrationV4(registration) => {
                if let ArtifactSourceV4::GluingInput { descriptor_id, .. } = registration.source() {
                    visit(descriptor_id);
                    visit(registration.id());
                }
            }
            HistoricalSourceRecordValueV4::Section(section) => {
                visit(section.projection_input_descriptor_id());
                visit(section.projection_claim_assessment_id());
                for ids in [
                    section.projection_binding_ids(),
                    section.projection_evidence_ids(),
                    section.projection_verification_ids(),
                    section.projection_decision_ids(),
                    section.projection_finding_ids(),
                ] {
                    for id in ids {
                        visit(id);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    })?;
    if let Some(error) = error {
        return Err(error);
    }
    occurrences
        .checked_mul(1_024)
        .and_then(|bytes| bytes.checked_add(id_bytes.saturating_mul(4)))
        // Only one record's unique-union scratch is live at once.
        .and_then(|bytes| bytes.checked_add(MAX_M6_RECORD_METADATA_IDS * 1_024))
        .ok_or(M6Error::Incomplete {
            operation: "M6 target metadata inventory bytes",
            limit: MAX_M6_STALENESS_WORKING_BYTES,
            observed: usize::MAX,
        })
}

impl HistoricalSourceInventoryV5 {
    fn new(
        source: &HistoricalPrefixProjectionV4<'_>,
        source_program: &ProgramSpace,
    ) -> M6Result<Self> {
        let mut value = Self {
            program_ids: source_program.known_ids(),
            record_keys: BTreeSet::new(),
            plans_for_obligation: BTreeMap::new(),
            claims_for_evidence: BTreeMap::new(),
            reproducing_claims_for_evidence: BTreeMap::new(),
            bindings_for_evidence: BTreeMap::new(),
            v4_registrations_for_descriptor: BTreeMap::new(),
            section_traces_for_descriptor: BTreeMap::new(),
            coverage_contributors: BTreeMap::new(),
            coverage_obligation_for_claim: BTreeMap::new(),
        };
        source.try_visit_records(|record| {
            let descriptor = HistoricalRecordDescriptorV5::from_projection(record);
            let key = OwnedHistoricalRecordKeyV5 {
                kind: descriptor.key().kind,
                id: descriptor.key().id.clone(),
            };
            if !value.record_keys.insert(key.clone()) {
                return Err(M6Error::InvalidSourceClosure(
                    "duplicate historical source (kind,id) inventory key",
                ));
            }
            match descriptor.typed_body() {
                HistoricalSourceRecordValueV4::ReviewPlan(plan) => {
                    for wave in plan.waves() {
                        for id in wave.obligation_ids() {
                            value
                                .plans_for_obligation
                                .entry(id.clone())
                                .or_default()
                                .insert(key.clone());
                        }
                    }
                }
                HistoricalSourceRecordValueV4::Claim(claim)
                    if claim.obligation_ids().len() == 1 =>
                {
                    // The coverage reducer accepts only one-obligation claims
                    // in the selected plan closure.  Index no claim outside
                    // an actual numerator; its plan/envelope/execution chain
                    // must not contaminate a coverage source closure.
                    let id = claim
                        .obligation_ids()
                        .iter()
                        .next()
                        .expect("one obligation checked above");
                    let coverage = source.coverage();
                    let contributes = coverage.evidence_supported_obligation_ids().contains(id)
                        || coverage.verified_obligation_ids().contains(id)
                        || coverage.fresh_obligation_ids().contains(id)
                        || coverage.human_accepted_obligation_ids().contains(id);
                    if contributes {
                        value
                            .coverage_obligation_for_claim
                            .insert(claim.id().clone(), id.clone());
                        value
                            .coverage_contributors
                            .entry(id.clone())
                            .or_default()
                            .insert(key.clone());
                    }
                }
                HistoricalSourceRecordValueV4::ClaimAssessment(assessment) => {
                    if let Some(obligation_id) = value
                        .coverage_obligation_for_claim
                        .get(assessment.claim_id())
                        .filter(|id| {
                            source
                                .coverage()
                                .human_accepted_obligation_ids()
                                .contains(*id)
                        })
                    {
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(key.clone());
                    }
                }
                HistoricalSourceRecordValueV4::EvidenceBinding(binding) => {
                    let contributes = value
                        .coverage_obligation_for_claim
                        .get(binding.claim_id())
                        .is_some_and(|id| {
                            source
                                .coverage()
                                .evidence_supported_obligation_ids()
                                .contains(id)
                        })
                        && binding.relation() == crate::EvidenceRelationV3::Reproduces;
                    value
                        .claims_for_evidence
                        .entry(binding.evidence_id().clone())
                        .or_default()
                        .insert(OwnedHistoricalRecordKeyV5 {
                            kind: HistoricalSourceRecordKindV4::Claim,
                            id: binding.claim_id().clone(),
                        });
                    value
                        .bindings_for_evidence
                        .entry(binding.evidence_id().clone())
                        .or_default()
                        .insert(key.clone());
                    if contributes
                        && let Some(obligation_id) =
                            value.coverage_obligation_for_claim.get(binding.claim_id())
                    {
                        value
                            .reproducing_claims_for_evidence
                            .entry(binding.evidence_id().clone())
                            .or_default()
                            .insert(OwnedHistoricalRecordKeyV5 {
                                kind: HistoricalSourceRecordKindV4::Claim,
                                id: binding.claim_id().clone(),
                            });
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(key.clone());
                    }
                }
                HistoricalSourceRecordValueV4::Evidence(_) => {}
                HistoricalSourceRecordValueV4::Verification(verification) => {
                    if let Some(obligation_id) = value
                        .coverage_obligation_for_claim
                        .get(verification.claim_id())
                        .filter(|id| source.coverage().verified_obligation_ids().contains(*id))
                        .filter(|_| verification.outcome() == VerificationOutcomeV3::Passed)
                    {
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(key.clone());
                    }
                }
                HistoricalSourceRecordValueV4::Decision(decision) => {
                    if let Some(obligation_id) = value
                        .coverage_obligation_for_claim
                        .get(decision.claim_id())
                        .filter(|id| {
                            source
                                .coverage()
                                .human_accepted_obligation_ids()
                                .contains(*id)
                        })
                        .filter(|_| decision.outcome() == DecisionOutcomeV3::Accept)
                    {
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(key.clone());
                    }
                }
                HistoricalSourceRecordValueV4::Finding(finding) => {
                    if let Some(obligation_id) = value
                        .coverage_obligation_for_claim
                        .get(finding.claim_id())
                        .filter(|id| {
                            source
                                .coverage()
                                .human_accepted_obligation_ids()
                                .contains(*id)
                        })
                        .filter(|_| finding.status() == FindingStatusV3::Accepted)
                    {
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(key.clone());
                    }
                }
                HistoricalSourceRecordValueV4::ArtifactRegistrationV4(registration) => {
                    if let ArtifactSourceV4::GluingInput { descriptor_id, .. } =
                        registration.source()
                    {
                        value
                            .v4_registrations_for_descriptor
                            .entry(descriptor_id.clone())
                            .or_default()
                            .insert(key);
                    }
                }
                HistoricalSourceRecordValueV4::Section(section) => {
                    let traces = value
                        .section_traces_for_descriptor
                        .entry(section.projection_input_descriptor_id().clone())
                        .or_default();
                    traces.insert(OwnedHistoricalRecordKeyV5 {
                        kind: HistoricalSourceRecordKindV4::ClaimAssessment,
                        id: section.projection_claim_assessment_id().clone(),
                    });
                    for (kind, ids) in [
                        (
                            HistoricalSourceRecordKindV4::EvidenceBinding,
                            section.projection_binding_ids(),
                        ),
                        (
                            HistoricalSourceRecordKindV4::Evidence,
                            section.projection_evidence_ids(),
                        ),
                        (
                            HistoricalSourceRecordKindV4::Verification,
                            section.projection_verification_ids(),
                        ),
                        (
                            HistoricalSourceRecordKindV4::Decision,
                            section.projection_decision_ids(),
                        ),
                        (
                            HistoricalSourceRecordKindV4::Finding,
                            section.projection_finding_ids(),
                        ),
                    ] {
                        for id in ids {
                            traces.insert(OwnedHistoricalRecordKeyV5 {
                                kind,
                                id: id.clone(),
                            });
                        }
                    }
                }
                _ => {}
            }
            Ok::<(), M6Error>(())
        })?;
        // Evidence is listed before bindings in the canonical inventory.
        // Resolve its coverage ownership only after the complete binding
        // reverse index exists; this is a second borrow-only scan, not an
        // inferred StableId relation.
        source.try_visit_records(|record| {
            let descriptor = HistoricalRecordDescriptorV5::from_projection(record);
            if let HistoricalSourceRecordValueV4::Evidence(evidence) = descriptor.typed_body()
                && let Some(claims) = value.reproducing_claims_for_evidence.get(evidence.id())
            {
                for claim_key in claims {
                    if let Some(obligation_id) =
                        value.coverage_obligation_for_claim.get(&claim_key.id)
                    {
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(OwnedHistoricalRecordKeyV5 {
                                kind: descriptor.key().kind,
                                id: descriptor.key().id.clone(),
                            });
                    }
                }
            }
            Ok::<(), M6Error>(())
        })?;
        Ok(value)
    }

    fn new_target(
        target: &TargetActualRecordInventoryV5<'_>,
        target_program: &ProgramSpace,
    ) -> M6Result<Self> {
        let metadata_reservation =
            target_metadata_inventory_reservation_v5(target, target_program)?;
        let combined_reservation = usize::try_from(target.working_reservation_bytes())
            .unwrap_or(usize::MAX)
            .checked_add(metadata_reservation)
            .ok_or(M6Error::Incomplete {
                operation: "M6 target metadata combined working bytes",
                limit: MAX_M6_STALENESS_WORKING_BYTES,
                observed: usize::MAX,
            })?;
        if combined_reservation > MAX_M6_STALENESS_WORKING_BYTES {
            return Err(M6Error::Incomplete {
                operation: "M6 target metadata combined working bytes",
                limit: MAX_M6_STALENESS_WORKING_BYTES,
                observed: combined_reservation,
            });
        }
        let mut value = Self {
            program_ids: target_program.known_ids(),
            record_keys: BTreeSet::new(),
            plans_for_obligation: BTreeMap::new(),
            claims_for_evidence: BTreeMap::new(),
            reproducing_claims_for_evidence: BTreeMap::new(),
            bindings_for_evidence: BTreeMap::new(),
            v4_registrations_for_descriptor: BTreeMap::new(),
            section_traces_for_descriptor: BTreeMap::new(),
            coverage_contributors: BTreeMap::new(),
            coverage_obligation_for_claim: BTreeMap::new(),
        };
        target.try_visit_records(|record| {
            let descriptor = HistoricalRecordDescriptorV5::from_target(&record);
            let key = OwnedHistoricalRecordKeyV5 {
                kind: descriptor.key().kind,
                id: descriptor.key().id.clone(),
            };
            if !value.record_keys.insert(key.clone()) {
                return Err(DomainError::HistoricalPrefixMismatch(
                    "duplicate target actual (kind,id) inventory key",
                ));
            }
            match descriptor.typed_body() {
                HistoricalSourceRecordValueV4::ReviewPlan(plan) => {
                    for wave in plan.waves() {
                        for id in wave.obligation_ids() {
                            value
                                .plans_for_obligation
                                .entry(id.clone())
                                .or_default()
                                .insert(key.clone());
                        }
                    }
                }
                HistoricalSourceRecordValueV4::Claim(claim)
                    if claim.obligation_ids().len() == 1 =>
                {
                    value.coverage_obligation_for_claim.insert(
                        claim.id().clone(),
                        claim
                            .obligation_ids()
                            .iter()
                            .next()
                            .expect("one obligation checked")
                            .clone(),
                    );
                }
                HistoricalSourceRecordValueV4::ArtifactRegistrationV4(registration) => {
                    if let ArtifactSourceV4::GluingInput { descriptor_id, .. } =
                        registration.source()
                    {
                        value
                            .v4_registrations_for_descriptor
                            .entry(descriptor_id.clone())
                            .or_default()
                            .insert(key);
                    }
                }
                HistoricalSourceRecordValueV4::Section(section) => {
                    let traces = value
                        .section_traces_for_descriptor
                        .entry(section.projection_input_descriptor_id().clone())
                        .or_default();
                    traces.insert(OwnedHistoricalRecordKeyV5 {
                        kind: HistoricalSourceRecordKindV4::ClaimAssessment,
                        id: section.projection_claim_assessment_id().clone(),
                    });
                    for (kind, ids) in [
                        (
                            HistoricalSourceRecordKindV4::EvidenceBinding,
                            section.projection_binding_ids(),
                        ),
                        (
                            HistoricalSourceRecordKindV4::Evidence,
                            section.projection_evidence_ids(),
                        ),
                        (
                            HistoricalSourceRecordKindV4::Verification,
                            section.projection_verification_ids(),
                        ),
                        (
                            HistoricalSourceRecordKindV4::Decision,
                            section.projection_decision_ids(),
                        ),
                        (
                            HistoricalSourceRecordKindV4::Finding,
                            section.projection_finding_ids(),
                        ),
                    ] {
                        for id in ids {
                            traces.insert(OwnedHistoricalRecordKeyV5 {
                                kind,
                                id: id.clone(),
                            });
                        }
                    }
                }
                _ => {}
            }
            Ok(())
        })?;
        let coverage = target.coverage();
        target.try_visit_records(|record| {
            let descriptor = HistoricalRecordDescriptorV5::from_target(&record);
            let key = OwnedHistoricalRecordKeyV5 {
                kind: descriptor.key().kind,
                id: descriptor.key().id.clone(),
            };
            match descriptor.typed_body() {
                HistoricalSourceRecordValueV4::Claim(claim)
                    if claim.obligation_ids().len() == 1 =>
                {
                    let obligation_id = value
                        .coverage_obligation_for_claim
                        .get(claim.id())
                        .expect("one-obligation claim indexed in first pass");
                    if coverage
                        .evidence_supported_obligation_ids()
                        .contains(obligation_id)
                        || coverage.verified_obligation_ids().contains(obligation_id)
                        || coverage.fresh_obligation_ids().contains(obligation_id)
                        || coverage
                            .human_accepted_obligation_ids()
                            .contains(obligation_id)
                    {
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(key);
                    }
                }
                HistoricalSourceRecordValueV4::ClaimAssessment(assessment) => {
                    if let Some(obligation_id) = value
                        .coverage_obligation_for_claim
                        .get(assessment.claim_id())
                        .filter(|id| coverage.human_accepted_obligation_ids().contains(*id))
                    {
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(key);
                    }
                }
                HistoricalSourceRecordValueV4::EvidenceBinding(binding) => {
                    value
                        .claims_for_evidence
                        .entry(binding.evidence_id().clone())
                        .or_default()
                        .insert(OwnedHistoricalRecordKeyV5 {
                            kind: HistoricalSourceRecordKindV4::Claim,
                            id: binding.claim_id().clone(),
                        });
                    value
                        .bindings_for_evidence
                        .entry(binding.evidence_id().clone())
                        .or_default()
                        .insert(key.clone());
                    if binding.relation() == crate::EvidenceRelationV3::Reproduces
                        && let Some(obligation_id) = value
                            .coverage_obligation_for_claim
                            .get(binding.claim_id())
                            .filter(|id| coverage.evidence_supported_obligation_ids().contains(*id))
                    {
                        value
                            .reproducing_claims_for_evidence
                            .entry(binding.evidence_id().clone())
                            .or_default()
                            .insert(OwnedHistoricalRecordKeyV5 {
                                kind: HistoricalSourceRecordKindV4::Claim,
                                id: binding.claim_id().clone(),
                            });
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(key);
                    }
                }
                HistoricalSourceRecordValueV4::Verification(verification) => {
                    if let Some(obligation_id) = value
                        .coverage_obligation_for_claim
                        .get(verification.claim_id())
                        .filter(|id| coverage.verified_obligation_ids().contains(*id))
                        .filter(|_| verification.outcome() == VerificationOutcomeV3::Passed)
                    {
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(key);
                    }
                }
                HistoricalSourceRecordValueV4::Decision(decision) => {
                    if let Some(obligation_id) = value
                        .coverage_obligation_for_claim
                        .get(decision.claim_id())
                        .filter(|id| coverage.human_accepted_obligation_ids().contains(*id))
                        .filter(|_| decision.outcome() == DecisionOutcomeV3::Accept)
                    {
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(key);
                    }
                }
                HistoricalSourceRecordValueV4::Finding(finding) => {
                    if let Some(obligation_id) = value
                        .coverage_obligation_for_claim
                        .get(finding.claim_id())
                        .filter(|id| coverage.human_accepted_obligation_ids().contains(*id))
                        .filter(|_| finding.status() == FindingStatusV3::Accepted)
                    {
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(key);
                    }
                }
                _ => {}
            }
            Ok(())
        })?;
        target.try_visit_records(|record| {
            let descriptor = HistoricalRecordDescriptorV5::from_target(&record);
            if let HistoricalSourceRecordValueV4::Evidence(evidence) = descriptor.typed_body()
                && let Some(claims) = value.reproducing_claims_for_evidence.get(evidence.id())
            {
                for claim_key in claims {
                    if let Some(obligation_id) =
                        value.coverage_obligation_for_claim.get(&claim_key.id)
                        && coverage
                            .evidence_supported_obligation_ids()
                            .contains(obligation_id)
                    {
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(OwnedHistoricalRecordKeyV5 {
                                kind: descriptor.key().kind,
                                id: descriptor.key().id.clone(),
                            });
                    }
                }
            }
            Ok(())
        })?;
        let realized = value.target_retained_bytes();
        if realized > metadata_reservation {
            return Err(M6Error::Incomplete {
                operation: "M6 target metadata realized retained bytes",
                limit: metadata_reservation,
                observed: realized,
            });
        }
        Ok(value)
    }

    fn target_retained_bytes(&self) -> usize {
        fn id_bytes(id: &StableId) -> usize {
            std::mem::size_of::<StableId>().saturating_add(id.allocated_bytes())
        }
        fn key_bytes(key: &OwnedHistoricalRecordKeyV5) -> usize {
            std::mem::size_of::<OwnedHistoricalRecordKeyV5>()
                .saturating_add(key.id.allocated_bytes())
                .saturating_add(128)
        }
        let mut total = std::mem::size_of::<Self>();
        for id in &self.program_ids {
            total = total.saturating_add(id_bytes(id)).saturating_add(128);
        }
        for key in &self.record_keys {
            total = total.saturating_add(key_bytes(key));
        }
        for map in [
            &self.plans_for_obligation,
            &self.claims_for_evidence,
            &self.reproducing_claims_for_evidence,
            &self.bindings_for_evidence,
            &self.v4_registrations_for_descriptor,
            &self.section_traces_for_descriptor,
            &self.coverage_contributors,
        ] {
            for (id, keys) in map {
                total = total.saturating_add(id_bytes(id)).saturating_add(128);
                for key in keys {
                    total = total.saturating_add(key_bytes(key));
                }
            }
        }
        for (claim, obligation) in &self.coverage_obligation_for_claim {
            total = total
                .saturating_add(id_bytes(claim))
                .saturating_add(id_bytes(obligation))
                .saturating_add(128);
        }
        total
    }

    fn is_program_id(&self, id: &StableId) -> bool {
        self.program_ids.contains(id)
    }

    fn visit_plans_for_obligations(
        &self,
        ids: &BTreeSet<StableId>,
        mut visit: impl FnMut(&OwnedHistoricalRecordKeyV5),
    ) {
        for id in ids {
            if let Some(keys) = self.plans_for_obligation.get(id) {
                for key in keys {
                    visit(key);
                }
            }
        }
    }

    fn visit_claims_for_evidence(
        &self,
        id: &StableId,
        mut visit: impl FnMut(&OwnedHistoricalRecordKeyV5),
    ) {
        if let Some(keys) = self.claims_for_evidence.get(id) {
            for key in keys {
                visit(key);
            }
        }
    }

    fn visit_bindings_for_evidence(
        &self,
        id: &StableId,
        mut visit: impl FnMut(&OwnedHistoricalRecordKeyV5),
    ) {
        if let Some(keys) = self.bindings_for_evidence.get(id) {
            for key in keys {
                visit(key);
            }
        }
    }

    fn visit_v4_registrations_for_descriptor(
        &self,
        id: &StableId,
        mut visit: impl FnMut(&OwnedHistoricalRecordKeyV5),
    ) {
        if let Some(keys) = self.v4_registrations_for_descriptor.get(id) {
            for key in keys {
                visit(key);
            }
        }
    }

    fn visit_section_traces_for_descriptor(
        &self,
        id: &StableId,
        mut visit: impl FnMut(&OwnedHistoricalRecordKeyV5),
    ) {
        if let Some(keys) = self.section_traces_for_descriptor.get(id) {
            for key in keys {
                visit(key);
            }
        }
    }

    fn visit_typed_trace_sources(
        &self,
        ids: &[StableId],
        mut visit: impl FnMut(&OwnedHistoricalRecordKeyV5),
    ) {
        // A decision's `source_ids` is audit provenance, not a generic
        // dependency list.  Only actual inventory members of the explicitly
        // allowed M4 trace kinds may enter this closure.
        for id in ids {
            for kind in [
                HistoricalSourceRecordKindV4::Claim,
                HistoricalSourceRecordKindV4::EvidenceBinding,
                HistoricalSourceRecordKindV4::Evidence,
                HistoricalSourceRecordKindV4::Verification,
            ] {
                let key = OwnedHistoricalRecordKeyV5 {
                    kind,
                    id: id.clone(),
                };
                if self.record_keys.contains(&key) {
                    // `key` is only a transient lookup.  Visit the canonical
                    // owned key in the inventory so no reverse-lookup Vec or
                    // StableId clone survives this traversal.
                    if let Some(existing) = self.record_keys.get(&key) {
                        visit(existing);
                    }
                }
            }
        }
    }

    fn visit_coverage_contributors(
        &self,
        id: &StableId,
        mut visit: impl FnMut(&OwnedHistoricalRecordKeyV5),
    ) {
        if let Some(keys) = self.coverage_contributors.get(id) {
            for key in keys {
                visit(key);
            }
        }
    }
}

impl HistoricalInventoryViewV5 for HistoricalSourceInventoryV5 {
    fn is_program_id(&self, id: &StableId) -> bool {
        self.is_program_id(id)
    }
    fn visit_plans_for_obligations(
        &self,
        ids: &BTreeSet<StableId>,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_plans_for_obligations(ids, |key| visit(key.kind, &key.id));
    }
    fn visit_claims_for_evidence(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_claims_for_evidence(id, |key| visit(key.kind, &key.id));
    }
    fn visit_bindings_for_evidence(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_bindings_for_evidence(id, |key| visit(key.kind, &key.id));
    }
    fn visit_v4_registrations_for_descriptor(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_v4_registrations_for_descriptor(id, |key| visit(key.kind, &key.id));
    }
    fn visit_section_traces_for_descriptor(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_section_traces_for_descriptor(id, |key| visit(key.kind, &key.id));
    }
    fn visit_typed_trace_sources(
        &self,
        ids: &[StableId],
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_typed_trace_sources(ids, |key| visit(key.kind, &key.id));
    }
    fn visit_coverage_contributors(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_coverage_contributors(id, |key| visit(key.kind, &key.id));
    }
}

/// Allocation-free counterpart of the retained inventory.  Its lookup
/// methods rescan the immutable V4 replay values instead of building maps;
/// admission is cold-path work and must prove the real sparse reference count
/// before the HPP/vector allocation is allowed.
struct HistoricalAdmissionInventoryV5<'a> {
    source: &'a HistoricalPrefixAdmissionV4<'a>,
}

impl HistoricalAdmissionInventoryV5<'_> {
    fn visit_records(
        &self,
        mut visit: impl FnMut(crate::event::HistoricalSourceRecordAdmissionV4<'_>),
    ) {
        match self.source.try_visit_replay_records(|record| {
            visit(record);
            Ok::<(), std::convert::Infallible>(())
        }) {
            Ok(()) => {}
            Err(never) => match never {},
        }
    }
}

impl HistoricalInventoryViewV5 for HistoricalAdmissionInventoryV5<'_> {
    fn is_program_id(&self, id: &StableId) -> bool {
        let mut found = false;
        self.source
            .program_space()
            .visit_known_ids(|candidate| found |= candidate == id);
        found
    }

    fn visit_plans_for_obligations(
        &self,
        ids: &BTreeSet<StableId>,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_records(|record| {
            if let HistoricalSourceRecordValueV4::ReviewPlan(plan) = record.value()
                && plan
                    .waves()
                    .iter()
                    .flat_map(|wave| wave.obligation_ids())
                    .any(|id| ids.contains(id))
            {
                visit(HistoricalSourceRecordKindV4::ReviewPlan, plan.id());
            }
        });
    }

    fn visit_claims_for_evidence(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_records(|record| {
            if let HistoricalSourceRecordValueV4::EvidenceBinding(binding) = record.value()
                && binding.evidence_id() == id
            {
                visit(HistoricalSourceRecordKindV4::Claim, binding.claim_id());
            }
        });
    }

    fn visit_bindings_for_evidence(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_records(|record| {
            if let HistoricalSourceRecordValueV4::EvidenceBinding(binding) = record.value()
                && binding.evidence_id() == id
            {
                visit(HistoricalSourceRecordKindV4::EvidenceBinding, binding.id());
            }
        });
    }

    fn visit_v4_registrations_for_descriptor(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_records(|record| {
            if let HistoricalSourceRecordValueV4::ArtifactRegistrationV4(registration) = record.value()
                && matches!(registration.source(), ArtifactSourceV4::GluingInput { descriptor_id, .. } if descriptor_id == id)
            {
                visit(HistoricalSourceRecordKindV4::ArtifactRegistrationV4, registration.id());
            }
        });
    }

    fn visit_section_traces_for_descriptor(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_records(|record| {
            let HistoricalSourceRecordValueV4::Section(section) = record.value() else {
                return;
            };
            if section.projection_input_descriptor_id() != id {
                return;
            }
            visit(
                HistoricalSourceRecordKindV4::ClaimAssessment,
                section.projection_claim_assessment_id(),
            );
            for (kind, ids) in [
                (
                    HistoricalSourceRecordKindV4::EvidenceBinding,
                    section.projection_binding_ids(),
                ),
                (
                    HistoricalSourceRecordKindV4::Evidence,
                    section.projection_evidence_ids(),
                ),
                (
                    HistoricalSourceRecordKindV4::Verification,
                    section.projection_verification_ids(),
                ),
                (
                    HistoricalSourceRecordKindV4::Decision,
                    section.projection_decision_ids(),
                ),
                (
                    HistoricalSourceRecordKindV4::Finding,
                    section.projection_finding_ids(),
                ),
            ] {
                for member in ids {
                    visit(kind, member);
                }
            }
        });
    }

    fn visit_typed_trace_sources(
        &self,
        ids: &[StableId],
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        for id in ids {
            self.visit_records(|record| {
                if record.id() != id {
                    return;
                }
                if matches!(
                    record.kind(),
                    HistoricalSourceRecordKindV4::Claim
                        | HistoricalSourceRecordKindV4::EvidenceBinding
                        | HistoricalSourceRecordKindV4::Evidence
                        | HistoricalSourceRecordKindV4::Verification
                ) {
                    visit(record.kind(), record.id());
                }
            });
        }
    }

    fn visit_coverage_contributors(
        &self,
        _id: &StableId,
        _visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        // Coverage is handled as one bounded aggregate below; invoking this
        // scan per denominator member would turn an O(records) source into an
        // artificial O(records*denominator) admission cost.
    }
}

impl<'a> HistoricalRecordDescriptorV5<'a> {
    fn from_target(value: &'a TargetActualRecordV5<'_>) -> Self {
        Self {
            key: HistoricalRecordKeyV5 {
                kind: value.kind(),
                id: value.id(),
            },
            body_hash: Some(value.body_hash()),
            pinned_active_or_current: false,
            // Reuse the exact source-history typed descriptor vocabulary. A
            // target actual record therefore cannot acquire dependencies via
            // a second, drifting target-only match statement.
            value: value.value().historical_value(),
        }
    }
    fn from_admission(value: crate::event::HistoricalSourceRecordAdmissionV4<'a>) -> Self {
        Self {
            key: HistoricalRecordKeyV5 {
                kind: value.kind(),
                id: value.id(),
            },
            body_hash: None,
            pinned_active_or_current: value.pinned_active_or_current(),
            value: *value.value(),
        }
    }
    fn from_projection(value: HistoricalSourceRecordProjectionV4<'a>) -> Self {
        Self {
            key: HistoricalRecordKeyV5 {
                kind: value.kind(),
                id: value.id(),
            },
            // The hash is borrowed from the one pinned projection.  ContentHash
            // owns its canonical string; copying it here would allocate once
            // for the projection and again for each descriptor traversal.
            body_hash: Some(value.body_hash()),
            pinned_active_or_current: value.pinned_active_or_current(),
            value: *value.value(),
        }
    }
    pub(crate) const fn key(&self) -> HistoricalRecordKeyV5<'a> {
        self.key
    }
    pub(crate) const fn body_hash(&self) -> &ContentHash {
        match self.body_hash {
            Some(value) => value,
            None => panic!("historical admission descriptor has no body hash"),
        }
    }
    pub(crate) const fn pinned_active_or_current(&self) -> bool {
        self.pinned_active_or_current
    }
    pub(crate) const fn typed_body(&self) -> HistoricalSourceRecordValueV4<'a> {
        self.value
    }

    /// Visits direct ProgramSpace dependencies only.  Each source DTO kind is
    /// intentionally listed here; `source_ids` are not treated as a generic
    /// substitute for semantic dependencies.
    fn visit_direct_program_ids(
        &self,
        inventory: &impl HistoricalInventoryViewV5,
        mut visit: impl FnMut(&'a StableId),
    ) {
        // StableId kinds are namespace labels, not an M6 semantic classifier:
        // accepted ProgramSpace artifacts intentionally include file, test,
        // repository, snapshot and language-specific IDs.  An explicit field
        // becomes a Program dependency only if it is a member of this exact
        // pinned ProgramSpace inventory.
        let program = |id: &'a StableId, visit: &mut dyn FnMut(&'a StableId)| {
            if inventory.is_program_id(id) {
                visit(id);
            }
        };
        match self.value {
            HistoricalSourceRecordValueV4::Obligation(v) => {
                for id in v
                    .normalized_target_refs()
                    .iter()
                    .chain(v.normalized_context_ids())
                    .chain(v.normalized_source_ids())
                    .chain(v.qualification_ids())
                    .chain(v.generator_ids())
                {
                    program(id, &mut visit);
                }
            }
            HistoricalSourceRecordValueV4::ContextEnvelope(v) => {
                for id in v
                    .candidate_source_ids()
                    .iter()
                    .chain(v.normalized_included_source_ids())
                {
                    program(id, &mut visit);
                }
            }
            HistoricalSourceRecordValueV4::Claim(v) => {
                for id in v.target_refs().iter().chain(v.source_ids()) {
                    program(id, &mut visit);
                }
            }
            HistoricalSourceRecordValueV4::Evidence(v) => {
                for id in v.subject_ids() {
                    program(id, &mut visit);
                }
            }
            HistoricalSourceRecordValueV4::ArtifactRegistrationV3(v) => match v.source() {
                ArtifactSourceV3::SnapshotIngest { snapshot_id, .. } => {
                    program(snapshot_id, &mut visit);
                }
                ArtifactSourceV3::ExternalHarnessWitness {
                    repository_id,
                    snapshot_id,
                    test_artifact_id,
                    ..
                } => {
                    for id in [repository_id, snapshot_id, test_artifact_id] {
                        program(id, &mut visit);
                    }
                }
                ArtifactSourceV3::RunGenesis { .. }
                | ArtifactSourceV3::ReviewerExecution { .. }
                | ArtifactSourceV3::VerifierArtifact { .. } => {}
            },
            HistoricalSourceRecordValueV4::ArtifactRegistrationV4(v) => match v.source() {
                ArtifactSourceV4::SnapshotIngest { snapshot_id, .. } => {
                    program(snapshot_id, &mut visit);
                }
                ArtifactSourceV4::ExternalHarnessWitness {
                    repository_id,
                    snapshot_id,
                    test_artifact_id,
                    ..
                } => {
                    for id in [repository_id, snapshot_id, test_artifact_id] {
                        program(id, &mut visit);
                    }
                }
                ArtifactSourceV4::GluingInput {
                    context_id,
                    repository_id,
                    snapshot_id,
                    ..
                } => {
                    for id in [context_id, repository_id, snapshot_id] {
                        program(id, &mut visit);
                    }
                }
                ArtifactSourceV4::RunGenesis { .. }
                | ArtifactSourceV4::ReviewerExecution { .. }
                | ArtifactSourceV4::VerifierArtifact { .. } => {}
            },
            HistoricalSourceRecordValueV4::GluingInputDescriptor(v) => {
                for id in std::iter::once(v.context_id()).chain(v.qualification_source_ids()) {
                    program(id, &mut visit);
                }
            }
            HistoricalSourceRecordValueV4::ContextCover(v) => {
                for id in v
                    .cover_domain_ids()
                    .iter()
                    .chain(v.projection_required_context_ids())
                {
                    program(id, &mut visit);
                }
            }
            HistoricalSourceRecordValueV4::Section(v) => {
                for id in [v.context_id(), v.projection_invariant_id()] {
                    program(id, &mut visit);
                }
                for id in v.projection_qualification_source_ids() {
                    program(id, &mut visit);
                }
            }
            HistoricalSourceRecordValueV4::Restriction(v) => {
                for id in v
                    .projection_context_pair()
                    .iter()
                    .chain(v.projection_overlap_member_ids())
                    .chain(v.projection_qualification_source_ids())
                {
                    program(id, &mut visit);
                }
            }
            HistoricalSourceRecordValueV4::GlobalCandidate(v) => {
                program(v.projection_invariant_id(), &mut visit);
                for id in v.projection_qualification_source_ids() {
                    program(id, &mut visit);
                }
            }
            HistoricalSourceRecordValueV4::GluingAttempt(v) => {
                program(v.projection_invariant_id(), &mut visit);
            }
            HistoricalSourceRecordValueV4::GluingObstruction(v) => {
                for id in v
                    .projection_conflicting_context_ids()
                    .iter()
                    .chain(v.projection_overlap_member_ids())
                    .chain(std::iter::once(v.projection_affected_invariant_id()))
                    .chain(v.projection_blocks())
                {
                    program(id, &mut visit);
                }
            }
            HistoricalSourceRecordValueV4::ReviewPlan(_)
            | HistoricalSourceRecordValueV4::Execution(_)
            | HistoricalSourceRecordValueV4::ClaimAssessment(_)
            | HistoricalSourceRecordValueV4::EvidenceBinding(_)
            | HistoricalSourceRecordValueV4::Verification(_)
            | HistoricalSourceRecordValueV4::Decision(_)
            | HistoricalSourceRecordValueV4::Finding(_)
            | HistoricalSourceRecordValueV4::Coverage(_) => {}
        }
    }

    /// Required predecessor records, expressed by explicit DTO fields.
    /// Ownership back-references in the M5 bundle are intentionally omitted.
    fn visit_required_records(
        &self,
        inventory: &impl HistoricalInventoryViewV5,
        mut visit: impl FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        let mut emit = |kind, id: &StableId| visit(kind, id);
        match self.value {
            HistoricalSourceRecordValueV4::Obligation(v) => {
                for id in v.normalized_depends_on() {
                    emit(HistoricalSourceRecordKindV4::Obligation, id);
                }
            }
            HistoricalSourceRecordValueV4::ReviewPlan(v) => {
                for wave in v.waves() {
                    for id in wave.obligation_ids() {
                        emit(HistoricalSourceRecordKindV4::Obligation, id);
                    }
                }
            }
            HistoricalSourceRecordValueV4::ContextEnvelope(v) => {
                for id in v.obligation_ids() {
                    emit(HistoricalSourceRecordKindV4::Obligation, id);
                }
                // Envelopes have no plan field.  The only legal association is
                // the inventory's actual plan/wave membership, never an ID
                // spelling or an inferred source_ids relation.
                inventory.visit_plans_for_obligations(v.obligation_ids(), &mut |kind, id| {
                    emit(kind, id);
                });
            }
            HistoricalSourceRecordValueV4::ArtifactRegistrationV3(v) => match v.source() {
                // Reviewer raw registration is owned by Execution.  Keeping
                // Execution -> registration is sufficient; the reverse edge
                // would make an artificial ownership cycle.
                ArtifactSourceV3::ReviewerExecution { .. } => {}
                ArtifactSourceV3::VerifierArtifact { claim_id, .. }
                | ArtifactSourceV3::ExternalHarnessWitness { claim_id, .. } => {
                    emit(HistoricalSourceRecordKindV4::Claim, claim_id);
                }
                ArtifactSourceV3::RunGenesis { .. } | ArtifactSourceV3::SnapshotIngest { .. } => {}
            },
            HistoricalSourceRecordValueV4::ArtifactRegistrationV4(v) => match v.source() {
                ArtifactSourceV4::ReviewerExecution { execution_id, .. } => {
                    emit(HistoricalSourceRecordKindV4::Execution, execution_id);
                }
                ArtifactSourceV4::VerifierArtifact { claim_id, .. }
                | ArtifactSourceV4::ExternalHarnessWitness { claim_id, .. } => {
                    emit(HistoricalSourceRecordKindV4::Claim, claim_id);
                }
                ArtifactSourceV4::GluingInput { plan_id, .. } => {
                    // Descriptor owns its materialized V4 registration. The
                    // reverse descriptor edge is excluded to retain a DAG.
                    emit(HistoricalSourceRecordKindV4::ReviewPlan, plan_id);
                }
                ArtifactSourceV4::RunGenesis { .. } | ArtifactSourceV4::SnapshotIngest { .. } => {}
            },
            HistoricalSourceRecordValueV4::Execution(v) => {
                emit(HistoricalSourceRecordKindV4::ReviewPlan, v.plan_id());
                emit(
                    HistoricalSourceRecordKindV4::ContextEnvelope,
                    v.envelope_id(),
                );
                for id in v.obligation_ids() {
                    emit(HistoricalSourceRecordKindV4::Obligation, id);
                }
                emit(
                    HistoricalSourceRecordKindV4::ArtifactRegistrationV3,
                    v.raw_artifact_registration_id(),
                );
            }
            HistoricalSourceRecordValueV4::Claim(v) => {
                emit(HistoricalSourceRecordKindV4::Execution, v.execution_id());
                for id in v.obligation_ids() {
                    emit(HistoricalSourceRecordKindV4::Obligation, id);
                }
            }
            HistoricalSourceRecordValueV4::ClaimAssessment(v) => {
                emit(HistoricalSourceRecordKindV4::Claim, v.claim_id());
                for id in v.binding_ids() {
                    emit(HistoricalSourceRecordKindV4::EvidenceBinding, id);
                }
                for id in v.evidence_ids() {
                    emit(HistoricalSourceRecordKindV4::Evidence, id);
                }
                for id in v.verification_ids() {
                    emit(HistoricalSourceRecordKindV4::Verification, id);
                }
                for id in v.decision_ids() {
                    emit(HistoricalSourceRecordKindV4::Decision, id);
                }
                for id in v.finding_ids() {
                    emit(HistoricalSourceRecordKindV4::Finding, id);
                }
            }
            HistoricalSourceRecordValueV4::Evidence(v) => {
                emit(
                    HistoricalSourceRecordKindV4::ArtifactRegistrationV3,
                    v.input_registration_id(),
                );
                emit(
                    HistoricalSourceRecordKindV4::ArtifactRegistrationV3,
                    v.output_registration_id(),
                );
                // A binding points back to evidence.  Resolve its real claim
                // through the inventory instead of adding a binding edge and
                // creating Evidence <-> EvidenceBinding ownership cycles.
                inventory.visit_claims_for_evidence(v.id(), &mut |kind, id| emit(kind, id));
            }
            HistoricalSourceRecordValueV4::EvidenceBinding(v) => {
                emit(HistoricalSourceRecordKindV4::Claim, v.claim_id());
                emit(HistoricalSourceRecordKindV4::Evidence, v.evidence_id());
            }
            HistoricalSourceRecordValueV4::Verification(v) => {
                emit(HistoricalSourceRecordKindV4::Claim, v.claim_id());
                for id in v.evidence_ids() {
                    emit(HistoricalSourceRecordKindV4::Evidence, id);
                }
                emit(
                    HistoricalSourceRecordKindV4::ArtifactRegistrationV3,
                    v.input_registration_id(),
                );
                emit(
                    HistoricalSourceRecordKindV4::ArtifactRegistrationV3,
                    v.output_registration_id(),
                );
                for id in v.evidence_ids() {
                    inventory.visit_bindings_for_evidence(id, &mut |kind, id| emit(kind, id));
                }
            }
            HistoricalSourceRecordValueV4::Decision(v) => {
                emit(HistoricalSourceRecordKindV4::Claim, v.claim_id());
                inventory.visit_typed_trace_sources(v.source_ids(), &mut |kind, id| {
                    emit(kind, id);
                });
            }
            HistoricalSourceRecordValueV4::Finding(v) => {
                emit(HistoricalSourceRecordKindV4::Claim, v.claim_id());
                if let Some(id) = v.decision_id() {
                    emit(HistoricalSourceRecordKindV4::Decision, id);
                }
                for id in v.evidence_ids() {
                    emit(HistoricalSourceRecordKindV4::Evidence, id);
                }
                for id in v.verification_ids() {
                    emit(HistoricalSourceRecordKindV4::Verification, id);
                }
                if let Some(id) = v.supersedes_finding_id() {
                    emit(HistoricalSourceRecordKindV4::Finding, id);
                }
            }
            HistoricalSourceRecordValueV4::GluingInputDescriptor(v) => {
                emit(HistoricalSourceRecordKindV4::ReviewPlan, v.plan_id());
                // Registration is the reverse owner of this descriptor.  It
                // is indexed rather than guessed from its StableId.
                inventory.visit_v4_registrations_for_descriptor(v.id(), &mut |kind, id| {
                    emit(kind, id);
                });
                inventory.visit_section_traces_for_descriptor(v.id(), &mut |kind, id| {
                    emit(kind, id);
                });
            }
            HistoricalSourceRecordValueV4::ContextCover(v) => {
                emit(HistoricalSourceRecordKindV4::ReviewPlan, v.plan_id());
                for id in v.selected_obligation_ids() {
                    emit(HistoricalSourceRecordKindV4::Obligation, id);
                }
            }
            HistoricalSourceRecordValueV4::Section(v) => {
                // Do not classify naked IDs by equality or StableId kind:
                // ClaimAssessment intentionally shares Claim's ID. Context is
                // a Program fact and consequently never a historical record.
                emit(
                    HistoricalSourceRecordKindV4::ContextCover,
                    v.projection_cover_id(),
                );
                emit(
                    HistoricalSourceRecordKindV4::Obligation,
                    v.projection_obligation_id(),
                );
                emit(HistoricalSourceRecordKindV4::Claim, v.projection_claim_id());
                emit(
                    HistoricalSourceRecordKindV4::ClaimAssessment,
                    v.projection_claim_assessment_id(),
                );
                emit(
                    HistoricalSourceRecordKindV4::GluingInputDescriptor,
                    v.projection_input_descriptor_id(),
                );
                emit(
                    HistoricalSourceRecordKindV4::ArtifactRegistrationV4,
                    v.projection_input_registration_id(),
                );
                for id in v.projection_binding_ids() {
                    emit(HistoricalSourceRecordKindV4::EvidenceBinding, id);
                }
                for id in v.projection_evidence_ids() {
                    emit(HistoricalSourceRecordKindV4::Evidence, id);
                }
                for id in v.projection_verification_ids() {
                    emit(HistoricalSourceRecordKindV4::Verification, id);
                }
                for id in v.projection_decision_ids() {
                    emit(HistoricalSourceRecordKindV4::Decision, id);
                }
                for id in v.projection_finding_ids() {
                    emit(HistoricalSourceRecordKindV4::Finding, id);
                }
            }
            HistoricalSourceRecordValueV4::Restriction(v) => {
                emit(
                    HistoricalSourceRecordKindV4::Section,
                    v.projection_section_id(),
                );
                for id in v.projection_claim_ids() {
                    emit(HistoricalSourceRecordKindV4::Claim, id);
                }
                for id in v.projection_evidence_ids() {
                    emit(HistoricalSourceRecordKindV4::Evidence, id);
                }
                for id in v.projection_verification_ids() {
                    emit(HistoricalSourceRecordKindV4::Verification, id);
                }
                for id in v.projection_decision_ids() {
                    emit(HistoricalSourceRecordKindV4::Decision, id);
                }
                for id in v.projection_finding_ids() {
                    emit(HistoricalSourceRecordKindV4::Finding, id);
                }
            }
            HistoricalSourceRecordValueV4::GluingAttempt(v) => {
                emit(
                    HistoricalSourceRecordKindV4::ContextCover,
                    v.projection_cover_id(),
                );
                for id in v.projection_input_descriptor_ids() {
                    emit(HistoricalSourceRecordKindV4::GluingInputDescriptor, id);
                }
                for id in v.projection_section_ids() {
                    emit(HistoricalSourceRecordKindV4::Section, id);
                }
                for id in v.projection_restriction_ids() {
                    emit(HistoricalSourceRecordKindV4::Restriction, id);
                }
                if let Some(id) = v.projection_global_candidate_id() {
                    emit(HistoricalSourceRecordKindV4::GlobalCandidate, id);
                }
                if let Some(id) = v.projection_obstruction_id() {
                    emit(HistoricalSourceRecordKindV4::GluingObstruction, id);
                }
                for id in v.projection_claim_ids() {
                    emit(HistoricalSourceRecordKindV4::Claim, id);
                }
                for id in v.projection_evidence_ids() {
                    emit(HistoricalSourceRecordKindV4::Evidence, id);
                }
                for id in v.projection_verification_ids() {
                    emit(HistoricalSourceRecordKindV4::Verification, id);
                }
                for id in v.projection_decision_ids() {
                    emit(HistoricalSourceRecordKindV4::Decision, id);
                }
                for id in v.projection_finding_ids() {
                    emit(HistoricalSourceRecordKindV4::Finding, id);
                }
            }
            HistoricalSourceRecordValueV4::GlobalCandidate(v) => {
                emit(
                    HistoricalSourceRecordKindV4::ContextCover,
                    v.projection_cover_id(),
                );
                for id in v.projection_required_section_ids() {
                    emit(HistoricalSourceRecordKindV4::Section, id);
                }
                for id in v.projection_restriction_ids() {
                    emit(HistoricalSourceRecordKindV4::Restriction, id);
                }
                for id in v.projection_claim_ids() {
                    emit(HistoricalSourceRecordKindV4::Claim, id);
                }
                for id in v.projection_evidence_ids() {
                    emit(HistoricalSourceRecordKindV4::Evidence, id);
                }
                for id in v.projection_verification_ids() {
                    emit(HistoricalSourceRecordKindV4::Verification, id);
                }
                for id in v.projection_decision_ids() {
                    emit(HistoricalSourceRecordKindV4::Decision, id);
                }
                for id in v.projection_finding_ids() {
                    emit(HistoricalSourceRecordKindV4::Finding, id);
                }
            }
            HistoricalSourceRecordValueV4::GluingObstruction(v) => {
                // The attempt owns its selected obstruction.  `attempt_id`
                // is therefore the one M5 ownership back-reference omitted
                // from the acyclic predecessor graph; the attempt itself
                // includes the complete option closure above.
                for id in v.projection_section_ids() {
                    emit(HistoricalSourceRecordKindV4::Section, id);
                }
                for id in v.projection_claim_ids() {
                    emit(HistoricalSourceRecordKindV4::Claim, id);
                }
                for id in v.projection_evidence_ids() {
                    emit(HistoricalSourceRecordKindV4::Evidence, id);
                }
                for id in v.projection_verification_ids() {
                    emit(HistoricalSourceRecordKindV4::Verification, id);
                }
                for id in v.projection_decision_ids() {
                    emit(HistoricalSourceRecordKindV4::Decision, id);
                }
                for id in v.projection_finding_ids() {
                    emit(HistoricalSourceRecordKindV4::Finding, id);
                }
            }
            HistoricalSourceRecordValueV4::Coverage(v) => {
                // The denominator is an obligation universe.  Numerators are
                // expanded from the aggregate's actual current M4 ownership
                // through inventory reverse indexes, never from an implied
                // state-axis relation.
                for id in v.denominator_obligation_ids() {
                    emit(HistoricalSourceRecordKindV4::Obligation, id);
                    inventory.visit_coverage_contributors(id, &mut |kind, id| emit(kind, id));
                }
            }
        }
    }

    /// Full audit closure for one source record.  This extends the acyclic
    /// predecessor graph with closed DTO references that would otherwise
    /// introduce only ownership/derived-state cycles.  The reducer uses it
    /// for dependency_source_ids and M/C selection, while preserving the
    /// acyclic graph above for deterministic successor ordering.
    fn visit_closure_records(
        &self,
        inventory: &impl HistoricalInventoryViewV5,
        mut visit: impl FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_required_records(inventory, |kind, id| visit(kind, id));
        match self.value {
            HistoricalSourceRecordValueV4::ArtifactRegistrationV3(v) => match v.source() {
                ArtifactSourceV3::ReviewerExecution { execution_id, .. } => {
                    visit(HistoricalSourceRecordKindV4::Execution, execution_id);
                }
                ArtifactSourceV3::VerifierArtifact { claim_id, .. }
                | ArtifactSourceV3::ExternalHarnessWitness { claim_id, .. } => {
                    visit(HistoricalSourceRecordKindV4::Claim, claim_id);
                }
                ArtifactSourceV3::RunGenesis { .. } | ArtifactSourceV3::SnapshotIngest { .. } => {}
            },
            HistoricalSourceRecordValueV4::Decision(v) => {
                // ClaimAssessment deliberately shares the claim StableId, but
                // it is a different historical kind.  Carry both types so a
                // decision's closure contains its actual M4 disposition and
                // the verifier/registration predecessors it owns.
                visit(HistoricalSourceRecordKindV4::ClaimAssessment, v.claim_id());
            }
            HistoricalSourceRecordValueV4::Finding(v) => {
                // A finding is current only through its decision/assessment
                // chain.  Do not infer this from an ID namespace; the typed
                // ClaimAssessment kind is explicit.
                visit(HistoricalSourceRecordKindV4::ClaimAssessment, v.claim_id());
            }
            _ => {}
        }
    }
}

/// M6-facing target descriptor stream. The descriptor and its body hash are
/// callback-scoped because coverage is freshly reduced by Core and is never
/// retained as a second target topology.
impl TargetActualRecordProjectionV5<'_, '_> {
    pub(crate) fn try_visit_historical_descriptors(
        &self,
        mut visitor: impl for<'record> FnMut(HistoricalRecordDescriptorV5<'record>) -> M6Result<()>,
    ) -> M6Result<()> {
        let target = self.materialize_inventory_v5()?;
        let mut callback_error = None;
        let result = target.try_visit_records(|record| {
            if let Err(error) = visitor(HistoricalRecordDescriptorV5::from_target(&record)) {
                callback_error = Some(error);
                return Err(DomainError::HistoricalPrefixMismatch(
                    "M6 target descriptor visitor aborted",
                ));
            }
            Ok(())
        });
        callback_error.map_or_else(|| result.map_err(M6Error::from), Err)
    }

    /// Visits each target body and the exact Program/predecessor metadata
    /// defined by the source-history descriptor. Reverse edges are resolved
    /// only by scanning actual target DTO fields; mappings and
    /// correspondences never participate in this inventory.
    pub(crate) fn try_visit_historical_metadata(
        &self,
        mut visit_record: impl for<'record> FnMut(
            &HistoricalRecordDescriptorV5<'record>,
        ) -> M6Result<()>,
        mut visit_program_dependency: impl FnMut(HistoricalRecordKeyV5<'_>, &StableId) -> M6Result<()>,
        mut visit_required_record: impl FnMut(
            HistoricalRecordKeyV5<'_>,
            HistoricalSourceRecordKindV4,
            &StableId,
        ) -> M6Result<()>,
    ) -> M6Result<()> {
        let target = self.materialize_inventory_v5()?;
        let inventory = HistoricalSourceInventoryV5::new_target(&target, self.program_space())?;
        let mut callback_error = None;
        let result = target.try_visit_records(|record| {
            let descriptor = HistoricalRecordDescriptorV5::from_target(&record);
            if let Err(error) = visit_record(&descriptor) {
                callback_error = Some(error);
                return Err(DomainError::HistoricalPrefixMismatch(
                    "M6 target metadata visitor aborted",
                ));
            }
            let key = descriptor.key();
            // Check membership and the exact +1 boundary before cloning each
            // new StableId. Thus an over-limit input never allocates storage
            // for its 513th unique dependency, while duplicates remain free.
            let mut dependency_ids = BTreeSet::new();
            let mut program_ids = BTreeSet::new();
            let mut required_keys = BTreeSet::new();
            let mut error = None;
            descriptor.visit_direct_program_ids(&inventory, |id| {
                if error.is_some() {
                    return;
                }
                if !dependency_ids.contains(id) {
                    if dependency_ids.len() == MAX_M6_RECORD_METADATA_IDS {
                        error = Some(M6Error::Incomplete {
                            operation: "M6 target record metadata IDs",
                            limit: MAX_M6_RECORD_METADATA_IDS,
                            observed: MAX_M6_RECORD_METADATA_IDS + 1,
                        });
                        return;
                    }
                    dependency_ids.insert(id.clone());
                }
                program_ids.insert(id.clone());
            });
            descriptor.visit_closure_records(&inventory, |kind, id| {
                if error.is_some() {
                    return;
                }
                if !dependency_ids.contains(id) {
                    if dependency_ids.len() == MAX_M6_RECORD_METADATA_IDS {
                        error = Some(M6Error::Incomplete {
                            operation: "M6 target record metadata IDs",
                            limit: MAX_M6_RECORD_METADATA_IDS,
                            observed: MAX_M6_RECORD_METADATA_IDS + 1,
                        });
                        return;
                    }
                    dependency_ids.insert(id.clone());
                }
                required_keys.insert((kind, id.clone()));
            });
            for id in &program_ids {
                if error.is_none()
                    && let Err(value) = visit_program_dependency(key, id)
                {
                    error = Some(value);
                }
            }
            for (kind, id) in &required_keys {
                if error.is_none()
                    && let Err(value) = visit_required_record(key, *kind, id)
                {
                    error = Some(value);
                }
            }
            if let Some(value) = error {
                callback_error = Some(value);
            }
            if callback_error.is_some() {
                Err(DomainError::HistoricalPrefixMismatch(
                    "M6 target metadata visitor aborted",
                ))
            } else {
                Ok(())
            }
        });
        callback_error.map_or_else(|| result.map_err(M6Error::from), Err)
    }
}

/// The sole authority-free handoff to the later reducer.  It borrows both
/// tails and all accepted phases; callers cannot pass record lists or mint a
/// replacement source topology.
pub(crate) struct IncrementalStalenessInputV5<'a> {
    source: HistoricalPrefixProjectionV4<'a>,
    closure: &'a IncrementalSourceClosureV5,
    mapping: &'a M6MappingPhaseV5,
    correspondence: &'a M6ObligationCorrespondencePhaseV5,
    target: &'a V5PreIncrementalStructuralPrefixProjection<'a>,
    inventory: HistoricalSourceInventoryV5,
}

#[cfg(test)]
thread_local! {
    static STALENESS_INPUT_MATERIALIZATIONS_V5: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_staleness_input_materializations_v5_for_test() {
    STALENESS_INPUT_MATERIALIZATIONS_V5.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn staleness_input_materializations_v5_for_test() -> usize {
    STALENESS_INPUT_MATERIALIZATIONS_V5.with(std::cell::Cell::get)
}

#[derive(Clone, Copy, Debug, Default)]
struct HistoricalAdmissionReferenceStatsV5 {
    occurrences: usize,
    id_bytes: usize,
}

fn historical_admission_reference_stats_v5(
    source: &HistoricalPrefixAdmissionV4<'_>,
) -> M6Result<HistoricalAdmissionReferenceStatsV5> {
    fn add(stats: &mut HistoricalAdmissionReferenceStatsV5, id: &StableId) -> M6Result<()> {
        stats.occurrences = stats
            .occurrences
            .checked_add(1)
            .ok_or(M6Error::Incomplete {
                operation: "M6 staleness historical reference count",
                limit: MAX_M6_STALENESS_WORKING_BYTES,
                observed: usize::MAX,
            })?;
        stats.id_bytes =
            stats
                .id_bytes
                .checked_add(id.allocated_bytes())
                .ok_or(M6Error::Incomplete {
                    operation: "M6 staleness historical reference ID bytes",
                    limit: MAX_M6_STALENESS_WORKING_BYTES,
                    observed: usize::MAX,
                })?;
        Ok(())
    }

    let inventory = HistoricalAdmissionInventoryV5 { source };
    let mut stats = HistoricalAdmissionReferenceStatsV5::default();
    let mut failed = None;
    source.try_visit_replay_records(|record| {
        let descriptor = HistoricalRecordDescriptorV5::from_admission(record);
        descriptor.visit_closure_records(&inventory, |_, id| {
            if failed.is_none() {
                failed = add(&mut stats, id).err();
            }
        });
        Ok::<(), M6Error>(())
    })?;
    if let Some(error) = failed {
        return Err(error);
    }
    // HPP's synthetic coverage member has one denominator predecessor for
    // each actual universe obligation.  A coverage contributor is a typed
    // historical record key, so no source record can contribute more than
    // once to its unique obligation axis; charge every replay record once as
    // the tight allocation-free upper image without deriving coverage sets.
    for id in source.universe_obligation_ids() {
        add(&mut stats, id)?;
    }
    source.try_visit_replay_records(|record| {
        add(&mut stats, record.id())?;
        Ok::<(), M6Error>(())
    })?;
    Ok(stats)
}

/// Allocation-free combined resident-set admission for the historical reducer.
/// Mapping/correspondence *construction* peaks are intentionally not added:
/// those phases are already sealed.  The source projection's own oracle is
/// charged separately; this function covers only currently retained sealed
/// DTO ownership, target predecessor, and every inventory/topology collection
/// that this constructor can materialize.
fn staleness_input_external_reservation_bytes(
    source: &HistoricalPrefixAdmissionV4<'_>,
    mapping: &M6MappingPhaseV5,
    correspondence: &M6ObligationCorrespondencePhaseV5,
    target: &V5PreIncrementalStructuralPrefixProjection<'_>,
) -> M6Result<usize> {
    fn add(total: usize, value: usize) -> M6Result<usize> {
        total.checked_add(value).ok_or(M6Error::Incomplete {
            operation: "M6 staleness combined working bytes",
            limit: MAX_M6_STALENESS_WORKING_BYTES,
            observed: usize::MAX,
        })
    }
    fn mul(left: usize, right: usize) -> M6Result<usize> {
        left.checked_mul(right).ok_or(M6Error::Incomplete {
            operation: "M6 staleness combined working bytes",
            limit: MAX_M6_STALENESS_WORKING_BYTES,
            observed: usize::MAX,
        })
    }

    let record_count = source.record_count();
    let mut program_id_count = 0_usize;
    let mut program_id_bytes = 0_usize;
    let mut program_id_overflow = false;
    source.program_space().visit_known_ids(|id| {
        program_id_count = program_id_count.checked_add(1).unwrap_or_else(|| {
            program_id_overflow = true;
            usize::MAX
        });
        program_id_bytes = program_id_bytes
            .checked_add(id.allocated_bytes())
            .unwrap_or_else(|| {
                program_id_overflow = true;
                usize::MAX
            });
    });
    if program_id_overflow {
        return Err(M6Error::Incomplete {
            operation: "M6 staleness Program ID accounting",
            limit: MAX_M6_STALENESS_WORKING_BYTES,
            observed: usize::MAX,
        });
    }
    // The no-allocation pass walks the actual typed predecessor vocabulary,
    // so sparse legal histories reserve their observed references rather
    // than a record-count-times-maximum fiction.
    let reference_stats = historical_admission_reference_stats_v5(source)?;
    let key_slot = std::mem::size_of::<OwnedHistoricalRecordKeyV5>();
    let inventory_bytes = [
        mul(record_count, key_slot)?,
        source.record_id_bytes(),
        // Reverse maps retain one owner key and one referenced key per
        // appearance.  Topology retains the same reference in `edges`,
        // `dependents`, and Kahn's indegree/frontier.  Each is charged with a
        // concrete key slot; dynamic StableId storage is charged below.
        mul(
            reference_stats.occurrences,
            key_slot.checked_mul(5).ok_or(M6Error::Incomplete {
                operation: "M6 staleness inventory slot bytes",
                limit: MAX_M6_STALENESS_WORKING_BYTES,
                observed: usize::MAX,
            })?,
        )?,
        mul(reference_stats.id_bytes, 5)?,
        mul(program_id_count, std::mem::size_of::<StableId>())?,
        program_id_bytes,
        // The identity map, indegree table and ready frontier each own at
        // most one key per historical record.
        mul(
            record_count,
            key_slot.checked_mul(3).ok_or(M6Error::Incomplete {
                operation: "M6 staleness topology slot bytes",
                limit: MAX_M6_STALENESS_WORKING_BYTES,
                observed: usize::MAX,
            })?,
        )?,
    ]
    .into_iter()
    .try_fold(0_usize, add)?;
    let (mapping_retained, correspondence_retained) =
        staleness_mapping_correspondence_retained_bytes(mapping, correspondence)?;
    // Event owns the replay backing type, so it supplies one capacity-aware
    // retained oracle rather than letting this reducer accidentally omit the
    // duplicate plan, registration Vec spare slots, or run/tail scalars.
    let target_retained = target.retained_bytes_for_m6().map_err(M6Error::from)?;
    let total = [
        mapping_retained,
        correspondence_retained,
        target_retained,
        inventory_bytes,
        MAX_M6_CANONICAL_BYTES,
    ]
    .into_iter()
    .try_fold(0_usize, add)?;
    bounded(
        total,
        MAX_M6_STALENESS_WORKING_BYTES,
        "M6 staleness external working bytes",
    )?;
    Ok(total)
}

fn staleness_mapping_correspondence_retained_bytes(
    mapping: &M6MappingPhaseV5,
    correspondence: &M6ObligationCorrespondencePhaseV5,
) -> M6Result<(usize, usize)> {
    fn add(total: usize, value: usize) -> M6Result<usize> {
        total.checked_add(value).ok_or(M6Error::Incomplete {
            operation: "M6 staleness phase retained bytes",
            limit: MAX_M6_STALENESS_WORKING_BYTES,
            observed: usize::MAX,
        })
    }
    let mapping_retained = mapping
        .mappings()
        .iter()
        .try_fold(mapping.morphism().allocated_bytes(), |total, item| {
            add(total, item.allocated_bytes())
        })?;
    let mapping_retained = add(
        mapping_retained,
        mapping
            .mappings_capacity()
            .checked_mul(std::mem::size_of::<ProgramMappingV5>())
            .ok_or(M6Error::Incomplete {
                operation: "M6 staleness mapping vector slots",
                limit: MAX_M6_STALENESS_WORKING_BYTES,
                observed: usize::MAX,
            })?,
    )?;
    let correspondence_retained = correspondence.entries().iter().try_fold(
        correspondence.correspondence().allocated_bytes(),
        |total, item| add(total, item.allocated_bytes()),
    )?;
    let correspondence_retained = add(
        correspondence_retained,
        correspondence
            .entries_capacity()
            .checked_mul(std::mem::size_of::<ObligationCorrespondenceEntryV5>())
            .ok_or(M6Error::Incomplete {
                operation: "M6 staleness correspondence vector slots",
                limit: MAX_M6_STALENESS_WORKING_BYTES,
                observed: usize::MAX,
            })?,
    )?;
    Ok((mapping_retained, correspondence_retained))
}

/// Splits the fixed M6 process budget into the already-live external inputs
/// and the source-only historical projection.  The source projection oracle
/// includes source replay ownership; this function never charges it again.
fn staleness_source_working_limit(
    working_limit: usize,
    source_reservation: usize,
    external_reservation: usize,
) -> M6Result<usize> {
    let source_limit =
        working_limit
            .checked_sub(external_reservation)
            .ok_or(M6Error::Incomplete {
                operation: "M6 staleness combined working bytes",
                limit: working_limit,
                observed: usize::MAX,
            })?;
    let combined_reservation =
        source_reservation
            .checked_add(external_reservation)
            .ok_or(M6Error::Incomplete {
                operation: "M6 staleness combined working bytes",
                limit: working_limit,
                observed: usize::MAX,
            })?;
    bounded(
        combined_reservation,
        working_limit,
        "M6 staleness combined working bytes",
    )?;
    Ok(source_limit)
}

/// Checks the sealed mapping's complete ProgramSpace domains against the two
/// actual pinned spaces.  This runs before HPP materialization; a matching
/// snapshot ID is deliberately insufficient because it does not commit to the
/// accepted fact set.
fn validate_mapping_program_domains(
    mapping: &M6MappingPhaseV5,
    source: &ProgramSpace,
    target: &ProgramSpace,
) -> M6Result<()> {
    let (source_domain_count, source_domain_digest) = source.m6_known_id_domain()?;
    let (target_domain_count, target_domain_digest) = target.m6_known_id_domain()?;
    let expected_source_count =
        usize::try_from(mapping.morphism().source_domain_count()).map_err(|_| {
            M6Error::InvalidHistoricalTopology(
                "mapping source ProgramSpace domain count is not representable",
            )
        })?;
    let expected_target_count =
        usize::try_from(mapping.morphism().target_domain_count()).map_err(|_| {
            M6Error::InvalidHistoricalTopology(
                "mapping target ProgramSpace domain count is not representable",
            )
        })?;
    if source_domain_count != expected_source_count
        || &source_domain_digest != mapping.morphism().source_domain_digest()
        || target_domain_count != expected_target_count
        || &target_domain_digest != mapping.morphism().target_domain_digest()
    {
        return Err(M6Error::InvalidHistoricalTopology(
            "mapping ProgramSpace domain does not equal the pinned source/target facts",
        ));
    }
    Ok(())
}

impl<'a> IncrementalStalenessInputV5<'a> {
    /// Creates the only reducer handoff from exact replay-owned inputs. No
    /// generic record list, raw JSON, or caller-provided provenance set can
    /// replace the pinned source topology.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        source_log: &'a EventLogV4,
        closure: &'a IncrementalSourceClosureV5,
        mapping: &'a M6MappingPhaseV5,
        correspondence: &'a M6ObligationCorrespondencePhaseV5,
        target: &'a V5PreIncrementalStructuralPrefixProjection<'a>,
    ) -> M6Result<Self> {
        Self::new_with_working_limit(
            source_log,
            closure,
            mapping,
            correspondence,
            target,
            MAX_M6_STALENESS_WORKING_BYTES,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new_with_working_limit(
        source_log: &'a EventLogV4,
        closure: &'a IncrementalSourceClosureV5,
        mapping: &'a M6MappingPhaseV5,
        correspondence: &'a M6ObligationCorrespondencePhaseV5,
        target: &'a V5PreIncrementalStructuralPrefixProjection<'a>,
        working_limit: usize,
    ) -> M6Result<Self> {
        // This must remain the first operation: HPP generation, `known_ids`,
        // inventory maps, topology edges and reducer scratch all allocate.
        // The V4 admission and every retained external input are observed only
        // through allocation-free accessors.  The source oracle already
        // includes replay ownership, so it is added exactly once here and is
        // deliberately excluded from the external reservation below.
        let source_admission = source_log.historical_prefix_admission_v4()?;
        if source_log.tail_hash() != closure.source_tail_hash()
            || source_log.run_id() != &closure.input.source_run_id
            || source_admission.program_space().snapshot_id() != closure.source_snapshot_id()
            || target.tail_hash() != closure.target_predecessor_tail_hash()
            || target.run_id() != &closure.input.target_run_id
            || target.genesis_hash() != &closure.input.target_genesis_hash
            || target.predecessor_offset() != closure.input.target_predecessor_offset
            || target.predecessor_event_count() != closure.input.target_predecessor_event_count
            || target.program_space().snapshot_id() != closure.target_snapshot_id()
            || target.universe().id() != closure.target_universe_id()
        {
            return Err(M6Error::InvalidHistoricalTopology(
                "source/target replay coordinates or snapshot do not equal closure",
            ));
        }
        let accounting = target.accounting();
        if accounting.canonical_prefix_bytes() != target.predecessor_offset()
            || accounting.event_count() != target.predecessor_event_count()
            || accounting.retained_envelope_bytes() == 0
        {
            return Err(M6Error::InvalidHistoricalTopology(
                "target structural replay accounting does not bind its predecessor coordinates",
            ));
        }
        if mapping.morphism().source_closure_id() != closure.id()
            || mapping.morphism().source_snapshot_id() != closure.source_snapshot_id()
            || mapping.morphism().target_snapshot_id() != closure.target_snapshot_id()
            || correspondence.correspondence().morphism_id() != mapping.morphism().id()
            || correspondence.correspondence().source_universe_id() != closure.source_universe_id()
            || correspondence.correspondence().target_universe_id() != closure.target_universe_id()
        {
            return Err(M6Error::InvalidHistoricalTopology(
                "mapping/correspondence phase is not bound to the closure",
            ));
        }
        // Snapshot IDs alone are not a ProgramSpace domain binding: two
        // independently admitted extractions can legitimately carry the same
        // snapshot ID while exposing different accepted fact sets.  Recompute
        // the exact sorted fact-ID domain in a streaming, allocation-free pass
        // before asking the source log to materialize HPP.
        validate_mapping_program_domains(
            mapping,
            source_admission.program_space(),
            target.program_space(),
        )?;
        let external_reservation = staleness_input_external_reservation_bytes(
            &source_admission,
            mapping,
            correspondence,
            target,
        )?;
        let source_limit = staleness_source_working_limit(
            working_limit,
            source_admission.working_reservation_bytes(),
            external_reservation,
        )?;
        let source = source_log.historical_prefix_projection_v4_with_limit(
            u64::try_from(source_limit).map_err(|_| M6Error::Incomplete {
                operation: "M6 staleness source working limit",
                limit: working_limit,
                observed: usize::MAX,
            })?,
        )?;
        #[cfg(test)]
        STALENESS_INPUT_MATERIALIZATIONS_V5.with(|count| count.set(count.get() + 1));
        let inventory = HistoricalSourceInventoryV5::new(&source, source.program_space())?;
        Self::validate_topology(&source, &inventory)?;
        Ok(Self {
            source,
            closure,
            mapping,
            correspondence,
            target,
            inventory,
        })
    }

    fn validate_topology(
        source: &HistoricalPrefixProjectionV4<'_>,
        inventory: &HistoricalSourceInventoryV5,
    ) -> M6Result<()> {
        let mut edges =
            BTreeMap::<OwnedHistoricalRecordKeyV5, BTreeSet<OwnedHistoricalRecordKeyV5>>::new();
        source.try_visit_records(|record| {
            let descriptor = HistoricalRecordDescriptorV5::from_projection(record);
            let key = OwnedHistoricalRecordKeyV5 {
                kind: descriptor.key().kind,
                id: descriptor.key().id.clone(),
            };
            let mut dependencies = BTreeSet::new();
            let mut self_edge = false;
            descriptor.visit_required_records(inventory, |kind, id| {
                let dependency = OwnedHistoricalRecordKeyV5 {
                    kind,
                    id: id.clone(),
                };
                if dependency == key {
                    // Ownership backreferences are omitted in the individual
                    // record arms. Any remaining self reference is an actual
                    // malformed predecessor, never a harmless duplicate.
                    self_edge = true;
                    return;
                }
                dependencies.insert(dependency);
            });
            if self_edge {
                return Err(M6Error::InvalidHistoricalTopology(
                    "historical required-record graph contains a self edge",
                ));
            }
            if edges.insert(key, dependencies).is_some() {
                return Err(M6Error::InvalidHistoricalTopology(
                    "duplicate source record key",
                ));
            }
            Ok(())
        })?;
        for dependencies in edges.values() {
            if dependencies.iter().any(|key| !edges.contains_key(key)) {
                return Err(M6Error::InvalidHistoricalTopology(
                    "required source record is external to the pinned inventory",
                ));
            }
        }
        let mut indegree = edges
            .iter()
            .map(|(key, dependencies)| (key.clone(), dependencies.len()))
            .collect::<BTreeMap<_, _>>();
        let mut dependents =
            BTreeMap::<OwnedHistoricalRecordKeyV5, BTreeSet<OwnedHistoricalRecordKeyV5>>::new();
        for (record, dependencies) in &edges {
            for dependency in dependencies {
                dependents
                    .entry(dependency.clone())
                    .or_default()
                    .insert(record.clone());
            }
        }
        let mut ready = indegree
            .iter()
            .filter_map(|(key, count)| (*count == 0).then_some(key.clone()))
            .collect::<BTreeSet<_>>();
        let mut visited = 0_usize;
        while let Some(key) = ready.pop_first() {
            visited += 1;
            for dependent in dependents.get(&key).into_iter().flatten() {
                let count =
                    indegree
                        .get_mut(dependent)
                        .ok_or(M6Error::InvalidHistoricalTopology(
                            "dependent edge has no source owner",
                        ))?;
                *count = count
                    .checked_sub(1)
                    .ok_or(M6Error::InvalidHistoricalTopology(
                        "historical dependency indegree underflow",
                    ))?;
                if *count == 0 {
                    ready.insert(dependent.clone());
                }
            }
        }
        if visited != edges.len() {
            return Err(M6Error::InvalidHistoricalTopology(
                "historical required-record graph contains a cycle",
            ));
        }
        Ok(())
    }

    pub(crate) fn visit_source_descriptors(
        &self,
        mut visitor: impl for<'b> FnMut(HistoricalRecordDescriptorV5<'b>) -> M6Result<()>,
    ) -> M6Result<()> {
        self.source.try_visit_records(|record| {
            visitor(HistoricalRecordDescriptorV5::from_projection(record))
        })
    }
}

/// Complete in-memory correspondence phase. Event/Store persistence is owned
/// by a later slice; this value grants no append or acceptance authority.
#[derive(Clone, Debug)]
pub struct M6ObligationCorrespondencePhaseV5 {
    entries: Vec<ObligationCorrespondenceEntryV5>,
    correspondence: ObligationCorrespondenceV5,
    working_peak_upper_bound_bytes: usize,
}

impl M6ObligationCorrespondencePhaseV5 {
    #[must_use]
    pub fn entries(&self) -> &[ObligationCorrespondenceEntryV5] {
        &self.entries
    }
    fn entries_capacity(&self) -> usize {
        self.entries.capacity()
    }
    #[must_use]
    pub fn correspondence(&self) -> &ObligationCorrespondenceV5 {
        &self.correspondence
    }
    #[must_use]
    pub fn working_peak_upper_bound_bytes(&self) -> usize {
        self.working_peak_upper_bound_bytes
    }

    #[doc(hidden)]
    pub fn validate_replayed_canonical(
        &self,
        entry_bytes: &[Vec<u8>],
        correspondence_bytes: &[u8],
    ) -> M6Result<()> {
        if entry_bytes.len() != self.entries.len() {
            return Err(M6Error::InvalidWire(
                "replayed correspondence entry count differs from recomputed phase".to_owned(),
            ));
        }
        for (bytes, expected) in entry_bytes.iter().zip(&self.entries) {
            if ObligationCorrespondenceEntryV5::from_json_bytes(bytes)? != *expected {
                return Err(M6Error::InvalidWire(
                    "replayed correspondence entry differs from recomputed phase".to_owned(),
                ));
            }
        }
        if ObligationCorrespondenceV5::from_json_bytes(correspondence_bytes, &self.correspondence)?
            != self.correspondence
        {
            return Err(M6Error::InvalidWire(
                "replayed correspondence seal differs from recomputed phase".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ObligationCandidateKeyV5 {
    rule_id: String,
    property_id: String,
    property_version: String,
    target_kind: String,
    semantic_key: String,
    target_ids: BTreeSet<StableId>,
    context_ids: BTreeSet<StableId>,
    generator_program_ids: BTreeSet<StableId>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ObligationSideV5 {
    Source,
    Target,
}

#[derive(Clone, Debug)]
struct ObligationComponentSeedV5 {
    from_ids: BTreeSet<StableId>,
    to_ids: BTreeSet<StableId>,
    stage: usize,
}

impl ObligationComponentSeedV5 {
    fn allocated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            .saturating_add(id_set_heap(&self.from_ids))
            .saturating_add(id_set_heap(&self.to_ids))
    }
}

fn id_obligation_ref_map_heap(values: &BTreeMap<StableId, &Obligation>) -> usize {
    values
        .len()
        .saturating_mul(std::mem::size_of::<(StableId, &Obligation)>())
        .saturating_add(values.keys().map(StableId::allocated_bytes).sum::<usize>())
}

fn id_mapping_ref_map_heap(values: &BTreeMap<StableId, &ProgramMappingV5>) -> usize {
    values
        .len()
        .saturating_mul(std::mem::size_of::<(StableId, &ProgramMappingV5)>())
        .saturating_add(values.keys().map(StableId::allocated_bytes).sum::<usize>())
}

fn id_id_map_heap(values: &BTreeMap<StableId, StableId>) -> usize {
    values
        .len()
        .saturating_mul(std::mem::size_of::<(StableId, StableId)>())
        .saturating_add(
            values
                .iter()
                .map(|(left, right)| {
                    left.allocated_bytes()
                        .saturating_add(right.allocated_bytes())
                })
                .sum::<usize>(),
        )
}

fn obligation_entry_owner_map_heap(
    values: &BTreeMap<StableId, (usize, StableId, MappingStatusV5)>,
) -> usize {
    values
        .len()
        .saturating_mul(std::mem::size_of::<(
            StableId,
            (usize, StableId, MappingStatusV5),
        )>())
        .saturating_add(
            values
                .iter()
                .map(|(id, (_, entry_id, _))| {
                    id.allocated_bytes()
                        .saturating_add(entry_id.allocated_bytes())
                })
                .sum::<usize>(),
        )
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case", tag = "reference_kind", content = "id")]
enum NormalizedObligationReferenceV5 {
    TargetProgram(StableId),
    UnmappedSourceProgram(StableId),
    TargetObligation(StableId),
    UnmappedSourceObligation(StableId),
}

#[derive(Serialize)]
struct NormalizedObligationBodyV5<'a> {
    target_kind: &'a str,
    target_refs: BTreeSet<NormalizedObligationReferenceV5>,
    semantic_key: String,
    property_id: &'a str,
    property_version: &'a str,
    context_ids: BTreeSet<NormalizedObligationReferenceV5>,
    required_capabilities: &'a BTreeSet<String>,
    evidence_required: bool,
    accepted_evidence_modes: &'a BTreeSet<String>,
    applicability_status: &'a str,
    applicability_reasons: &'a BTreeSet<String>,
    qualification_ids: BTreeSet<NormalizedObligationReferenceV5>,
    weight: f64,
    version_profile: &'a str,
    version_rule: &'a str,
    version_extractor_set: &'a ContentHash,
    version_snapshot: NormalizedObligationReferenceV5,
    depends_on: Vec<NormalizedObligationReferenceV5>,
    generator_ids: BTreeSet<NormalizedObligationReferenceV5>,
    source_ids: BTreeSet<NormalizedObligationReferenceV5>,
}

/// Immutable authority body used only to prove that an accepted aggregate
/// still contains the exact obligation definition independently synthesized
/// from its accepted ProgramSpace.  Ordered fields deliberately remain
/// ordered here: path target order is part of obligation identity and must not
/// be laundered through a normalized set before cross-snapshot comparison.
#[derive(Serialize)]
struct AcceptedObligationDefinitionV5<'a> {
    id: &'a StableId,
    target_kind: &'a str,
    target_refs: &'a [StableId],
    normalized_target_refs: &'a BTreeSet<StableId>,
    semantic_key: &'a str,
    property_id: &'a str,
    property_version: &'a str,
    context_ids: &'a [StableId],
    normalized_context_ids: &'a BTreeSet<StableId>,
    required_capabilities: &'a BTreeSet<String>,
    evidence_required: bool,
    accepted_evidence_modes: &'a BTreeSet<String>,
    applicability_status: &'a str,
    applicability_reasons: &'a BTreeSet<String>,
    qualification_ids: &'a BTreeSet<StableId>,
    weight: f64,
    version: &'a crate::VersionTuple,
    depends_on: &'a [StableId],
    normalized_depends_on: &'a BTreeSet<StableId>,
    generator_ids: &'a BTreeSet<StableId>,
    source_ids: &'a [StableId],
    normalized_source_ids: &'a BTreeSet<StableId>,
}

fn accepted_obligation_definition(obligation: &Obligation) -> AcceptedObligationDefinitionV5<'_> {
    AcceptedObligationDefinitionV5 {
        id: obligation.id(),
        target_kind: obligation.target_kind(),
        target_refs: obligation.target_refs(),
        normalized_target_refs: obligation.normalized_target_refs(),
        semantic_key: obligation.semantic_key(),
        property_id: obligation.property_id(),
        property_version: obligation.property_version(),
        context_ids: obligation.context_ids(),
        normalized_context_ids: obligation.normalized_context_ids(),
        required_capabilities: obligation.required_capabilities(),
        evidence_required: obligation.evidence_required(),
        accepted_evidence_modes: obligation.accepted_evidence_modes(),
        applicability_status: obligation.applicability_status(),
        applicability_reasons: obligation.applicability_reasons(),
        qualification_ids: obligation.qualification_ids(),
        weight: obligation.weight(),
        version: obligation.version(),
        depends_on: obligation.depends_on(),
        normalized_depends_on: obligation.normalized_depends_on(),
        generator_ids: obligation.generator_ids(),
        source_ids: obligation.source_ids(),
        normalized_source_ids: obligation.normalized_source_ids(),
    }
}

fn obligation_program_ids(
    obligation: &Obligation,
    program_domain: &BTreeSet<StableId>,
) -> BTreeSet<StableId> {
    obligation
        .normalized_target_refs()
        .iter()
        .chain(obligation.normalized_source_ids())
        .chain(obligation.qualification_ids())
        .chain(obligation.normalized_context_ids())
        .chain(obligation.generator_ids())
        .filter(|id| program_domain.contains(*id))
        .cloned()
        .chain(std::iter::once(obligation.version().snapshot().clone()))
        .collect()
}

fn validate_obligation_dependency_dag(
    obligations: &BTreeMap<StableId, &Obligation>,
) -> M6Result<BTreeMap<StableId, usize>> {
    let mut indegree = BTreeMap::new();
    let mut dependents = BTreeMap::<StableId, BTreeSet<StableId>>::new();
    let mut depths = obligations
        .keys()
        .cloned()
        .map(|id| (id, 0_usize))
        .collect::<BTreeMap<_, _>>();
    for (id, obligation) in obligations {
        for dependency in obligation.normalized_depends_on() {
            if !obligations.contains_key(dependency) {
                return Err(M6Error::InvalidObligationUniverse(
                    "obligation dependency is outside its accepted universe",
                ));
            }
            dependents
                .entry(dependency.clone())
                .or_default()
                .insert(id.clone());
        }
        indegree.insert(id.clone(), obligation.normalized_depends_on().len());
    }
    let mut ready = indegree
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| id.clone())
        .collect::<BTreeSet<_>>();
    let mut visited = 0_usize;
    while let Some(id) = ready.pop_first() {
        visited = visited.checked_add(1).ok_or(M6Error::Incomplete {
            operation: "M6 obligation topological count",
            limit: MAX_M6_OBLIGATIONS_PER_UNIVERSE,
            observed: usize::MAX,
        })?;
        let next_depth = depths[&id].checked_add(1).ok_or(M6Error::Incomplete {
            operation: "M6 obligation dependency depth",
            limit: usize::MAX,
            observed: usize::MAX,
        })?;
        for dependent in dependents.get(&id).into_iter().flatten() {
            depths
                .entry(dependent.clone())
                .and_modify(|depth| *depth = (*depth).max(next_depth));
            let count = indegree.get_mut(dependent).unwrap();
            *count -= 1;
            if *count == 0 {
                ready.insert(dependent.clone());
            }
        }
    }
    if visited != obligations.len() {
        return Err(M6Error::InvalidObligationUniverse(
            "obligation dependency graph contains a cycle",
        ));
    }
    Ok(depths)
}

fn mapped_candidate_program_set(
    ids: impl IntoIterator<Item = StableId>,
    side: ObligationSideV5,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> Option<BTreeSet<StableId>> {
    let ids = ids.into_iter().collect::<BTreeSet<_>>();
    match side {
        ObligationSideV5::Target => Some(ids),
        ObligationSideV5::Source => {
            let mut mapped = BTreeSet::new();
            for id in ids {
                let targets = successors.get(&id)?;
                if targets.is_empty() {
                    return None;
                }
                mapped.extend(targets.iter().cloned());
            }
            Some(mapped)
        }
    }
}

fn obligation_candidate_key(
    obligation: &Obligation,
    side: ObligationSideV5,
    program_domain: &BTreeSet<StableId>,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> Option<ObligationCandidateKeyV5> {
    let generator_program_ids = obligation
        .generator_ids()
        .iter()
        .filter(|id| program_domain.contains(*id))
        .cloned();
    Some(ObligationCandidateKeyV5 {
        rule_id: obligation.version().rule().to_owned(),
        property_id: obligation.property_id().to_owned(),
        property_version: obligation.property_version().to_owned(),
        target_kind: obligation.target_kind().to_owned(),
        semantic_key: normalized_obligation_semantic_key(obligation, side, successors),
        target_ids: mapped_candidate_program_set(
            obligation.normalized_target_refs().iter().cloned(),
            side,
            successors,
        )?,
        context_ids: mapped_candidate_program_set(
            obligation.normalized_context_ids().iter().cloned(),
            side,
            successors,
        )?,
        generator_program_ids: mapped_candidate_program_set(
            generator_program_ids,
            side,
            successors,
        )?,
    })
}

fn normalized_obligation_semantic_key(
    obligation: &Obligation,
    side: ObligationSideV5,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> String {
    let replacement = match side {
        ObligationSideV5::Target => None,
        ObligationSideV5::Source => successors
            .get(obligation.version().snapshot())
            .filter(|targets| targets.len() == 1)
            .and_then(|targets| targets.iter().next()),
    };
    obligation
        .semantic_key()
        .split('|')
        .map(|part| {
            if part == obligation.version().snapshot().as_str() {
                replacement.map_or(part, StableId::as_str)
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn collapse_and_stage_obligation_components(
    seeds: Vec<ObligationComponentSeedV5>,
    source: &BTreeMap<StableId, &Obligation>,
    target: &BTreeMap<StableId, &Obligation>,
) -> M6Result<Vec<ObligationComponentSeedV5>> {
    let source_owner = seeds
        .iter()
        .enumerate()
        .flat_map(|(index, seed)| seed.from_ids.iter().cloned().map(move |id| (id, index)))
        .collect::<BTreeMap<_, _>>();
    let target_owner = seeds
        .iter()
        .enumerate()
        .flat_map(|(index, seed)| seed.to_ids.iter().cloned().map(move |id| (id, index)))
        .collect::<BTreeMap<_, _>>();
    let mut dependencies = vec![BTreeSet::new(); seeds.len()];
    for (index, seed) in seeds.iter().enumerate() {
        for (ids, obligations, owners) in [
            (&seed.from_ids, source, &source_owner),
            (&seed.to_ids, target, &target_owner),
        ] {
            for id in ids {
                for dependency in obligations[id].normalized_depends_on() {
                    let owner = owners[dependency];
                    if owner != index {
                        dependencies[index].insert(owner);
                    }
                }
            }
        }
    }

    struct Tarjan<'a> {
        edges: &'a [BTreeSet<usize>],
        next: usize,
        indices: Vec<Option<usize>>,
        lowlink: Vec<usize>,
        stack: Vec<usize>,
        on_stack: Vec<bool>,
        components: Vec<Vec<usize>>,
    }
    impl Tarjan<'_> {
        fn visit(&mut self, vertex: usize) {
            let index = self.next;
            self.next += 1;
            self.indices[vertex] = Some(index);
            self.lowlink[vertex] = index;
            self.stack.push(vertex);
            self.on_stack[vertex] = true;
            for next in &self.edges[vertex] {
                if self.indices[*next].is_none() {
                    self.visit(*next);
                    self.lowlink[vertex] = self.lowlink[vertex].min(self.lowlink[*next]);
                } else if self.on_stack[*next] {
                    self.lowlink[vertex] = self.lowlink[vertex].min(self.indices[*next].unwrap());
                }
            }
            if self.lowlink[vertex] == self.indices[vertex].unwrap() {
                let mut component = Vec::new();
                loop {
                    let member = self.stack.pop().unwrap();
                    self.on_stack[member] = false;
                    component.push(member);
                    if member == vertex {
                        break;
                    }
                }
                component.sort_unstable();
                self.components.push(component);
            }
        }
    }
    let mut tarjan = Tarjan {
        edges: &dependencies,
        next: 0,
        indices: vec![None; seeds.len()],
        lowlink: vec![0; seeds.len()],
        stack: Vec::new(),
        on_stack: vec![false; seeds.len()],
        components: Vec::new(),
    };
    for index in 0..seeds.len() {
        if tarjan.indices[index].is_none() {
            tarjan.visit(index);
        }
    }
    tarjan
        .components
        .sort_by_key(|component| component.first().copied());
    let membership = tarjan
        .components
        .iter()
        .enumerate()
        .flat_map(|(component, members)| {
            members
                .iter()
                .copied()
                .map(move |member| (member, component))
        })
        .collect::<BTreeMap<_, _>>();
    let mut collapsed = Vec::with_capacity(tarjan.components.len());
    for members in &tarjan.components {
        let from_ids = members
            .iter()
            .flat_map(|index| seeds[*index].from_ids.iter().cloned())
            .collect::<BTreeSet<_>>();
        let to_ids = members
            .iter()
            .flat_map(|index| seeds[*index].to_ids.iter().cloned())
            .collect::<BTreeSet<_>>();
        bounded(
            from_ids.len(),
            MAX_M6_CORRESPONDENCE_SIDE_IDS,
            "M6 grouped correspondence source obligations",
        )?;
        bounded(
            to_ids.len(),
            MAX_M6_CORRESPONDENCE_SIDE_IDS,
            "M6 grouped correspondence target obligations",
        )?;
        collapsed.push(ObligationComponentSeedV5 {
            from_ids,
            to_ids,
            stage: 0,
        });
    }
    let condensed = tarjan
        .components
        .iter()
        .enumerate()
        .map(|(component, members)| {
            members
                .iter()
                .flat_map(|member| dependencies[*member].iter())
                .map(|dependency| membership[dependency])
                .filter(|dependency| *dependency != component)
                .collect::<BTreeSet<_>>()
        })
        .collect::<Vec<_>>();
    fn depth(
        index: usize,
        dependencies: &[BTreeSet<usize>],
        memo: &mut [Option<usize>],
    ) -> M6Result<usize> {
        if let Some(value) = memo[index] {
            return Ok(value);
        }
        let mut value = 0_usize;
        for dependency in &dependencies[index] {
            value = value.max(
                depth(*dependency, dependencies, memo)?
                    .checked_add(1)
                    .ok_or(M6Error::Incomplete {
                        operation: "M6 correspondence dependency depth",
                        limit: usize::MAX,
                        observed: usize::MAX,
                    })?,
            );
        }
        memo[index] = Some(value);
        Ok(value)
    }
    let mut memo = vec![None; collapsed.len()];
    for (index, seed) in collapsed.iter_mut().enumerate() {
        seed.stage = depth(index, &condensed, &mut memo)?;
    }
    collapsed.sort_by(|left, right| {
        left.stage
            .cmp(&right.stage)
            .then_with(|| {
                left.from_ids
                    .iter()
                    .next()
                    .cmp(&right.from_ids.iter().next())
            })
            .then_with(|| left.to_ids.iter().next().cmp(&right.to_ids.iter().next()))
    });
    Ok(collapsed)
}

fn normalized_program_reference(
    id: &StableId,
    side: ObligationSideV5,
    preserved_successors: &BTreeMap<StableId, StableId>,
) -> NormalizedObligationReferenceV5 {
    match side {
        ObligationSideV5::Target => NormalizedObligationReferenceV5::TargetProgram(id.clone()),
        ObligationSideV5::Source => preserved_successors.get(id).map_or_else(
            || NormalizedObligationReferenceV5::UnmappedSourceProgram(id.clone()),
            |target| NormalizedObligationReferenceV5::TargetProgram(target.clone()),
        ),
    }
}

fn normalized_obligation_reference(
    id: &StableId,
    side: ObligationSideV5,
    preserved_successors: &BTreeMap<StableId, StableId>,
) -> NormalizedObligationReferenceV5 {
    match side {
        ObligationSideV5::Target => NormalizedObligationReferenceV5::TargetObligation(id.clone()),
        ObligationSideV5::Source => preserved_successors.get(id).map_or_else(
            || NormalizedObligationReferenceV5::UnmappedSourceObligation(id.clone()),
            |target| NormalizedObligationReferenceV5::TargetObligation(target.clone()),
        ),
    }
}

fn normalized_obligation_body_hash(
    obligation: &Obligation,
    side: ObligationSideV5,
    program_domain: &BTreeSet<StableId>,
    obligation_domain: &BTreeSet<StableId>,
    preserved_program_successors: &BTreeMap<StableId, StableId>,
    preserved_obligation_successors: &BTreeMap<StableId, StableId>,
) -> M6Result<ContentHash> {
    let normalize = |id: &StableId| {
        if program_domain.contains(id) {
            normalized_program_reference(id, side, preserved_program_successors)
        } else if obligation_domain.contains(id) {
            normalized_obligation_reference(id, side, preserved_obligation_successors)
        } else {
            NormalizedObligationReferenceV5::UnmappedSourceObligation(id.clone())
        }
    };
    body_hash(&NormalizedObligationBodyV5 {
        target_kind: obligation.target_kind(),
        target_refs: obligation
            .normalized_target_refs()
            .iter()
            .map(&normalize)
            .collect(),
        semantic_key: normalized_obligation_semantic_key(
            obligation,
            side,
            &preserved_program_successors
                .iter()
                .map(|(source, target)| (source.clone(), BTreeSet::from([target.clone()])))
                .collect(),
        ),
        property_id: obligation.property_id(),
        property_version: obligation.property_version(),
        context_ids: obligation
            .normalized_context_ids()
            .iter()
            .map(&normalize)
            .collect(),
        required_capabilities: obligation.required_capabilities(),
        evidence_required: obligation.evidence_required(),
        accepted_evidence_modes: obligation.accepted_evidence_modes(),
        applicability_status: obligation.applicability_status(),
        applicability_reasons: obligation.applicability_reasons(),
        qualification_ids: obligation
            .qualification_ids()
            .iter()
            .map(&normalize)
            .collect(),
        weight: obligation.weight(),
        version_profile: obligation.version().profile(),
        version_rule: obligation.version().rule(),
        version_extractor_set: obligation.version().extractor_set(),
        version_snapshot: normalize(obligation.version().snapshot()),
        depends_on: obligation.depends_on().iter().map(&normalize).collect(),
        generator_ids: obligation.generator_ids().iter().map(&normalize).collect(),
        source_ids: obligation
            .normalized_source_ids()
            .iter()
            .map(&normalize)
            .collect(),
    })
}

fn validate_resynthesized_accepted_universe(aggregate: &ReviewAggregate) -> M6Result<()> {
    let expected = crate::MvpRulePack::synthesize(aggregate.program()).map_err(|_| {
        M6Error::InvalidObligationUniverse(
            "accepted ProgramSpace cannot reproduce its obligation universe",
        )
    })?;
    if expected.universe() != aggregate.universe() {
        return Err(M6Error::InvalidObligationUniverse(
            "accepted universe differs from deterministic rule-pack synthesis",
        ));
    }
    let actual = aggregate
        .obligations()
        .map(|obligation| (obligation.id().clone(), obligation))
        .collect::<BTreeMap<_, _>>();
    let expected = expected
        .obligations()
        .iter()
        .map(|obligation| (obligation.id().clone(), obligation))
        .collect::<BTreeMap<_, _>>();
    if actual.keys().ne(expected.keys()) {
        return Err(M6Error::InvalidObligationUniverse(
            "accepted obligation domain differs from deterministic rule-pack synthesis",
        ));
    }
    for id in actual.keys() {
        let actual_definition = accepted_obligation_definition(actual[id]);
        let expected_definition = accepted_obligation_definition(expected[id]);
        bounded_serialized(
            &actual_definition,
            MAX_M6_CANONICAL_BYTES,
            "M6 accepted obligation definition bytes",
        )?;
        bounded_serialized(
            &expected_definition,
            MAX_M6_CANONICAL_BYTES,
            "M6 synthesized obligation definition bytes",
        )?;
        if crate::canonical_json(&actual_definition)?
            != crate::canonical_json(&expected_definition)?
        {
            return Err(M6Error::InvalidObligationUniverse(
                "accepted obligation body differs from deterministic rule-pack synthesis",
            ));
        }
    }
    Ok(())
}

fn checked_correspondence_working_add(total: usize, addition: usize) -> M6Result<usize> {
    let observed = total.checked_add(addition).ok_or(M6Error::Incomplete {
        operation: "M6 correspondence retained working bytes",
        limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
        observed: usize::MAX,
    })?;
    bounded(
        observed,
        MAX_M6_CORRESPONDENCE_WORKING_BYTES,
        "M6 correspondence retained working bytes",
    )?;
    Ok(observed)
}

fn one_to_one_obligation_status(
    program_refs_are_unique: bool,
    dependency_refs_are_unique: bool,
    source_body_hash: &ContentHash,
    target_body_hash: &ContentHash,
) -> MappingStatusV5 {
    if !program_refs_are_unique || !dependency_refs_are_unique {
        MappingStatusV5::Unresolved
    } else if source_body_hash == target_body_hash {
        MappingStatusV5::Preserved
    } else {
        MappingStatusV5::Modified
    }
}

fn checked_correspondence_working_mul(value: usize, multiplier: usize) -> M6Result<usize> {
    let observed = value.checked_mul(multiplier).ok_or(M6Error::Incomplete {
        operation: "M6 correspondence reservation arithmetic",
        limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
        observed: usize::MAX,
    })?;
    bounded(
        observed,
        MAX_M6_CORRESPONDENCE_WORKING_BYTES,
        "M6 correspondence reservation arithmetic",
    )?;
    Ok(observed)
}

/// Deterministic allocation oracle evaluated before any correspondence-owned
/// map, domain, candidate, component, entry, or seal is constructed.  Each
/// field names a simultaneously chargeable ownership class rather than hiding
/// allocations behind one post-hoc multiplier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CorrespondenceAllocationOracleV5 {
    accepted_aggregate_bytes: usize,
    aggregate_validation_scratch_bytes: usize,
    mapping_phase_bytes: usize,
    obligation_domain_bytes: usize,
    owner_successor_bytes: usize,
    normalized_key_body_bytes: usize,
    dependency_component_bytes: usize,
    entry_seal_bytes: usize,
    serialization_scratch_bytes: usize,
}

impl CorrespondenceAllocationOracleV5 {
    fn reservation_bytes(self) -> M6Result<usize> {
        [
            self.accepted_aggregate_bytes,
            self.aggregate_validation_scratch_bytes,
            self.mapping_phase_bytes,
            self.obligation_domain_bytes,
            self.owner_successor_bytes,
            self.normalized_key_body_bytes,
            self.dependency_component_bytes,
            self.entry_seal_bytes,
            self.serialization_scratch_bytes,
        ]
        .into_iter()
        .try_fold(0_usize, checked_correspondence_working_add)
    }
}

fn correspondence_allocation_oracle(
    mapping_phase: &M6MappingPhaseV5,
    source: &ReviewAggregate,
    target: &ReviewAggregate,
) -> M6Result<CorrespondenceAllocationOracleV5> {
    let retained = |aggregate: &ReviewAggregate| {
        usize::try_from(aggregate.retained_bytes_v3()?).map_err(|_| M6Error::Incomplete {
            operation: "M6 correspondence accepted aggregate bytes",
            limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
            observed: usize::MAX,
        })
    };
    let accepted_aggregate_bytes =
        checked_correspondence_working_add(retained(source)?, retained(target)?)?;
    // ReviewAggregate::validate materializes obligation_ids, program_ids and
    // all_ids, and can retain an additional evidence/decision/key projection
    // while checking later records. Six full retained-aggregate ownership
    // copies are a deterministic allocation-free upper bound over those ID
    // clones and portable B-tree slots; it is charged separately because the
    // validation happens before correspondence collections exist.
    let aggregate_validation_scratch_bytes =
        checked_correspondence_working_mul(accepted_aggregate_bytes, 6)?;

    let mapping_records_bytes = mapping_phase.mappings().iter().try_fold(
        mapping_phase
            .mappings()
            .len()
            .checked_mul(std::mem::size_of::<ProgramMappingV5>())
            .ok_or(M6Error::Incomplete {
                operation: "M6 correspondence mapping phase arithmetic",
                limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
                observed: usize::MAX,
            })?,
        |total, mapping| {
            total
                .checked_add(mapping.allocated_bytes())
                .ok_or(M6Error::Incomplete {
                    operation: "M6 correspondence mapping phase arithmetic",
                    limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
                    observed: usize::MAX,
                })
        },
    )?;
    let mapping_phase_bytes = checked_correspondence_working_add(
        mapping_records_bytes,
        mapping_phase.morphism().allocated_bytes(),
    )?;

    let obligations = source.obligations().chain(target.obligations());
    let mut obligation_count = 0_usize;
    let mut obligation_owned_bytes = 0_usize;
    let mut obligation_id_bytes = 0_usize;
    let mut dependency_edges = 0_usize;
    let mut program_reference_slots = 0_usize;
    let mut max_id_bytes = 0_usize;
    for obligation in obligations {
        obligation_count = obligation_count.checked_add(1).ok_or(M6Error::Incomplete {
            operation: "M6 correspondence obligation count arithmetic",
            limit: MAX_M6_CORRESPONDENCE_ENTRIES,
            observed: usize::MAX,
        })?;
        obligation_owned_bytes = obligation_owned_bytes
            .checked_add(obligation.allocated_bytes())
            .ok_or(M6Error::Incomplete {
                operation: "M6 correspondence obligation ownership arithmetic",
                limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
                observed: usize::MAX,
            })?;
        let id_bytes = obligation.id().allocated_bytes();
        obligation_id_bytes =
            obligation_id_bytes
                .checked_add(id_bytes)
                .ok_or(M6Error::Incomplete {
                    operation: "M6 correspondence obligation ID arithmetic",
                    limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
                    observed: usize::MAX,
                })?;
        max_id_bytes = max_id_bytes.max(id_bytes);
        dependency_edges = dependency_edges
            .checked_add(obligation.normalized_depends_on().len())
            .ok_or(M6Error::Incomplete {
                operation: "M6 correspondence dependency arithmetic",
                limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
                observed: usize::MAX,
            })?;
        program_reference_slots = [
            obligation.normalized_target_refs().len(),
            obligation.normalized_source_ids().len(),
            obligation.normalized_context_ids().len(),
            obligation.qualification_ids().len(),
            obligation.generator_ids().len(),
            1,
        ]
        .into_iter()
        .try_fold(program_reference_slots, usize::checked_add)
        .ok_or(M6Error::Incomplete {
            operation: "M6 correspondence Program reference arithmetic",
            limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
            observed: usize::MAX,
        })?;
    }
    bounded(
        obligation_count,
        MAX_M6_CORRESPONDENCE_ENTRIES,
        "M6 correspondence total obligation count",
    )?;

    for mapping in mapping_phase.mappings() {
        max_id_bytes = max_id_bytes.max(mapping.id().allocated_bytes());
        for id in mapping.from_ids().iter().chain(mapping.to_ids()) {
            max_id_bytes = max_id_bytes.max(id.allocated_bytes());
        }
    }
    let id_slot_bytes = std::mem::size_of::<StableId>()
        .checked_add(max_id_bytes)
        .ok_or(M6Error::Incomplete {
            operation: "M6 correspondence ID slot arithmetic",
            limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
            observed: usize::MAX,
        })?;
    let program_domain_count = usize::try_from(
        mapping_phase
            .morphism()
            .source_domain_count()
            .checked_add(mapping_phase.morphism().target_domain_count())
            .ok_or(M6Error::Incomplete {
                operation: "M6 correspondence Program domain arithmetic",
                limit: MAX_M6_PROGRAM_DOMAIN_IDS * 2,
                observed: usize::MAX,
            })?,
    )
    .map_err(|_| M6Error::Incomplete {
        operation: "M6 correspondence Program domain arithmetic",
        limit: MAX_M6_PROGRAM_DOMAIN_IDS * 2,
        observed: usize::MAX,
    })?;

    // Domain/index charge: two obligation reference maps, two obligation ID
    // sets and both Program domain sets. Dynamic ID storage is charged for
    // every clone in addition to the portable map/set slot contract.
    let domain_slots = obligation_count
        .checked_mul(
            2 * std::mem::size_of::<(StableId, &Obligation)>()
                + 2 * std::mem::size_of::<StableId>(),
        )
        .and_then(|bytes| {
            program_domain_count
                .checked_mul(std::mem::size_of::<StableId>())
                .and_then(|program| bytes.checked_add(program))
        })
        .ok_or(M6Error::Incomplete {
            operation: "M6 correspondence domain reservation arithmetic",
            limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
            observed: usize::MAX,
        })?;
    let obligation_domain_bytes = [
        domain_slots,
        checked_correspondence_working_mul(obligation_id_bytes, 4)?,
        checked_correspondence_working_mul(mapping_phase_bytes, 2)?,
    ]
    .into_iter()
    .try_fold(0_usize, checked_correspondence_working_add)?;

    // Successor/owner maps clone mapping-domain IDs and retain mapping refs.
    // The factor also covers preserved-only projections and both source/target
    // ownership indexes.
    let owner_successor_bytes = [
        checked_correspondence_working_mul(mapping_phase_bytes, 5)?,
        checked_correspondence_working_mul(
            program_domain_count,
            id_slot_bytes.checked_mul(4).ok_or(M6Error::Incomplete {
                operation: "M6 correspondence owner slot arithmetic",
                limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
                observed: usize::MAX,
            })?,
        )?,
    ]
    .into_iter()
    .try_fold(0_usize, checked_correspondence_working_add)?;

    // One candidate key per obligation owns all key strings and mapped
    // target/context/generator sets. One normalized body and canonical buffer
    // may coexist while an entry is materialized.
    let normalized_key_body_bytes = [
        checked_correspondence_working_mul(obligation_owned_bytes, 4)?,
        checked_correspondence_working_mul(
            program_reference_slots,
            id_slot_bytes.checked_mul(2).ok_or(M6Error::Incomplete {
                operation: "M6 correspondence normalized reference slot arithmetic",
                limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
                observed: usize::MAX,
            })?,
        )?,
        MAX_M6_CANONICAL_BYTES,
    ]
    .into_iter()
    .try_fold(0_usize, checked_correspondence_working_add)?;

    // Raw DAG validation and quotient staging can coexist with owner maps,
    // dependency sets, Tarjan frontiers/components, membership and collapsed
    // seeds. Obligation bytes provide a checked upper bound for every cloned
    // dependency ID/string; slot charges cover the index-only vectors/maps.
    let dependency_component_bytes = [
        checked_correspondence_working_mul(obligation_owned_bytes, 8)?,
        checked_correspondence_working_mul(
            dependency_edges,
            id_slot_bytes
                .checked_add(std::mem::size_of::<usize>())
                .and_then(|bytes| bytes.checked_mul(6))
                .ok_or(M6Error::Incomplete {
                    operation: "M6 correspondence dependency slot arithmetic",
                    limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
                    observed: usize::MAX,
                })?,
        )?,
        checked_correspondence_working_mul(
            obligation_count,
            std::mem::size_of::<usize>()
                .checked_mul(24)
                .ok_or(M6Error::Incomplete {
                    operation: "M6 correspondence component slot arithmetic",
                    limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
                    observed: usize::MAX,
                })?,
        )?,
    ]
    .into_iter()
    .try_fold(0_usize, checked_correspondence_working_add)?;

    // Exclusive coverage means from/to and body-hash members total no more
    // than the two obligation domains. Source-mapping/predecessor sets retain
    // at most their declared 64 IDs for every possible component.
    let link_slots = obligation_count
        .checked_mul(
            2_usize
                .checked_mul(MAX_M6_CORRESPONDENCE_PREDECESSOR_IDS)
                .and_then(|count| count.checked_add(6))
                .ok_or(M6Error::Incomplete {
                    operation: "M6 correspondence entry link arithmetic",
                    limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
                    observed: usize::MAX,
                })?,
        )
        .ok_or(M6Error::Incomplete {
            operation: "M6 correspondence entry link arithmetic",
            limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
            observed: usize::MAX,
        })?;
    let entry_seal_bytes = [
        checked_correspondence_working_mul(
            obligation_count,
            std::mem::size_of::<ObligationCorrespondenceEntryV5>(),
        )?,
        checked_correspondence_working_mul(
            link_slots,
            id_slot_bytes
                .checked_add(std::mem::size_of::<ContentHash>())
                .ok_or(M6Error::Incomplete {
                    operation: "M6 correspondence entry slot arithmetic",
                    limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
                    observed: usize::MAX,
                })?,
        )?,
        checked_correspondence_working_mul(obligation_owned_bytes, 4)?,
        checked_correspondence_working_mul(mapping_phase_bytes, 2)?,
        std::mem::size_of::<ObligationCorrespondenceV5>(),
    ]
    .into_iter()
    .try_fold(0_usize, checked_correspondence_working_add)?;

    let serialization_scratch_bytes = checked_correspondence_working_add(
        MAX_M6_CANONICAL_BYTES,
        MAX_M6_CORRESPONDENCE_DTO_BYTES,
    )?;
    Ok(CorrespondenceAllocationOracleV5 {
        accepted_aggregate_bytes,
        aggregate_validation_scratch_bytes,
        mapping_phase_bytes,
        obligation_domain_bytes,
        owner_successor_bytes,
        normalized_key_body_bytes,
        dependency_component_bytes,
        entry_seal_bytes,
        serialization_scratch_bytes,
    })
}

#[cfg(test)]
std::thread_local! {
    static CORRESPONDENCE_AGGREGATE_VALIDATION_CALLS: std::cell::Cell<usize> = const {
        std::cell::Cell::new(0)
    };
}

fn validate_correspondence_aggregate(aggregate: &ReviewAggregate) -> Result<(), DomainError> {
    #[cfg(test)]
    CORRESPONDENCE_AGGREGATE_VALIDATION_CALLS.with(|calls| calls.set(calls.get() + 1));
    aggregate.validate()
}

impl ObligationCorrespondenceV5 {
    /// Computes and admits the complete correspondence reservation before any
    /// correspondence-owned retained collection is built.
    #[doc(hidden)]
    pub fn correspondence_reservation_bytes_from_accepted_universes(
        mapping_phase: &M6MappingPhaseV5,
        source: &ReviewAggregate,
        target: &ReviewAggregate,
    ) -> M6Result<usize> {
        correspondence_allocation_oracle(mapping_phase, source, target)?.reservation_bytes()
    }

    /// Recomputes correspondence only from validated accepted aggregates, the
    /// exact accepted closure, and its already sealed Program mapping phase.
    #[doc(hidden)]
    pub fn derive_from_accepted_universes(
        closure: &IncrementalSourceClosureV5,
        mapping_phase: &M6MappingPhaseV5,
        source: &ReviewAggregate,
        target: &ReviewAggregate,
    ) -> M6Result<M6ObligationCorrespondencePhaseV5> {
        Self::derive_with_working_limit(
            closure,
            mapping_phase,
            source,
            target,
            MAX_M6_CORRESPONDENCE_WORKING_BYTES,
        )
    }

    fn derive_with_working_limit(
        closure: &IncrementalSourceClosureV5,
        mapping_phase: &M6MappingPhaseV5,
        source: &ReviewAggregate,
        target: &ReviewAggregate,
        working_limit: usize,
    ) -> M6Result<M6ObligationCorrespondencePhaseV5> {
        // The allocation-free oracle and its admission are the first
        // operations. In particular ReviewAggregate::validate allocates
        // obligation/program/all-ID scratch and may not run before this gate.
        let allocation_oracle = correspondence_allocation_oracle(mapping_phase, source, target)?;
        let working_peak_upper_bound_bytes = allocation_oracle.reservation_bytes()?;
        bounded(
            working_peak_upper_bound_bytes,
            working_limit,
            "M6 correspondence preflight working bytes",
        )?;
        validate_correspondence_aggregate(source).map_err(|_| {
            M6Error::InvalidObligationUniverse("source aggregate/universe is not valid")
        })?;
        validate_correspondence_aggregate(target).map_err(|_| {
            M6Error::InvalidObligationUniverse("target aggregate/universe is not valid")
        })?;
        validate_resynthesized_accepted_universe(source)?;
        validate_resynthesized_accepted_universe(target)?;
        let morphism = mapping_phase.morphism();
        if morphism.source_closure_id() != closure.id()
            || source.program().repository_id() != target.program().repository_id()
            || source.program().snapshot_id() != morphism.source_snapshot_id()
            || target.program().snapshot_id() != morphism.target_snapshot_id()
            || source.universe().id() != &closure.input.source_universe_id
            || target.universe().id() != &closure.input.target_universe_id
        {
            return Err(M6Error::InvalidObligationUniverse(
                "accepted universes do not bind the closure and morphism snapshots",
            ));
        }
        let source_obligations = source
            .obligations()
            .map(|obligation| (obligation.id().clone(), obligation))
            .collect::<BTreeMap<_, _>>();
        let target_obligations = target
            .obligations()
            .map(|obligation| (obligation.id().clone(), obligation))
            .collect::<BTreeMap<_, _>>();
        bounded(
            source_obligations.len(),
            MAX_M6_OBLIGATIONS_PER_UNIVERSE,
            "M6 source obligations",
        )?;
        bounded(
            target_obligations.len(),
            MAX_M6_OBLIGATIONS_PER_UNIVERSE,
            "M6 target obligations",
        )?;
        if source_obligations.keys().cloned().collect::<BTreeSet<_>>()
            != *source.universe().obligation_ids()
            || target_obligations.keys().cloned().collect::<BTreeSet<_>>()
                != *target.universe().obligation_ids()
        {
            return Err(M6Error::InvalidObligationUniverse(
                "accepted universe denominator does not equal its obligation records",
            ));
        }
        validate_obligation_dependency_dag(&source_obligations)?;
        validate_obligation_dependency_dag(&target_obligations)?;

        let mut program_successors = BTreeMap::<StableId, BTreeSet<StableId>>::new();
        let mut preserved_program_successors = BTreeMap::<StableId, StableId>::new();
        let mut source_mapping_owner = BTreeMap::<StableId, &ProgramMappingV5>::new();
        let mut target_mapping_owner = BTreeMap::<StableId, &ProgramMappingV5>::new();
        for mapping in mapping_phase.mappings() {
            for id in mapping.from_ids() {
                program_successors.insert(id.clone(), mapping.to_ids().clone());
                source_mapping_owner.insert(id.clone(), mapping);
                if (mapping.status() == MappingStatusV5::Preserved
                    || mapping.object_kind() == ProgramObjectKindV5::Snapshot)
                    && mapping.to_ids().len() == 1
                {
                    preserved_program_successors
                        .insert(id.clone(), mapping.to_ids().iter().next().unwrap().clone());
                }
            }
            for id in mapping.to_ids() {
                target_mapping_owner.insert(id.clone(), mapping);
            }
        }
        let source_program_domain = source.program().known_ids();
        let target_program_domain = target.program().known_ids();
        if source_mapping_owner
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>()
            != source_program_domain
            || target_mapping_owner
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>()
                != target_program_domain
            || morphism.source_domain_count() != source_program_domain.len() as u64
            || morphism.target_domain_count() != target_program_domain.len() as u64
            || morphism.source_domain_digest() != &digest_ids(&source_program_domain)?
            || morphism.target_domain_digest() != &digest_ids(&target_program_domain)?
        {
            return Err(M6Error::InvalidObligationUniverse(
                "accepted morphism does not exactly cover both aggregate Program domains",
            ));
        }
        let source_obligation_domain = source_obligations.keys().cloned().collect::<BTreeSet<_>>();
        let target_obligation_domain = target_obligations.keys().cloned().collect::<BTreeSet<_>>();

        let mut source_by_key = BTreeMap::<ObligationCandidateKeyV5, BTreeSet<StableId>>::new();
        let mut target_by_key = BTreeMap::<ObligationCandidateKeyV5, BTreeSet<StableId>>::new();
        let mut unmatched_source = BTreeSet::new();
        for obligation in source_obligations.values() {
            if let Some(key) = obligation_candidate_key(
                obligation,
                ObligationSideV5::Source,
                &source_program_domain,
                &program_successors,
            ) {
                source_by_key
                    .entry(key)
                    .or_default()
                    .insert(obligation.id().clone());
            } else {
                unmatched_source.insert(obligation.id().clone());
            }
        }
        for obligation in target_obligations.values() {
            let key = obligation_candidate_key(
                obligation,
                ObligationSideV5::Target,
                &target_program_domain,
                &program_successors,
            )
            .ok_or(M6Error::InvalidObligationUniverse(
                "target obligation candidate key could not be constructed",
            ))?;
            target_by_key
                .entry(key)
                .or_default()
                .insert(obligation.id().clone());
        }
        let keys = source_by_key
            .keys()
            .chain(target_by_key.keys())
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut seeds = Vec::new();
        for key in keys {
            let from_ids = source_by_key.remove(&key).unwrap_or_default();
            let to_ids = target_by_key.remove(&key).unwrap_or_default();
            if from_ids.is_empty() {
                seeds.extend(to_ids.into_iter().map(|id| ObligationComponentSeedV5 {
                    from_ids: BTreeSet::new(),
                    to_ids: BTreeSet::from([id]),
                    stage: 0,
                }));
            } else if to_ids.is_empty() {
                seeds.extend(from_ids.into_iter().map(|id| ObligationComponentSeedV5 {
                    from_ids: BTreeSet::from([id]),
                    to_ids: BTreeSet::new(),
                    stage: 0,
                }));
            } else {
                seeds.push(ObligationComponentSeedV5 {
                    from_ids,
                    to_ids,
                    stage: 0,
                });
            }
        }
        seeds.extend(
            unmatched_source
                .into_iter()
                .map(|id| ObligationComponentSeedV5 {
                    from_ids: BTreeSet::from([id]),
                    to_ids: BTreeSet::new(),
                    stage: 0,
                }),
        );
        bounded(
            seeds.len(),
            MAX_M6_CORRESPONDENCE_ENTRIES,
            "M6 correspondence entries",
        )?;
        let seeds = collapse_and_stage_obligation_components(
            seeds,
            &source_obligations,
            &target_obligations,
        )?;

        let all_obligation_successors = seeds
            .iter()
            .flat_map(|seed| {
                seed.from_ids
                    .iter()
                    .cloned()
                    .map(move |id| (id, seed.to_ids.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        let mut preserved_obligation_successors = BTreeMap::<StableId, StableId>::new();
        let mut source_entry_owner =
            BTreeMap::<StableId, (usize, StableId, MappingStatusV5)>::new();
        let mut target_entry_owner =
            BTreeMap::<StableId, (usize, StableId, MappingStatusV5)>::new();
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(seeds.len())
            .map_err(|_| M6Error::Incomplete {
                operation: "M6 correspondence entry allocation",
                limit: MAX_M6_CORRESPONDENCE_ENTRIES,
                observed: usize::MAX,
            })?;
        for seed in &seeds {
            let mut predecessor_entry_ids = BTreeSet::new();
            for (side, id) in seed
                .from_ids
                .iter()
                .map(|id| (ObligationSideV5::Source, id))
                .chain(seed.to_ids.iter().map(|id| (ObligationSideV5::Target, id)))
            {
                let obligation = match side {
                    ObligationSideV5::Source => source_obligations[id],
                    ObligationSideV5::Target => target_obligations[id],
                };
                let owners = match side {
                    ObligationSideV5::Source => &source_entry_owner,
                    ObligationSideV5::Target => &target_entry_owner,
                };
                for dependency in obligation.normalized_depends_on() {
                    if let Some((stage, entry_id, _)) = owners.get(dependency)
                        && *stage < seed.stage
                    {
                        predecessor_entry_ids.insert(entry_id.clone());
                    }
                }
            }
            bounded(
                predecessor_entry_ids.len(),
                MAX_M6_CORRESPONDENCE_PREDECESSOR_IDS,
                "M6 correspondence predecessor entries",
            )?;

            let mut source_mapping_ids = BTreeSet::new();
            for id in &seed.from_ids {
                for program_id in
                    obligation_program_ids(source_obligations[id], &source_program_domain)
                {
                    source_mapping_ids.insert(source_mapping_owner[&program_id].id().clone());
                }
            }
            for id in &seed.to_ids {
                for program_id in
                    obligation_program_ids(target_obligations[id], &target_program_domain)
                {
                    source_mapping_ids.insert(target_mapping_owner[&program_id].id().clone());
                }
            }
            bounded(
                source_mapping_ids.len(),
                MAX_M6_CORRESPONDENCE_PREDECESSOR_IDS,
                "M6 correspondence source mappings",
            )?;

            let source_body_hashes = seed
                .from_ids
                .iter()
                .map(|id| {
                    IdBodyHashV5::new(
                        id.clone(),
                        normalized_obligation_body_hash(
                            source_obligations[id],
                            ObligationSideV5::Source,
                            &source_program_domain,
                            &source_obligation_domain,
                            &preserved_program_successors,
                            &preserved_obligation_successors,
                        )?,
                    )
                })
                .collect::<M6Result<Vec<_>>>()?;
            let target_body_hashes = seed
                .to_ids
                .iter()
                .map(|id| {
                    IdBodyHashV5::new(
                        id.clone(),
                        normalized_obligation_body_hash(
                            target_obligations[id],
                            ObligationSideV5::Target,
                            &target_program_domain,
                            &target_obligation_domain,
                            &preserved_program_successors,
                            &preserved_obligation_successors,
                        )?,
                    )
                })
                .collect::<M6Result<Vec<_>>>()?;

            let status = match (seed.from_ids.len(), seed.to_ids.len()) {
                (0, 1) => MappingStatusV5::Added,
                (1, 0) => MappingStatusV5::Removed,
                (1, 1) => {
                    let source_id = seed.from_ids.iter().next().unwrap();
                    let source_obligation = source_obligations[source_id];
                    let program_refs_are_unique =
                        obligation_program_ids(source_obligation, &source_program_domain)
                            .iter()
                            .all(|id| {
                                let mapping = source_mapping_owner[id];
                                mapping.to_ids().len() == 1
                                    && matches!(
                                        mapping.status(),
                                        MappingStatusV5::Preserved | MappingStatusV5::Modified
                                    )
                            });
                    let dependency_refs_are_unique = source_obligation
                        .normalized_depends_on()
                        .iter()
                        .all(|dependency| {
                            all_obligation_successors
                                .get(dependency)
                                .is_some_and(|targets| targets.len() == 1)
                                && source_entry_owner.get(dependency).is_some_and(
                                    |(_, _, status)| {
                                        matches!(
                                            status,
                                            MappingStatusV5::Preserved | MappingStatusV5::Modified
                                        )
                                    },
                                )
                        });
                    one_to_one_obligation_status(
                        program_refs_are_unique,
                        dependency_refs_are_unique,
                        &source_body_hashes[0].body_hash,
                        &target_body_hashes[0].body_hash,
                    )
                }
                (1, _) => MappingStatusV5::Split,
                (_, 1) => MappingStatusV5::Merged,
                _ => MappingStatusV5::Unresolved,
            };
            let entry = ObligationCorrespondenceEntryV5::from_parts(
                ObligationCorrespondenceEntryPartsV5 {
                    morphism_id: morphism.id().clone(),
                    from_obligation_ids: seed.from_ids.clone(),
                    to_obligation_ids: seed.to_ids.clone(),
                    status,
                    source_mapping_ids,
                    predecessor_entry_ids,
                    source_body_hashes,
                    target_body_hashes,
                },
            )?;
            if status == MappingStatusV5::Preserved
                && seed.from_ids.len() == 1
                && seed.to_ids.len() == 1
            {
                preserved_obligation_successors.insert(
                    seed.from_ids.iter().next().unwrap().clone(),
                    seed.to_ids.iter().next().unwrap().clone(),
                );
            }
            for id in &seed.from_ids {
                source_entry_owner.insert(id.clone(), (seed.stage, entry.id.clone(), status));
            }
            for id in &seed.to_ids {
                target_entry_owner.insert(id.clone(), (seed.stage, entry.id.clone(), status));
            }
            entries.push(entry);
        }
        entries.sort_by(|left, right| left.id.cmp(&right.id));
        let correspondence = Self::seal_derived(
            morphism,
            source.universe().id(),
            target.universe().id(),
            &entries,
            &source_obligation_domain,
            &target_obligation_domain,
        )?;
        // Allocation realization at the seal boundary.  Every still-live
        // correspondence collection is charged recursively; transient DAG,
        // candidate, normalized-body and component scratch were separately
        // reserved by the preflight oracle above.  A reservation bug is a
        // typed refusal rather than an unaccounted allocation.
        let realized_bytes = [
            allocation_oracle.accepted_aggregate_bytes,
            allocation_oracle.mapping_phase_bytes,
            id_obligation_ref_map_heap(&source_obligations),
            id_obligation_ref_map_heap(&target_obligations),
            successor_map_heap(&program_successors),
            id_id_map_heap(&preserved_program_successors),
            id_mapping_ref_map_heap(&source_mapping_owner),
            id_mapping_ref_map_heap(&target_mapping_owner),
            id_set_heap(&source_program_domain),
            id_set_heap(&target_program_domain),
            id_set_heap(&source_obligation_domain),
            id_set_heap(&target_obligation_domain),
            seeds
                .iter()
                .map(ObligationComponentSeedV5::allocated_bytes)
                .sum(),
            successor_map_heap(&all_obligation_successors),
            id_id_map_heap(&preserved_obligation_successors),
            obligation_entry_owner_map_heap(&source_entry_owner),
            obligation_entry_owner_map_heap(&target_entry_owner),
            entries
                .iter()
                .map(ObligationCorrespondenceEntryV5::allocated_bytes)
                .sum(),
            correspondence.allocated_bytes(),
            MAX_M6_CANONICAL_BYTES,
            MAX_M6_CORRESPONDENCE_DTO_BYTES,
        ]
        .into_iter()
        .try_fold(0_usize, checked_correspondence_working_add)?;
        if realized_bytes > working_peak_upper_bound_bytes {
            return Err(M6Error::Incomplete {
                operation: "M6 correspondence reservation underflow",
                limit: working_peak_upper_bound_bytes,
                observed: realized_bytes,
            });
        }
        Ok(M6ObligationCorrespondencePhaseV5 {
            entries,
            correspondence,
            working_peak_upper_bound_bytes,
        })
    }
}

#[cfg(test)]
pub(crate) struct M6ProgramFixture {
    program: ProgramSpace,
    source_bytes: BTreeMap<StableId, Vec<u8>>,
}

#[cfg(test)]
impl M6ProgramFixture {
    pub(crate) fn program(&self) -> &ProgramSpace {
        &self.program
    }

    pub(crate) fn source_bytes(&self) -> &BTreeMap<StableId, Vec<u8>> {
        &self.source_bytes
    }

    pub(crate) fn into_parts(self) -> (ProgramSpace, BTreeMap<StableId, Vec<u8>>) {
        (self.program, self.source_bytes)
    }
}

/// Accepted ProgramSpace-v3 source used by the cross-run M6 fixture.  The
/// source bytes are returned with the program so Event can admit the exact CAS
/// content whose hashes appear in the accepted file facts.
#[cfg(test)]
pub(crate) fn complete_m5_s0_program_fixture() -> M6Result<M6ProgramFixture> {
    let mut value: Value = serde_json::from_slice(include_bytes!(
        "../../../examples/double-submit-payment/program-space.json"
    ))
    .map_err(|error| M6Error::Canonical(error.to_string()))?;
    let checkout_id = StableId::parse("file:checkout-controller")?;
    let repository_id = StableId::parse("file:payment-repository")?;
    let source_bytes = BTreeMap::from([
        (checkout_id.clone(), b"checkout\n".repeat(40)),
        (repository_id.clone(), b"repository\n".repeat(40)),
    ]);

    value["schema"] = Value::String("reviewgraphen.program_space.input.v3".to_owned());
    value["source"]["kind"] = Value::String("git".to_owned());
    value["source"]["revision"] = Value::String(format!("{:040x}", 10));
    value["source"]["content_hash"] = Value::String(format!("git:{:040x}", 11));
    value["snapshot"]["base_revision"] = Value::String(format!("{:040x}", 9));
    value["snapshot"]["target_revision"] = Value::String(format!("{:040x}", 10));
    value["snapshot"]["tree_hash"] = Value::String(format!("git:{:040x}", 11));
    value["profile"]["id"] = Value::String("double-submit-payment".to_owned());
    value["profile"]["version"] = Value::String("1".to_owned());

    for context in value["contexts"]
        .as_array_mut()
        .ok_or(M6Error::InvalidHistoricalTopology(
            "fixture contexts are not an array",
        ))?
    {
        if context["id"] == crate::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID {
            let members =
                context["member_ids"]
                    .as_array_mut()
                    .ok_or(M6Error::InvalidHistoricalTopology(
                        "fixture context members are not an array",
                    ))?;
            members.extend([
                Value::String(checkout_id.to_string()),
                Value::String(repository_id.to_string()),
            ]);
            members.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
        }
    }

    let relation_template = value["relations"]
        .as_array()
        .and_then(|relations| relations.first())
        .cloned()
        .ok_or(M6Error::InvalidHistoricalTopology(
            "fixture has no relation template",
        ))?;
    let mut contains = relation_template;
    contains["id"] = Value::String("relation:file-contains-payment-charge".to_owned());
    contains["kind"] = Value::String("contains".to_owned());
    contains["source_id"] = Value::String(repository_id.to_string());
    contains["target_ids"] = serde_json::json!(["function:payment-charge"]);
    contains["directed"] = Value::Bool(true);
    value["relations"]
        .as_array_mut()
        .expect("fixture relations checked above")
        .push(contains);

    let mut anchors = serde_json::Map::new();
    for artifact in value["artifacts"]
        .as_array_mut()
        .ok_or(M6Error::InvalidHistoricalTopology(
            "fixture artifacts are not an array",
        ))?
    {
        let artifact_id = artifact["id"]
            .as_str()
            .ok_or(M6Error::InvalidHistoricalTopology(
                "fixture artifact has no ID",
            ))?
            .to_owned();
        if artifact["kind"] == "file" {
            let id = StableId::parse(&artifact_id)?;
            let bytes = source_bytes
                .get(&id)
                .ok_or(M6Error::InvalidHistoricalTopology(
                    "fixture file has no source bytes",
                ))?;
            artifact["content_hash"] = Value::String(ContentHash::sha256(bytes).to_string());
        }
        if artifact_id == "test:double-submit" {
            artifact["location"]["start_line"] = Value::Null;
            artifact["location"]["end_line"] = Value::Null;
        }
        let rust_symbol = artifact["language"] == "rust"
            && matches!(
                artifact["kind"].as_str(),
                Some("function" | "method" | "type")
            );
        if rust_symbol {
            artifact["provenance"]["extraction_method"] =
                Value::String("reviewgraphen.ingest.rust_syn.v1".to_owned());
            anchors.insert(
                artifact_id.clone(),
                serde_json::json!({
                    "descriptor": RUST_SYMBOL_ANCHOR_V1,
                    "language": "rust",
                    "symbol_kind": artifact["kind"].as_str().expect("symbol kind"),
                    "signature_shape_hash": ContentHash::sha256(format!("{artifact_id}:signature").as_bytes()),
                    "normalized_body_hash": ContentHash::sha256(format!("{artifact_id}:body").as_bytes()),
                }),
            );
        }
    }

    for relation in value["relations"]
        .as_array_mut()
        .expect("fixture relations checked above")
    {
        relation["ordered_target_ids"] = relation["target_ids"].clone();
    }
    let provenance = value["artifacts"][0]["provenance"].clone();
    value["evidence"] = serde_json::json!([{
        "id": "evidence:seeded-payment-source",
        "kind": "static_analysis",
        "target_ids": ["function:payment-charge"],
        "artifact_ref": null,
        "content_hash": null,
        "attributes": {"seeded": true},
        "provenance": provenance,
    }]);
    let invariant = value["invariants"]
        .as_array_mut()
        .and_then(|invariants| {
            invariants
                .iter_mut()
                .find(|invariant| invariant["property_id"] == crate::M4_PROPERTY_ID)
        })
        .ok_or(M6Error::InvalidHistoricalTopology(
            "fixture payment invariant is absent",
        ))?;
    invariant["scope_ids"] = serde_json::json!(["context:payment", "context:ui-event"]);
    value["incremental_facts"] = serde_json::json!({
        "git_revision_closure": {
            "base_commit_oid": format!("{:040x}", 9),
            "base_tree_hash": format!("git:{:040x}", 8),
            "target_commit_oid": format!("{:040x}", 10),
            "target_tree_hash": format!("git:{:040x}", 11),
        },
        "rust_anchor_extractor_id": crate::RUST_SYMBOL_ANCHOR_EXTRACTOR_V1,
        "rust_anchor_syn_version": crate::RUST_SYMBOL_ANCHOR_SYN_VERSION_V1,
        "rust_symbol_anchors": anchors,
    });
    let program = ProgramSpace::from_json_slice(
        &serde_json::to_vec(&value).map_err(|error| M6Error::Canonical(error.to_string()))?,
    )?;
    Ok(M6ProgramFixture {
        program,
        source_bytes,
    })
}

#[cfg(test)]
pub(crate) fn distinct_s1_program_fixture_from(
    source: &M6ProgramFixture,
) -> M6Result<M6ProgramFixture> {
    Ok(M6ProgramFixture {
        program: distinct_s1_program_from(source.program())?,
        source_bytes: source.source_bytes().clone(),
    })
}

#[cfg(test)]
#[allow(clippy::type_complexity)]
pub(crate) fn m6_distinct_s0_s1_program_fixture() -> M6Result<(
    ProgramSpace,
    ProgramSpace,
    BTreeMap<StableId, Vec<u8>>,
    BTreeMap<StableId, Vec<u8>>,
)> {
    let source = complete_m5_s0_program_fixture()?;
    let target = distinct_s1_program_fixture_from(&source)?;
    let (source_program, source_bytes) = source.into_parts();
    let (target_program, target_bytes) = target.into_parts();
    Ok((source_program, target_program, source_bytes, target_bytes))
}

/// The candidate fixture changes only the accepted S0 context membership that
/// admits the already-present source artifacts to the fixed UI context.  Its
/// S1 is derived from that exact S0, keeping the cross-snapshot topology and
/// source hashes independent from the obstruction fixture.
#[cfg(test)]
#[allow(clippy::type_complexity)]
pub(crate) fn successful_m5_distinct_s0_s1_program_fixture() -> M6Result<(
    ProgramSpace,
    ProgramSpace,
    BTreeMap<StableId, Vec<u8>>,
    BTreeMap<StableId, Vec<u8>>,
)> {
    let source = complete_m5_s0_program_fixture()?;
    let (source_program, source_bytes) = source.into_parts();
    let mut source_value = serde_json::to_value(source_program.streaming_ref())
        .map_err(|error| M6Error::Canonical(error.to_string()))?;
    let ui_members = source_value["contexts"]
        .as_array_mut()
        .ok_or(M6Error::InvalidHistoricalTopology(
            "successful fixture contexts are not an array",
        ))?
        .iter_mut()
        .find(|context| context["id"] == crate::DOUBLE_SUBMIT_UI_CONTEXT_ID)
        .and_then(|context| context["member_ids"].as_array_mut())
        .ok_or(M6Error::InvalidHistoricalTopology(
            "successful fixture UI context members are absent",
        ))?;
    ui_members.extend([
        Value::String("file:checkout-controller".to_owned()),
        Value::String("file:payment-repository".to_owned()),
    ]);
    ui_members.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    ui_members.dedup();
    let source_program = ProgramSpace::from_json_slice(
        &serde_json::to_vec(&source_value)
            .map_err(|error| M6Error::Canonical(error.to_string()))?,
    )?;
    let target_program = distinct_s1_program_from(&source_program)?;
    let target_bytes = source_bytes.clone();
    Ok((source_program, target_program, source_bytes, target_bytes))
}

/// Small accepted source-bearing topology for the scheduled-reviewer tests.
/// It differs from the successful M5 fixture only by declaring every extractor
/// capability complete, so the derived rerun set contains the concrete payment
/// obligation rather than source-less capability-gap obligations.
#[cfg(test)]
#[allow(clippy::type_complexity)]
pub(crate) fn scheduled_reviewer_distinct_s0_s1_program_fixture() -> M6Result<(
    ProgramSpace,
    ProgramSpace,
    BTreeMap<StableId, Vec<u8>>,
    BTreeMap<StableId, Vec<u8>>,
)> {
    let (source_program, _, source_bytes, _) = successful_m5_distinct_s0_s1_program_fixture()?;
    let mut source = serde_json::to_value(source_program.streaming_ref())
        .map_err(|error| M6Error::Canonical(error.to_string()))?;
    let capabilities = source["extraction"]["capabilities"].as_object_mut().ok_or(
        M6Error::InvalidHistoricalTopology("scheduled reviewer fixture capabilities are absent"),
    )?;
    for capability in capabilities.values_mut() {
        capability["state"] = Value::String("complete".to_owned());
    }
    source["extraction"]["limitations"] = Value::Array(Vec::new());
    // Keep the invariant as the sole at-most-once obligation source.  The
    // base example also tags the integration-test cover relation with the
    // same property, which deliberately produces a second, indistinguishable
    // obligation and therefore exercises the ambiguity path instead.
    if let Some(relation) = source["relations"].as_array_mut().and_then(|relations| {
        relations
            .iter_mut()
            .find(|relation| relation["id"] == "relation:test-covers-path")
    }) && let Some(attrs) = relation["attributes"].as_object_mut()
    {
        attrs.remove("property_id");
    }
    let source_program = ProgramSpace::from_json_slice(
        &serde_json::to_vec(&source).map_err(|error| M6Error::Canonical(error.to_string()))?,
    )?;
    let target_program = distinct_s1_program_from(&source_program)?;
    let target_bytes = source_bytes.clone();
    Ok((source_program, target_program, source_bytes, target_bytes))
}

/// Positive post-D2 gluing topology.  Unlike the successful-M5 source
/// fixture, the UI context keeps its original member set so the frozen M5
/// selector observes one claim per fixed context after reviewer replay.  All
/// extraction capabilities are complete to avoid source-less capability-gap
/// obligations in the scheduled suffix.
#[cfg(test)]
#[allow(clippy::type_complexity)]
pub(crate) fn gluing_positive_distinct_s0_s1_program_fixture() -> M6Result<(
    ProgramSpace,
    ProgramSpace,
    BTreeMap<StableId, Vec<u8>>,
    BTreeMap<StableId, Vec<u8>>,
)> {
    let source = complete_m5_s0_program_fixture()?;
    let (source_program, mut source_bytes) = source.into_parts();
    let mut source = serde_json::to_value(source_program.streaming_ref())
        .map_err(|error| M6Error::Canonical(error.to_string()))?;
    let capabilities = source["extraction"]["capabilities"].as_object_mut().ok_or(
        M6Error::InvalidHistoricalTopology("gluing positive fixture capabilities are absent"),
    )?;
    for capability in capabilities.values_mut() {
        capability["state"] = Value::String("complete".to_owned());
    }
    source["extraction"]["limitations"] = Value::Array(Vec::new());
    // Both fixed contexts reach checkout-submit.  This file is reachable by
    // the context builder through `contains`, but is intentionally not a
    // direct context member.  It therefore yields source-grounded reviewer
    // claims whose sources are disjoint from M(c), exercising Missing/Missing
    // rather than weakening the frozen selector.
    let gluing_source_id = StableId::parse("file:gluing-review-source")?;
    let gluing_bytes = b"gluing-review-source\n".repeat(40);
    let gluing_hash = ContentHash::sha256(&gluing_bytes);
    source_bytes.insert(gluing_source_id.clone(), gluing_bytes);
    let artifact = source["artifacts"]
        .as_array()
        .and_then(|artifacts| {
            artifacts
                .iter()
                .find(|artifact| artifact["id"] == "file:checkout-controller")
        })
        .cloned()
        .ok_or(M6Error::InvalidHistoricalTopology(
            "gluing positive fixture checkout file is absent",
        ))?;
    let mut artifact = artifact;
    artifact["id"] = Value::String(gluing_source_id.to_string());
    artifact["content_hash"] = Value::String(gluing_hash.to_string());
    artifact["location"]["path"] = Value::String("src/gluing_review.rs".to_owned());
    source["artifacts"]
        .as_array_mut()
        .ok_or(M6Error::InvalidHistoricalTopology(
            "gluing positive fixture artifacts are absent",
        ))?
        .push(artifact);
    let relation = source["relations"]
        .as_array()
        .and_then(|relations| relations.first())
        .cloned()
        .ok_or(M6Error::InvalidHistoricalTopology(
            "gluing positive fixture relation template is absent",
        ))?;
    let mut relation = relation;
    relation["id"] = Value::String("relation:gluing-review-contains-checkout".to_owned());
    relation["kind"] = Value::String("contains".to_owned());
    relation["source_id"] = Value::String(gluing_source_id.to_string());
    relation["target_ids"] = serde_json::json!(["function:checkout-submit"]);
    relation["ordered_target_ids"] = serde_json::json!(["function:checkout-submit"]);
    relation["directed"] = Value::Bool(true);
    relation["attributes"] = serde_json::json!({});
    source["relations"]
        .as_array_mut()
        .ok_or(M6Error::InvalidHistoricalTopology(
            "gluing positive fixture relations are absent",
        ))?
        .push(relation);
    let source_program = ProgramSpace::from_json_slice(
        &serde_json::to_vec(&source).map_err(|error| M6Error::Canonical(error.to_string()))?,
    )?;
    let target_program = distinct_s1_program_from(&source_program)?;
    let target_bytes = source_bytes.clone();
    Ok((source_program, target_program, source_bytes, target_bytes))
}

/// Selected/Selected post-D2 gluing topology. Each fixed context owns one
/// exclusive direct source, so the frozen selector has exactly one eligible
/// fresh claim in each context.
#[cfg(test)]
#[allow(clippy::type_complexity)]
pub(crate) fn gluing_selected_distinct_s0_s1_program_fixture() -> M6Result<(
    ProgramSpace,
    ProgramSpace,
    BTreeMap<StableId, Vec<u8>>,
    BTreeMap<StableId, Vec<u8>>,
)> {
    let (program, _, mut bytes, _) = gluing_positive_distinct_s0_s1_program_fixture()?;
    let mut value = serde_json::to_value(program.streaming_ref())
        .map_err(|error| M6Error::Canonical(error.to_string()))?;
    let template = value["artifacts"]
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|item| item["id"] == "file:checkout-controller")
        })
        .cloned()
        .ok_or(M6Error::InvalidHistoricalTopology(
            "UI source template is absent",
        ))?;
    let sources = [
        (
            "file:payment-review-source",
            "src/payment_review.rs",
            crate::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID,
        ),
        (
            "file:ui-review-source",
            "src/ui_review.rs",
            crate::DOUBLE_SUBMIT_UI_CONTEXT_ID,
        ),
    ];
    for (source_id, path, context_id) in sources {
        let id = StableId::parse(source_id)?;
        let content = format!("{source_id}\n").repeat(32).into_bytes();
        let hash = ContentHash::sha256(&content);
        bytes.insert(id.clone(), content);
        let mut artifact = template.clone();
        artifact["id"] = Value::String(id.to_string());
        artifact["content_hash"] = Value::String(hash.to_string());
        artifact["location"]["path"] = Value::String(path.to_owned());
        value["artifacts"]
            .as_array_mut()
            .ok_or(M6Error::InvalidHistoricalTopology(
                "selected source artifacts are absent",
            ))?
            .push(artifact);
        let members = value["contexts"]
            .as_array_mut()
            .and_then(|items| items.iter_mut().find(|item| item["id"] == context_id))
            .and_then(|item| item["member_ids"].as_array_mut())
            .ok_or(M6Error::InvalidHistoricalTopology(
                "selected source context is absent",
            ))?;
        members.push(Value::String(id.to_string()));
        members.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
        members.dedup();
    }
    let source = ProgramSpace::from_json_slice(
        &serde_json::to_vec(&value).map_err(|error| M6Error::Canonical(error.to_string()))?,
    )?;
    let target = distinct_s1_program_from(&source)?;
    Ok((source, target, bytes.clone(), bytes))
}

#[cfg(test)]
#[allow(clippy::type_complexity)]
pub(crate) fn preservation_distinct_s0_s1_program_fixture() -> M6Result<(
    ProgramSpace,
    ProgramSpace,
    BTreeMap<StableId, Vec<u8>>,
    BTreeMap<StableId, Vec<u8>>,
)> {
    let source = complete_m5_s0_program_fixture()?;
    let (source_program, source_bytes) = source.into_parts();
    let mut target = serde_json::to_value(source_program.streaming_ref())
        .map_err(|error| M6Error::Canonical(error.to_string()))?;
    fn replace(value: &mut Value, from: &str, to: &str) {
        match value {
            Value::String(text) if text == from => *text = to.to_owned(),
            Value::Array(values) => values.iter_mut().for_each(|value| replace(value, from, to)),
            Value::Object(values) => values
                .values_mut()
                .for_each(|value| replace(value, from, to)),
            _ => {}
        }
    }
    replace(
        &mut target,
        source_program.snapshot_id().as_str(),
        "snapshot:double-submit-preservation-v2",
    );
    let target_commit_oid = format!("{:040x}", 14);
    let target_tree_hash = format!("git:{:040x}", 15);
    target["snapshot"]["base_revision"] =
        Value::String(source_program.target_revision().to_owned());
    target["snapshot"]["target_revision"] = Value::String(target_commit_oid.clone());
    target["snapshot"]["tree_hash"] = Value::String(target_tree_hash.clone());
    target["source"]["revision"] = Value::String(target_commit_oid.clone());
    target["source"]["content_hash"] = Value::String(target_tree_hash.clone());
    let source_git = source_program.accepted_git_revision_closure().ok_or(
        M6Error::InvalidHistoricalTopology("preservation fixture lacks revision closure"),
    )?;
    let revisions = &mut target["incremental_facts"]["git_revision_closure"];
    revisions["base_commit_oid"] = Value::String(source_git.target_commit_oid().to_owned());
    revisions["base_tree_hash"] = Value::String(source_git.target_tree_hash().to_string());
    revisions["target_commit_oid"] = Value::String(target_commit_oid);
    revisions["target_tree_hash"] = Value::String(target_tree_hash);
    let target_program = ProgramSpace::from_json_slice(
        &serde_json::to_vec(&target).map_err(|error| M6Error::Canonical(error.to_string()))?,
    )?;
    let target_bytes = source_bytes.clone();
    Ok((source_program, target_program, source_bytes, target_bytes))
}

#[cfg(test)]
pub(crate) fn distinct_s1_program_from(source: &ProgramSpace) -> M6Result<ProgramSpace> {
    fn replace(value: &mut Value, from: &str, to: &str) {
        match value {
            Value::String(text) if text == from => *text = to.to_owned(),
            Value::Array(values) => values.iter_mut().for_each(|value| replace(value, from, to)),
            Value::Object(values) => values
                .values_mut()
                .for_each(|value| replace(value, from, to)),
            _ => {}
        }
    }
    let mut target = serde_json::to_value(source.streaming_ref())
        .map_err(|error| M6Error::Canonical(error.to_string()))?;
    replace(
        &mut target,
        source.snapshot_id().as_str(),
        "snapshot:double-submit-v2",
    );
    let target_commit_oid = format!("{:040x}", 12);
    let target_tree_hash = format!("git:{:040x}", 13);
    target["snapshot"]["base_revision"] = Value::String(source.target_revision().to_owned());
    target["snapshot"]["target_revision"] = Value::String(target_commit_oid.clone());
    target["snapshot"]["tree_hash"] = Value::String(target_tree_hash.clone());
    if let Some(source_git) = source.accepted_git_revision_closure() {
        // ProgramSpace v3 binds both the accepted source descriptor and the
        // incremental closure to the target revision.  Preserve S0's target
        // as the exact S1 base while advancing only S1's target coordinate.
        target["source"]["revision"] = Value::String(target_commit_oid.clone());
        target["source"]["content_hash"] = Value::String(target_tree_hash.clone());
        let revisions = &mut target["incremental_facts"]["git_revision_closure"];
        revisions["base_commit_oid"] = Value::String(source_git.target_commit_oid().to_owned());
        revisions["base_tree_hash"] = Value::String(source_git.target_tree_hash().to_string());
        revisions["target_commit_oid"] = Value::String(target_commit_oid);
        revisions["target_tree_hash"] = Value::String(target_tree_hash);
    }
    ProgramSpace::from_json_slice(
        &serde_json::to_vec(&target).map_err(|error| M6Error::Canonical(error.to_string()))?,
    )
    .map_err(M6Error::from)
}

#[cfg(test)]
pub(crate) struct M6FixturePhases {
    closure: IncrementalSourceClosureV5,
    mapping: M6MappingPhaseV5,
    correspondence: M6ObligationCorrespondencePhaseV5,
}

#[cfg(test)]
impl M6FixturePhases {
    pub(crate) fn closure(&self) -> &IncrementalSourceClosureV5 {
        &self.closure
    }

    pub(crate) fn mapping(&self) -> &M6MappingPhaseV5 {
        &self.mapping
    }

    pub(crate) fn correspondence(&self) -> &M6ObligationCorrespondencePhaseV5 {
        &self.correspondence
    }

    pub(crate) fn corrupt_mapping_status_for_test(
        &mut self,
        mapping_id: &StableId,
        status: MappingStatusV5,
    ) {
        self.mapping
            .mappings
            .iter_mut()
            .find(|mapping| mapping.id() == mapping_id)
            .expect("test dependency mapping")
            .status = status;
    }
}

#[cfg(test)]
pub(crate) fn m6_fixture_phases_from_exact_prefixes(
    source: &crate::event::CompleteM5V4Fixture,
    target: &crate::event::NoM5V5Fixture,
    target_actual: &TargetActualRecordInventoryV5<'_>,
) -> M6Result<M6FixturePhases> {
    let source_index = ContentHash::sha256(b"m6-e2e-source-index-v5");
    let target_index = ContentHash::sha256(b"m6-e2e-target-index-v6");
    let proposal = derive_untrusted_incremental_mapping_proposal_v5(
        source.log(),
        source.basis(),
        source.completed(),
        &source_index,
        target.log(),
        &target_index,
    )?;
    let closure = proposal
        .closure
        .bind_terminal_target_authority_v5(target_actual)?;
    if closure.source_snapshot_id() != source.program().snapshot_id()
        || closure.target_snapshot_id() != target.program().snapshot_id()
        || closure.input.source_run_id != *source.run_id()
        || closure.input.target_run_id != *target.run_id()
    {
        return Err(M6Error::InvalidHistoricalTopology(
            "fixture proposal coordinates differ from exact replay prefixes",
        ));
    }
    let accepted = |program: &ProgramSpace| -> M6Result<ReviewAggregate> {
        let (universe, obligations) = crate::MvpRulePack::synthesize(program)
            .map_err(M6Error::from)?
            .into_parts();
        ReviewAggregate::new(program.clone(), universe, obligations).map_err(M6Error::from)
    };
    let source_aggregate = accepted(source.program())?;
    let target_aggregate = accepted(target.program())?;
    let mapping = ChangeMorphismV5::derive_from_accepted_program_facts(
        &closure,
        source.program(),
        target.program(),
    )?;
    let correspondence = ObligationCorrespondenceV5::derive_from_accepted_universes(
        &closure,
        &mapping,
        &source_aggregate,
        &target_aggregate,
    )?;
    Ok(M6FixturePhases {
        closure,
        mapping,
        correspondence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_d2_gluing_dtos_are_strict_and_whole_set_dag_is_closed() {
        let closure = StableId::parse("incremental-source-closure-v5:c").unwrap();
        let partial = StableId::parse("partial-rerun-plan-v5:p").unwrap();
        let plan = StableId::parse("plan:target").unwrap();
        let payment = StableId::parse(crate::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID).unwrap();
        let ui = StableId::parse(crate::DOUBLE_SUBMIT_UI_CONTEXT_ID).unwrap();
        let bindings = [
            GluingClaimBindingV5::new(payment.clone(), GluingClaimBindingStatusV5::Missing, None)
                .unwrap(),
            GluingClaimBindingV5::new(ui.clone(), GluingClaimBindingStatusV5::Missing, None)
                .unwrap(),
        ];
        let scope = derive(
            "gluing-rerun-scope-v5",
            &(&closure, &partial, &plan, &bindings),
        )
        .unwrap();
        let payment_registration = GluingRerunActionV5::derive(
            scope.clone(),
            GluingRerunSubjectKindV5::GluingContext,
            payment,
            GluingRerunActionKindV5::RegisterGluingInput,
            vec![],
        )
        .unwrap();
        let ui_registration = GluingRerunActionV5::derive(
            scope.clone(),
            GluingRerunSubjectKindV5::GluingContext,
            ui,
            GluingRerunActionKindV5::RegisterGluingInput,
            vec![],
        )
        .unwrap();
        let invariant = StableId::parse(crate::DOUBLE_SUBMIT_INVARIANT_ID).unwrap();
        let mut reglue_prereqs = vec![
            ActionPrerequisiteV5::scheduled(payment_registration.id.clone()).unwrap(),
            ActionPrerequisiteV5::scheduled(ui_registration.id.clone()).unwrap(),
        ];
        reglue_prereqs.sort();
        let reglue = GluingRerunActionV5::derive(
            scope,
            GluingRerunSubjectKindV5::GluingAttempt,
            invariant,
            GluingRerunActionKindV5::Reglue,
            reglue_prereqs,
        )
        .unwrap();
        let mut actions = vec![payment_registration, ui_registration, reglue];
        actions.sort_by(|left, right| left.id.cmp(&right.id));
        let seal = GluingRerunPlanSealV5::derive(closure, partial, plan, bindings, &actions, None)
            .unwrap();
        let bytes = crate::canonical_json(&seal).unwrap();
        assert_eq!(
            GluingRerunPlanSealV5::from_event_json_bytes(&bytes).unwrap(),
            seal
        );

        let mut unknown: Value = serde_json::from_slice(&bytes).unwrap();
        unknown
            .as_object_mut()
            .unwrap()
            .insert("unknown".to_owned(), Value::Bool(true));
        assert!(
            GluingRerunPlanSealV5::from_event_json_bytes(&crate::canonical_json(&unknown).unwrap())
                .is_err()
        );

        assert!(
            GluingRerunPlanSealV5::derive(
                StableId::parse("incremental-source-closure-v5:c").unwrap(),
                StableId::parse("partial-rerun-plan-v5:p").unwrap(),
                StableId::parse("plan:target").unwrap(),
                [
                    GluingClaimBindingV5::new(
                        StableId::parse(crate::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID).unwrap(),
                        GluingClaimBindingStatusV5::Missing,
                        None,
                    )
                    .unwrap(),
                    GluingClaimBindingV5::new(
                        StableId::parse(crate::DOUBLE_SUBMIT_UI_CONTEXT_ID).unwrap(),
                        GluingClaimBindingStatusV5::Missing,
                        None,
                    )
                    .unwrap(),
                ],
                &[],
                None,
            )
            .is_err()
        );
        let existing = ExistingTargetRecordV5::new(
            StableId::parse("gluing-attempt-v4:existing").unwrap(),
            ContentHash::sha256(b"existing M5 bundle"),
            StableId::parse("event:existing-m5").unwrap(),
        )
        .unwrap();
        assert!(
            GluingRerunPlanSealV5::derive(
                StableId::parse("incremental-source-closure-v5:c").unwrap(),
                StableId::parse("partial-rerun-plan-v5:p").unwrap(),
                StableId::parse("plan:target").unwrap(),
                [
                    GluingClaimBindingV5::new(
                        StableId::parse(crate::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID).unwrap(),
                        GluingClaimBindingStatusV5::Missing,
                        None,
                    )
                    .unwrap(),
                    GluingClaimBindingV5::new(
                        StableId::parse(crate::DOUBLE_SUBMIT_UI_CONTEXT_ID).unwrap(),
                        GluingClaimBindingStatusV5::Missing,
                        None,
                    )
                    .unwrap(),
                ],
                &[],
                Some(existing),
            )
            .is_ok()
        );
        let wrong_existing = ExistingTargetRecordV5::new(
            StableId::parse("gluing-attempt:existing").unwrap(),
            ContentHash::sha256(b"existing M5 bundle"),
            StableId::parse("event:existing-m5").unwrap(),
        )
        .unwrap();
        assert!(
            GluingRerunPlanSealV5::derive(
                StableId::parse("incremental-source-closure-v5:c").unwrap(),
                StableId::parse("partial-rerun-plan-v5:p").unwrap(),
                StableId::parse("plan:target").unwrap(),
                [
                    GluingClaimBindingV5::new(
                        StableId::parse(crate::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID).unwrap(),
                        GluingClaimBindingStatusV5::Missing,
                        None,
                    )
                    .unwrap(),
                    GluingClaimBindingV5::new(
                        StableId::parse(crate::DOUBLE_SUBMIT_UI_CONTEXT_ID).unwrap(),
                        GluingClaimBindingStatusV5::Missing,
                        None,
                    )
                    .unwrap(),
                ],
                &[],
                Some(wrong_existing),
            )
            .is_err(),
            "only the frozen M5 attempt namespace can suppress the whole DAG"
        );

        let selected = [
            GluingClaimBindingV5::new(
                StableId::parse(crate::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID).unwrap(),
                GluingClaimBindingStatusV5::Selected,
                Some((
                    (
                        StableId::parse("claim:payment").unwrap(),
                        ContentHash::sha256(b"payment claim"),
                    ),
                    StableId::parse("obligation:payment").unwrap(),
                )),
            )
            .unwrap(),
            GluingClaimBindingV5::new(
                StableId::parse(crate::DOUBLE_SUBMIT_UI_CONTEXT_ID).unwrap(),
                GluingClaimBindingStatusV5::Missing,
                None,
            )
            .unwrap(),
        ];
        assert!(
            GluingRerunPlanSealV5::derive(
                StableId::parse("incremental-source-closure-v5:c").unwrap(),
                StableId::parse("partial-rerun-plan-v5:p").unwrap(),
                StableId::parse("plan:target").unwrap(),
                selected,
                &actions,
                None,
            )
            .is_err(),
            "a Selected binding cannot omit its rebuild action"
        );

        let selected_bindings = [
            GluingClaimBindingV5::new(
                StableId::parse(crate::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID).unwrap(),
                GluingClaimBindingStatusV5::Selected,
                Some((
                    (
                        StableId::parse("claim:payment-matrix").unwrap(),
                        ContentHash::sha256(b"payment matrix claim"),
                    ),
                    StableId::parse("obligation:payment-matrix").unwrap(),
                )),
            )
            .unwrap(),
            GluingClaimBindingV5::new(
                StableId::parse(crate::DOUBLE_SUBMIT_UI_CONTEXT_ID).unwrap(),
                GluingClaimBindingStatusV5::Selected,
                Some((
                    (
                        StableId::parse("claim:ui-matrix").unwrap(),
                        ContentHash::sha256(b"ui matrix claim"),
                    ),
                    StableId::parse("obligation:ui-matrix").unwrap(),
                )),
            )
            .unwrap(),
        ];
        let matrix_closure = StableId::parse("incremental-source-closure-v5:matrix").unwrap();
        let matrix_partial = StableId::parse("partial-rerun-plan-v5:matrix").unwrap();
        let matrix_plan = StableId::parse("plan:matrix").unwrap();
        let matrix_scope = derive(
            "gluing-rerun-scope-v5",
            &(
                &matrix_closure,
                &matrix_partial,
                &matrix_plan,
                &selected_bindings,
            ),
        )
        .unwrap();
        let payment_registration = GluingRerunActionV5::derive(
            matrix_scope.clone(),
            GluingRerunSubjectKindV5::GluingContext,
            StableId::parse(crate::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID).unwrap(),
            GluingRerunActionKindV5::RegisterGluingInput,
            vec![],
        )
        .unwrap();
        let ui_registration = GluingRerunActionV5::derive(
            matrix_scope.clone(),
            GluingRerunSubjectKindV5::GluingContext,
            StableId::parse(crate::DOUBLE_SUBMIT_UI_CONTEXT_ID).unwrap(),
            GluingRerunActionKindV5::RegisterGluingInput,
            vec![],
        )
        .unwrap();
        let payment_verifier = ActionPrerequisiteV5::scheduled(
            StableId::parse("partial-rerun-action-v5:payment-verifier").unwrap(),
        )
        .unwrap();
        let ui_verifier = ActionPrerequisiteV5::ExistingTargetRecord {
            record_id: StableId::parse("verification:ui-native").unwrap(),
            body_hash: ContentHash::sha256(b"ui native verification"),
            event_id: StableId::parse("event:ui-native-verification").unwrap(),
        };
        let rebuild =
            |context: &str, registration: &GluingRerunActionV5, verifier: ActionPrerequisiteV5| {
                let mut prerequisites = vec![
                    ActionPrerequisiteV5::scheduled(registration.id.clone()).unwrap(),
                    verifier,
                ];
                prerequisites.sort();
                GluingRerunActionV5::derive(
                    matrix_scope.clone(),
                    GluingRerunSubjectKindV5::GluingContext,
                    StableId::parse(context).unwrap(),
                    GluingRerunActionKindV5::RebuildSection,
                    prerequisites,
                )
                .unwrap()
            };
        let payment_rebuild = rebuild(
            crate::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID,
            &payment_registration,
            payment_verifier.clone(),
        );
        let ui_rebuild = rebuild(
            crate::DOUBLE_SUBMIT_UI_CONTEXT_ID,
            &ui_registration,
            ui_verifier,
        );
        let derive_reglue = |mut prerequisites: Vec<ActionPrerequisiteV5>| {
            prerequisites.sort();
            GluingRerunActionV5::derive(
                matrix_scope.clone(),
                GluingRerunSubjectKindV5::GluingAttempt,
                StableId::parse(crate::DOUBLE_SUBMIT_INVARIANT_ID).unwrap(),
                GluingRerunActionKindV5::Reglue,
                prerequisites,
            )
            .unwrap()
        };
        let exact_reglue_prerequisites = vec![
            ActionPrerequisiteV5::scheduled(payment_registration.id.clone()).unwrap(),
            ActionPrerequisiteV5::scheduled(ui_registration.id.clone()).unwrap(),
            ActionPrerequisiteV5::scheduled(payment_rebuild.id.clone()).unwrap(),
            ActionPrerequisiteV5::scheduled(ui_rebuild.id.clone()).unwrap(),
        ];
        let exact_reglue = derive_reglue(exact_reglue_prerequisites.clone());
        let mut exact_actions = vec![
            payment_registration.clone(),
            ui_registration.clone(),
            payment_rebuild.clone(),
            ui_rebuild.clone(),
            exact_reglue,
        ];
        exact_actions.sort_by(|left, right| left.id.cmp(&right.id));
        GluingRerunPlanSealV5::derive(
            matrix_closure.clone(),
            matrix_partial.clone(),
            matrix_plan.clone(),
            selected_bindings.clone(),
            &exact_actions,
            None,
        )
        .expect("scheduled and native verifier-shaped slots form one closed structural DAG");

        let cross_context_rebuild = rebuild(
            crate::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID,
            &ui_registration,
            payment_verifier.clone(),
        );
        let cross_context_reglue = derive_reglue(vec![
            ActionPrerequisiteV5::scheduled(payment_registration.id.clone()).unwrap(),
            ActionPrerequisiteV5::scheduled(ui_registration.id.clone()).unwrap(),
            ActionPrerequisiteV5::scheduled(cross_context_rebuild.id.clone()).unwrap(),
            ActionPrerequisiteV5::scheduled(ui_rebuild.id.clone()).unwrap(),
        ]);
        let mut cross_context_actions = vec![
            payment_registration.clone(),
            ui_registration.clone(),
            cross_context_rebuild,
            ui_rebuild.clone(),
            cross_context_reglue,
        ];
        cross_context_actions.sort_by(|left, right| left.id.cmp(&right.id));
        assert!(
            GluingRerunPlanSealV5::derive(
                matrix_closure.clone(),
                matrix_partial.clone(),
                matrix_plan.clone(),
                selected_bindings.clone(),
                &cross_context_actions,
                None,
            )
            .is_err(),
            "a payment rebuild cannot depend on the UI registration"
        );

        let omitted_reglue = derive_reglue(
            exact_reglue_prerequisites
                .iter()
                .filter(|value| {
                    **value
                        != ActionPrerequisiteV5::ScheduledAction {
                            action_id: ui_registration.id.clone(),
                        }
                })
                .cloned()
                .collect(),
        );
        let mut omitted_actions = vec![
            payment_registration.clone(),
            ui_registration.clone(),
            payment_rebuild.clone(),
            ui_rebuild.clone(),
            omitted_reglue,
        ];
        omitted_actions.sort_by(|left, right| left.id.cmp(&right.id));
        assert!(
            GluingRerunPlanSealV5::derive(
                matrix_closure.clone(),
                matrix_partial.clone(),
                matrix_plan.clone(),
                selected_bindings.clone(),
                &omitted_actions,
                None,
            )
            .is_err(),
            "reglue cannot omit either context registration"
        );

        let mut extra_reglue_prerequisites = exact_reglue_prerequisites.clone();
        extra_reglue_prerequisites.push(ActionPrerequisiteV5::ExistingTargetRecord {
            record_id: StableId::parse("evidence:extra").unwrap(),
            body_hash: ContentHash::sha256(b"extra"),
            event_id: StableId::parse("event:extra").unwrap(),
        });
        let extra_reglue = derive_reglue(extra_reglue_prerequisites);
        let mut extra_actions = vec![
            payment_registration.clone(),
            ui_registration.clone(),
            payment_rebuild.clone(),
            ui_rebuild.clone(),
            extra_reglue,
        ];
        extra_actions.sort_by(|left, right| left.id.cmp(&right.id));
        assert!(
            GluingRerunPlanSealV5::derive(
                matrix_closure.clone(),
                matrix_partial.clone(),
                matrix_plan.clone(),
                selected_bindings.clone(),
                &extra_actions,
                None,
            )
            .is_err(),
            "reglue cannot carry an extra predecessor"
        );

        let wrong_verifier_rebuild = rebuild(
            crate::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID,
            &payment_registration,
            ActionPrerequisiteV5::ExistingTargetRecord {
                record_id: StableId::parse("evidence:not-verification").unwrap(),
                body_hash: ContentHash::sha256(b"not verification"),
                event_id: StableId::parse("event:not-verification").unwrap(),
            },
        );
        let wrong_verifier_reglue = derive_reglue(vec![
            ActionPrerequisiteV5::scheduled(payment_registration.id.clone()).unwrap(),
            ActionPrerequisiteV5::scheduled(ui_registration.id.clone()).unwrap(),
            ActionPrerequisiteV5::scheduled(wrong_verifier_rebuild.id.clone()).unwrap(),
            ActionPrerequisiteV5::scheduled(ui_rebuild.id.clone()).unwrap(),
        ]);
        let mut wrong_verifier_actions = vec![
            payment_registration,
            ui_registration,
            wrong_verifier_rebuild,
            ui_rebuild,
            wrong_verifier_reglue,
        ];
        wrong_verifier_actions.sort_by(|left, right| left.id.cmp(&right.id));
        assert!(
            GluingRerunPlanSealV5::derive(
                matrix_closure,
                matrix_partial,
                matrix_plan,
                selected_bindings,
                &wrong_verifier_actions,
                None,
            )
            .is_err(),
            "a non-verification record cannot occupy the verifier-shaped slot"
        );

        let partial_action =
            ActionPrerequisiteV5::scheduled(StableId::parse("gluing-rerun-action-v5:x").unwrap())
                .unwrap();
        assert!(validate_partial_action_prerequisite_v5(&partial_action).is_err());

        for status in [
            GluingClaimBindingStatusV5::Selected,
            GluingClaimBindingStatusV5::Missing,
        ] {
            for mask in 0_u8..8 {
                let mut binding = serde_json::json!({
                    "context_id": crate::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID,
                    "status": status,
                    "claim_id": null,
                    "claim_body_hash": null,
                    "obligation_id": null
                });
                if mask & 1 != 0 {
                    binding["claim_id"] = serde_json::json!("claim:bound");
                }
                if mask & 2 != 0 {
                    binding["claim_body_hash"] =
                        serde_json::json!(ContentHash::sha256(b"bound claim"));
                }
                if mask & 4 != 0 {
                    binding["obligation_id"] = serde_json::json!("obligation:bound");
                }
                let accepted = serde_json::from_value::<GluingClaimBindingV5>(binding).is_ok();
                assert_eq!(
                    accepted,
                    (status == GluingClaimBindingStatusV5::Selected && mask == 7)
                        || (status == GluingClaimBindingStatusV5::Missing && mask == 0),
                    "status={status:?}, mask={mask:03b}"
                );
            }
        }
        let unknown_context = serde_json::json!({
            "context_id": "context:other",
            "status": "missing",
            "claim_id": null,
            "claim_body_hash": null,
            "obligation_id": null
        });
        assert!(serde_json::from_value::<GluingClaimBindingV5>(unknown_context).is_err());
        let unknown_field = serde_json::json!({
            "context_id": crate::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID,
            "status": "missing",
            "claim_id": null,
            "claim_body_hash": null,
            "obligation_id": null,
            "unknown": true
        });
        assert!(serde_json::from_value::<GluingClaimBindingV5>(unknown_field).is_err());
        assert!(
            GluingRerunPlanSealV5::derive(
                StableId::parse("incremental-source-closure-v5:order").unwrap(),
                StableId::parse("partial-rerun-plan-v5:order").unwrap(),
                StableId::parse("plan:order").unwrap(),
                [
                    GluingClaimBindingV5::new(
                        StableId::parse(crate::DOUBLE_SUBMIT_UI_CONTEXT_ID).unwrap(),
                        GluingClaimBindingStatusV5::Missing,
                        None,
                    )
                    .unwrap(),
                    GluingClaimBindingV5::new(
                        StableId::parse(crate::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID).unwrap(),
                        GluingClaimBindingStatusV5::Missing,
                        None,
                    )
                    .unwrap(),
                ],
                &[],
                Some(
                    ExistingTargetRecordV5::new(
                        StableId::parse("gluing-attempt-v4:order").unwrap(),
                        ContentHash::sha256(b"order"),
                        StableId::parse("event:order").unwrap(),
                    )
                    .unwrap(),
                ),
            )
            .is_err(),
            "bindings are fixed payment then UI"
        );

        let exact_hostile = vec![b' '; MAX_M6_EVENT_LINE_BYTES - 1];
        assert!(matches!(
            GluingRerunActionV5::from_event_json_bytes(&exact_hostile),
            Err(M6Error::InvalidWire(_))
        ));
        assert!(matches!(
            GluingRerunPlanSealV5::from_event_json_bytes(&exact_hostile),
            Err(M6Error::InvalidWire(_))
        ));
        let plus_one_hostile = vec![b' '; MAX_M6_EVENT_LINE_BYTES];
        assert!(matches!(
            GluingRerunActionV5::from_event_json_bytes(&plus_one_hostile),
            Err(M6Error::Incomplete {
                operation,
                observed,
                ..
            }) if operation == "M6 event-line bytes" && observed == MAX_M6_EVENT_LINE_BYTES + 1
        ));
        assert!(matches!(
            GluingRerunPlanSealV5::from_event_json_bytes(&plus_one_hostile),
            Err(M6Error::Incomplete {
                operation,
                observed,
                ..
            }) if operation == "M6 event-line bytes" && observed == MAX_M6_EVENT_LINE_BYTES + 1
        ));
    }
    use serde_json::Value;

    fn id(value: &str) -> StableId {
        StableId::parse(value).unwrap()
    }

    fn sha(value: usize) -> ContentHash {
        ContentHash::parse(format!("sha256:{value:064x}")).unwrap()
    }

    fn oid(value: usize) -> String {
        format!("{value:040x}")
    }

    fn tree(value: usize) -> ContentHash {
        ContentHash::parse(format!("git:{value:040x}")).unwrap()
    }

    fn staleness_dto_fixture() -> (
        HistoricalRecordAssessmentV5,
        GluingFreshnessV5,
        StalenessAssessmentV5,
    ) {
        let closure_id = id("incremental-source-closure-v5:fixture");
        let morphism_id = id("change-morphism-v5:fixture");
        let correspondence_id = id("obligation-correspondence-v5:fixture");
        let time = "2026-08-12T01:02:03Z";
        let assessment_id = StalenessAssessmentV5::assessment_id(
            &closure_id,
            &morphism_id,
            &correspondence_id,
            time,
        )
        .unwrap();
        let record = HistoricalRecordAssessmentV5::from_parts(HistoricalRecordAssessmentPartsV5 {
            assessment_id: assessment_id.clone(),
            source_record_kind: HistoricalRecordKindV5::Obligation,
            source_record_id: id("obligation:source"),
            source_record_body_hash: sha(1),
            successor_record_ids: BTreeSet::from([id("obligation:target")]),
            status: HistoricalAssessmentStatusV5::StructurallyPreserved,
            directness: StalenessDirectnessV5::NotApplicable,
            reasons: BTreeSet::new(),
            dependency_source_ids: BTreeSet::from([id("artifact:dependency")]),
            mapping_ids: BTreeSet::from([id("program-mapping-v5:fixture")]),
            correspondence_entry_ids: BTreeSet::from([id(
                "obligation-correspondence-entry-v5:fixture",
            )]),
        })
        .unwrap();
        let gluing = GluingFreshnessV5::derive(
            assessment_id,
            id("gluing-attempt-v4:source"),
            HistoricalAssessmentStatusV5::Stale,
            BTreeSet::from([StaleReasonV5::TargetChanged]),
            BTreeSet::from([id("program-mapping-v5:gluing")]),
            BTreeSet::new(),
        )
        .unwrap();
        let parts = StalenessAssessmentSealPartsV5 {
            source_closure_id: closure_id,
            morphism_id,
            correspondence_id,
            assessment_time: time.to_owned(),
            records: vec![record.clone()],
            gluing: vec![gluing.clone()],
            preservation_candidates: BTreeSet::from([id("obligation:target")]),
            m5_dependent_successors: BTreeSet::from([id("obligation:target")]),
        };
        let seal = StalenessAssessmentV5::seal(&parts).unwrap();
        (record, gluing, seal)
    }

    fn partial_rerun_phase_fixture(
        seeds: BTreeMap<StableId, PartialRerunActionSeedV5>,
        preservation_candidates: BTreeSet<StableId>,
    ) -> M6StalenessPhaseV5 {
        let (record, gluing, assessment) = staleness_dto_fixture();
        M6StalenessPhaseV5 {
            records: vec![record],
            gluing_freshness: vec![gluing],
            assessment,
            preservation_candidate_obligation_ids: preservation_candidates,
            target_gluing_required: false,
            m5_dependent_successor_obligation_ids: BTreeSet::new(),
            partial_rerun_seeds: seeds,
            target_plan_id: id("plan:target"),
            target_snapshot_id: id("snapshot:target"),
            target_run_id: id("run:target"),
            target_genesis_hash: sha(91),
            target_tail_hash: sha(92),
            target_event_count: 9,
            target_policy_revision_hash: sha(93),
            target_pre_incremental_basis_digest: sha(94),
            working_bytes: 1,
        }
    }

    fn native_seed(source: &str, reason: StaleReasonV5) -> PartialRerunActionSeedV5 {
        PartialRerunActionSeedV5 {
            require_native_pipeline: true,
            require_human_resolution: false,
            stale_source_record_ids: BTreeSet::from([id(source)]),
            reasons: BTreeSet::from([reason]),
        }
    }

    fn existing_prerequisite(kind: &str, value: usize) -> ActionPrerequisiteV5 {
        ActionPrerequisiteV5::ExistingTargetRecord {
            record_id: id(&format!("{kind}:target-{value}")),
            body_hash: sha(value),
            event_id: id(&format!("event:target-{value}")),
        }
    }

    fn reidentified_unchecked_action(
        action: PartialRerunActionKindV5,
        mut prerequisites: Vec<ActionPrerequisiteV5>,
    ) -> PartialRerunActionV5 {
        prerequisites.sort();
        let (_, _, assessment) = staleness_dto_fixture();
        let subject_ids = BTreeSet::from([id("obligation:target")]);
        let stale_source_record_ids = BTreeSet::from([id("claim:source")]);
        let reasons = BTreeSet::from([StaleReasonV5::TargetChanged]);
        let source_ids = std::iter::once(assessment.id().clone())
            .chain(subject_ids.iter().cloned())
            .chain(stale_source_record_ids.iter().cloned())
            .chain(prerequisites.iter().flat_map(|value| match value {
                ActionPrerequisiteV5::ScheduledAction { action_id } => vec![action_id.clone()],
                ActionPrerequisiteV5::ExistingTargetRecord {
                    record_id,
                    event_id,
                    ..
                } => vec![record_id.clone(), event_id.clone()],
            }))
            .collect::<BTreeSet<_>>();
        let identity = PartialRerunActionIdentityV5 {
            staleness_assessment_id: assessment.id(),
            subject_kind: PartialRerunSubjectKindV5::Obligation,
            subject_ids: &subject_ids,
            action,
            prerequisites: &prerequisites,
            stale_source_record_ids: &stale_source_record_ids,
            reasons: &reasons,
        };
        PartialRerunActionV5 {
            schema: "reviewgraphen.partial_rerun_action.v5",
            id: derive("partial-rerun-action-v5", &identity).unwrap(),
            staleness_assessment_id: assessment.id().clone(),
            subject_kind: PartialRerunSubjectKindV5::Obligation,
            subject_ids,
            action,
            prerequisites,
            stale_source_record_ids,
            reasons,
            source_ids,
        }
    }

    fn seal_partial_rerun_test_plan(
        mut actions: Vec<PartialRerunActionV5>,
        selected_target_ids: BTreeSet<StableId>,
    ) -> M6Result<PartialRerunPlanV5> {
        actions.sort_by(|left, right| left.id.cmp(&right.id));
        let (_, _, assessment) = staleness_dto_fixture();
        PartialRerunPlanV5::seal(PartialRerunPlanPartsV5 {
            source_closure_id: assessment.source_closure_id().clone(),
            morphism_id: assessment.morphism_id().clone(),
            correspondence_id: assessment.correspondence_id().clone(),
            staleness_assessment_id: assessment.id().clone(),
            target_plan_id: id("plan:target"),
            target_gluing_required: false,
            selected_target_ids: &selected_target_ids,
            actions: &actions,
            preservation_verifications: &[],
            required_human_resolution_ids: &BTreeSet::new(),
        })
    }

    #[test]
    fn partial_rerun_dtos_are_strict_and_no_suppression_dag_is_exact() {
        let target = id("obligation:target");
        let phase = partial_rerun_phase_fixture(
            BTreeMap::from([(
                target.clone(),
                native_seed("claim:source", StaleReasonV5::ModelPolicyChanged),
            )]),
            BTreeSet::new(),
        );
        let preservation = M6PreservationPhaseV5::empty(&phase);
        let (actions, plan) = phase
            .plan_partial_rerun_without_target_suppression_v5(&preservation)
            .unwrap();
        assert_eq!(actions.len(), 3);
        assert_eq!(plan.action_count(), 3);
        assert_eq!(plan.selected_target_count(), 1);
        assert_eq!(plan.required_human_resolution_count(), 0);
        assert_eq!(plan.target_plan_id(), &id("plan:target"));
        let by_kind = actions
            .iter()
            .map(|action| (action.action(), action))
            .collect::<BTreeMap<_, _>>();
        let context = by_kind[&PartialRerunActionKindV5::ReprojectContext];
        let reviewer = by_kind[&PartialRerunActionKindV5::RerunReviewer];
        let verifier = by_kind[&PartialRerunActionKindV5::RerunVerifier];
        assert!(context.prerequisites().is_empty());
        assert_eq!(
            reviewer.prerequisites(),
            &[ActionPrerequisiteV5::ScheduledAction {
                action_id: context.id().clone()
            }]
        );
        assert_eq!(
            verifier.prerequisites(),
            &[ActionPrerequisiteV5::ScheduledAction {
                action_id: reviewer.id().clone()
            }]
        );
        for action in &actions {
            assert_eq!(action.subject_ids(), &BTreeSet::from([target.clone()]));
            assert_eq!(
                action.stale_source_record_ids(),
                &BTreeSet::from([id("claim:source")])
            );
            assert_eq!(
                action.reasons(),
                &BTreeSet::from([StaleReasonV5::ModelPolicyChanged])
            );
            assert!(action.source_ids().contains(phase.assessment().id()));
            assert!(action.source_ids().contains(&target));
            assert!(action.source_ids().contains(&id("claim:source")));
            let bytes = crate::canonical_json(action).unwrap();
            assert_eq!(
                PartialRerunActionV5::from_json_bytes(&bytes).unwrap(),
                *action
            );
            let mut unknown: Value = serde_json::from_slice(&bytes).unwrap();
            unknown["unknown"] = Value::Bool(true);
            assert!(
                PartialRerunActionV5::from_json_bytes(&crate::canonical_json(&unknown).unwrap())
                    .is_err()
            );
        }
        let bytes = crate::canonical_json(&plan).unwrap();
        assert_eq!(
            PartialRerunPlanV5::from_json_bytes(&bytes, &plan).unwrap(),
            plan
        );
        let mut changed: Value = serde_json::from_slice(&bytes).unwrap();
        changed["action_count"] = serde_json::json!(4);
        assert!(
            PartialRerunPlanV5::from_json_bytes(&crate::canonical_json(&changed).unwrap(), &plan)
                .is_err()
        );
        let mut detached_preservation = preservation.clone();
        detached_preservation.target_snapshot_id = id("snapshot:other");
        assert!(matches!(
            phase.plan_partial_rerun_without_target_suppression_v5(&detached_preservation),
            Err(M6Error::PreservationUnsupported(_))
        ));

        let existing = ExistingTargetRecordV5::new(
            id("execution:target"),
            sha(44),
            id("event:target-execution"),
        )
        .unwrap();
        let existing_bytes = crate::canonical_json(&existing).unwrap();
        assert_eq!(
            ExistingTargetRecordV5::from_json_bytes(&existing_bytes).unwrap(),
            existing
        );
        let prerequisite = ActionPrerequisiteV5::ExistingTargetRecord {
            record_id: existing.record_id().clone(),
            body_hash: existing.body_hash().clone(),
            event_id: existing.event_id().clone(),
        };
        let prerequisite_bytes = crate::canonical_json(&prerequisite).unwrap();
        assert_eq!(
            ActionPrerequisiteV5::from_json_bytes(&prerequisite_bytes).unwrap(),
            prerequisite
        );
        let mut extra: Value = serde_json::from_slice(&prerequisite_bytes).unwrap();
        extra["extra"] = Value::Bool(true);
        assert!(
            ActionPrerequisiteV5::from_json_bytes(&crate::canonical_json(&extra).unwrap()).is_err()
        );
    }

    #[test]
    fn partial_rerun_prerequisite_shapes_and_scheduled_edges_are_closed() {
        let scheduled_a =
            ActionPrerequisiteV5::scheduled(id("partial-rerun-action-v5:a")).expect("scheduled A");
        let scheduled_b =
            ActionPrerequisiteV5::scheduled(id("partial-rerun-action-v5:b")).expect("scheduled B");
        let existing_context = existing_prerequisite("context-envelope", 70);
        let existing_execution = existing_prerequisite("execution", 71);
        let existing_claim = existing_prerequisite("claim", 72);
        let second_existing_execution = existing_prerequisite("execution", 73);
        let second_existing_claim = existing_prerequisite("claim", 74);
        let existing_verification = existing_prerequisite("verification", 75);
        for invalid in [
            reidentified_unchecked_action(
                PartialRerunActionKindV5::ReprojectContext,
                vec![scheduled_a.clone()],
            ),
            reidentified_unchecked_action(PartialRerunActionKindV5::RerunReviewer, vec![]),
            reidentified_unchecked_action(
                PartialRerunActionKindV5::RerunReviewer,
                vec![scheduled_a.clone(), scheduled_b.clone()],
            ),
            reidentified_unchecked_action(
                PartialRerunActionKindV5::RerunReviewer,
                vec![existing_execution.clone(), existing_claim.clone()],
            ),
            reidentified_unchecked_action(
                PartialRerunActionKindV5::RerunReviewer,
                vec![existing_execution.clone()],
            ),
            reidentified_unchecked_action(
                PartialRerunActionKindV5::RerunReviewer,
                vec![existing_verification.clone()],
            ),
            reidentified_unchecked_action(
                PartialRerunActionKindV5::RerunReviewer,
                vec![scheduled_a.clone(), existing_execution.clone()],
            ),
            reidentified_unchecked_action(PartialRerunActionKindV5::RerunVerifier, vec![]),
            reidentified_unchecked_action(
                PartialRerunActionKindV5::RerunVerifier,
                vec![existing_execution.clone()],
            ),
            reidentified_unchecked_action(
                PartialRerunActionKindV5::RerunVerifier,
                vec![scheduled_a.clone(), scheduled_b.clone()],
            ),
            reidentified_unchecked_action(
                PartialRerunActionKindV5::RerunVerifier,
                vec![scheduled_a.clone(), existing_execution.clone()],
            ),
            reidentified_unchecked_action(
                PartialRerunActionKindV5::RerunVerifier,
                vec![existing_execution.clone(), second_existing_execution],
            ),
            reidentified_unchecked_action(
                PartialRerunActionKindV5::RerunVerifier,
                vec![existing_claim.clone(), second_existing_claim],
            ),
            reidentified_unchecked_action(
                PartialRerunActionKindV5::RerunVerifier,
                vec![existing_context.clone(), existing_claim.clone()],
            ),
            reidentified_unchecked_action(PartialRerunActionKindV5::RerunHumanDecision, vec![]),
            reidentified_unchecked_action(
                PartialRerunActionKindV5::RerunHumanDecision,
                vec![existing_execution.clone(), existing_claim.clone()],
            ),
            reidentified_unchecked_action(
                PartialRerunActionKindV5::RerunHumanDecision,
                vec![existing_execution.clone()],
            ),
            reidentified_unchecked_action(
                PartialRerunActionKindV5::RerunHumanDecision,
                vec![existing_context.clone()],
            ),
            reidentified_unchecked_action(
                PartialRerunActionKindV5::RerunHumanDecision,
                vec![scheduled_a.clone(), scheduled_b.clone()],
            ),
            reidentified_unchecked_action(
                PartialRerunActionKindV5::RerunHumanDecision,
                vec![scheduled_a, existing_execution.clone()],
            ),
        ] {
            let bytes = crate::canonical_json(&invalid).expect("canonical re-ID mutation");
            assert!(matches!(
                PartialRerunActionV5::from_json_bytes(&bytes),
                Err(M6Error::InvalidStalenessAssessment(_))
            ));
        }

        let (_, _, assessment) = staleness_dto_fixture();
        for (action, mut prerequisites) in [
            (
                PartialRerunActionKindV5::RerunReviewer,
                vec![existing_context],
            ),
            (
                PartialRerunActionKindV5::RerunVerifier,
                vec![existing_execution.clone(), existing_claim],
            ),
            (
                PartialRerunActionKindV5::RerunHumanDecision,
                vec![existing_verification],
            ),
        ] {
            prerequisites.sort();
            PartialRerunActionV5::derive(
                assessment.id().clone(),
                id("obligation:target"),
                action,
                prerequisites,
                BTreeSet::new(),
                BTreeSet::new(),
            )
            .expect("exact existing-record prerequisite shape");
        }

        let target_a = id("obligation:a");
        let target_b = id("obligation:b");
        let context_a = PartialRerunActionV5::derive(
            assessment.id().clone(),
            target_a.clone(),
            PartialRerunActionKindV5::ReprojectContext,
            vec![],
            BTreeSet::new(),
            BTreeSet::new(),
        )
        .unwrap();
        let dangling = PartialRerunActionV5::derive(
            assessment.id().clone(),
            target_a.clone(),
            PartialRerunActionKindV5::RerunReviewer,
            vec![ActionPrerequisiteV5::scheduled(id("partial-rerun-action-v5:dangling")).unwrap()],
            BTreeSet::new(),
            BTreeSet::new(),
        )
        .unwrap();
        assert!(matches!(
            seal_partial_rerun_test_plan(
                vec![context_a.clone(), dangling],
                BTreeSet::from([target_a.clone()])
            ),
            Err(M6Error::InvalidStalenessAssessment(_))
        ));

        let cross_subject = PartialRerunActionV5::derive(
            assessment.id().clone(),
            target_b.clone(),
            PartialRerunActionKindV5::RerunReviewer,
            vec![ActionPrerequisiteV5::scheduled(context_a.id().clone()).unwrap()],
            BTreeSet::new(),
            BTreeSet::new(),
        )
        .unwrap();
        assert!(matches!(
            seal_partial_rerun_test_plan(
                vec![context_a.clone(), cross_subject],
                BTreeSet::from([target_a.clone(), target_b]),
            ),
            Err(M6Error::InvalidStalenessAssessment(_))
        ));

        let reviewer_a = PartialRerunActionV5::derive(
            assessment.id().clone(),
            target_a.clone(),
            PartialRerunActionKindV5::RerunReviewer,
            vec![ActionPrerequisiteV5::scheduled(context_a.id().clone()).unwrap()],
            BTreeSet::new(),
            BTreeSet::new(),
        )
        .unwrap();
        let wrong_kind = PartialRerunActionV5::derive(
            assessment.id().clone(),
            target_a.clone(),
            PartialRerunActionKindV5::RerunHumanDecision,
            vec![ActionPrerequisiteV5::scheduled(reviewer_a.id().clone()).unwrap()],
            BTreeSet::new(),
            BTreeSet::new(),
        )
        .unwrap();
        assert!(matches!(
            seal_partial_rerun_test_plan(
                vec![context_a, reviewer_a, wrong_kind],
                BTreeSet::from([target_a]),
            ),
            Err(M6Error::InvalidStalenessAssessment(_))
        ));
    }

    #[test]
    fn partial_rerun_unions_obligations_and_human_preservation_without_suppression() {
        let first = id("obligation:a");
        let second = id("obligation:b");
        let mut human_seed = native_seed(
            "decision-v3:source",
            StaleReasonV5::HumanAuthorityNotCarried,
        );
        human_seed.require_human_resolution = true;
        let phase = partial_rerun_phase_fixture(
            BTreeMap::from([
                (
                    first.clone(),
                    native_seed("execution:source", StaleReasonV5::TargetChanged),
                ),
                (second.clone(), human_seed),
            ]),
            BTreeSet::from([first.clone()]),
        );
        let preservation = M6PreservationPhaseV5::empty(&phase);
        let (actions, plan) = phase
            .plan_partial_rerun_without_target_suppression_v5(&preservation)
            .unwrap();
        assert_eq!(actions.len(), 7);
        assert_eq!(plan.selected_target_count(), 2);
        assert_eq!(plan.required_human_resolution_count(), 1);
        assert_eq!(
            actions
                .iter()
                .filter(|action| action.action() == PartialRerunActionKindV5::RerunHumanDecision)
                .count(),
            1
        );
        assert!(actions.windows(2).all(|pair| pair[0].id() < pair[1].id()));
        let reversed = phase
            .plan_partial_rerun_without_target_suppression_v5(&preservation)
            .unwrap();
        assert_eq!(actions, reversed.0);
        assert_eq!(plan, reversed.1);
    }

    #[test]
    fn partial_rerun_removed_and_gluing_only_rows_schedule_nothing() {
        let phase = partial_rerun_phase_fixture(BTreeMap::new(), BTreeSet::new());
        let preservation = M6PreservationPhaseV5::empty(&phase);
        let (actions, plan) = phase
            .plan_partial_rerun_without_target_suppression_v5(&preservation)
            .unwrap();
        assert!(actions.is_empty());
        assert_eq!(plan.action_count(), 0);
        assert_eq!(plan.selected_target_count(), 0);
    }

    #[test]
    fn partial_rerun_unresolved_and_unsupported_rows_are_conservative() {
        let (_closure, mappings, source, _target, mut correspondence) = correspondence_phase();
        let entry = correspondence.entries.first_mut().unwrap();
        entry.status = MappingStatusV5::Unresolved;
        let source_obligation = entry.from_obligation_ids.first().unwrap().clone();
        let target_obligation = entry.to_obligation_ids.first().unwrap().clone();
        let rows = partial_rerun_row_v5(
            HistoricalSourceRecordKindV4::Claim,
            &BTreeSet::from([id("claim:source")]),
            &source_obligation,
            &ObligationImpactRowV5 {
                direct: BTreeSet::new(),
                indirect: BTreeSet::new(),
                supported: false,
            },
            &BTreeSet::new(),
            false,
            false,
            false,
            &mappings,
            &correspondence,
            source.program(),
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, target_obligation.clone());
        assert!(!rows[0].1.require_human_resolution);
        assert_eq!(
            rows[0].1.reasons,
            BTreeSet::from([
                StaleReasonV5::MappingUnresolved,
                StaleReasonV5::UnsupportedImpactPolicy,
            ])
        );

        let entry = correspondence.entries.first_mut().unwrap();
        entry.status = MappingStatusV5::Removed;
        entry.to_obligation_ids.clear();
        assert!(
            partial_rerun_row_v5(
                HistoricalSourceRecordKindV4::Claim,
                &BTreeSet::from([id("claim:source")]),
                &source_obligation,
                &ObligationImpactRowV5 {
                    direct: BTreeSet::new(),
                    indirect: BTreeSet::new(),
                    supported: true,
                },
                &BTreeSet::new(),
                true,
                false,
                false,
                &mappings,
                &correspondence,
                source.program(),
                &BTreeMap::new(),
                &BTreeMap::new(),
            )
            .unwrap()
            .is_empty()
        );

        let entry = correspondence.entries.first_mut().unwrap();
        entry.status = MappingStatusV5::Added;
        entry.from_obligation_ids.clear();
        entry.to_obligation_ids.insert(target_obligation.clone());
        let mut seeds = BTreeMap::new();
        add_correspondence_only_partial_seeds_v5(&correspondence, &mut seeds);
        assert_eq!(
            seeds[&target_obligation].reasons,
            BTreeSet::from([StaleReasonV5::TargetChanged])
        );
    }

    #[test]
    fn partial_rerun_limits_are_inclusive_and_overflow_closed() {
        let target = id("obligation:target");
        let phase = partial_rerun_phase_fixture(
            BTreeMap::from([(
                target.clone(),
                native_seed("claim:source", StaleReasonV5::TargetChanged),
            )]),
            BTreeSet::new(),
        );
        let preservation = M6PreservationPhaseV5::empty(&phase);
        let independently_counted_metadata_ids = 2_usize;
        let exact_working = 3_usize
            .checked_mul(std::mem::size_of::<PartialRerunActionV5>() + 1_024)
            .and_then(|value| {
                independently_counted_metadata_ids
                    .checked_mul(std::mem::size_of::<StableId>() + 128)
                    .and_then(|metadata| value.checked_add(metadata))
            })
            .and_then(|value| value.checked_add(MAX_M6_CANONICAL_BYTES))
            .expect("independent working-byte boundary");
        let exact = phase
            .plan_partial_rerun_with_limits_for_test(&preservation, 3, exact_working)
            .unwrap();
        assert_eq!(exact.0.len(), 3);
        assert!(matches!(
            phase.plan_partial_rerun_with_limits_for_test(&preservation, 3, exact_working - 1),
            Err(M6Error::Incomplete { .. })
        ));

        let mut four_action_seed = native_seed("claim:source", StaleReasonV5::TargetChanged);
        four_action_seed.require_human_resolution = true;
        let four_action_phase = partial_rerun_phase_fixture(
            BTreeMap::from([(target, four_action_seed)]),
            BTreeSet::new(),
        );
        let four_action_preservation = M6PreservationPhaseV5::empty(&four_action_phase);
        assert!(matches!(
            four_action_phase.plan_partial_rerun_with_limits_for_test(
                &four_action_preservation,
                3,
                MAX_M6_PARTIAL_RERUN_WORKING_BYTES,
            ),
            Err(M6Error::Incomplete { .. })
        ));
        assert!(matches!(
            partial_rerun_working_bytes_v5(usize::MAX, usize::MAX, exact_working),
            Err(M6Error::Incomplete { .. })
        ));
    }

    #[test]
    fn staleness_dtos_are_strict_canonical_and_body_bound() {
        let (record, gluing, seal) = staleness_dto_fixture();
        let record_bytes = crate::canonical_json(&record).unwrap();
        let gluing_bytes = crate::canonical_json(&gluing).unwrap();
        let seal_bytes = crate::canonical_json(&seal).unwrap();
        assert_eq!(
            HistoricalRecordAssessmentV5::from_json_bytes(&record_bytes).unwrap(),
            record
        );
        assert_eq!(
            GluingFreshnessV5::from_json_bytes(&gluing_bytes).unwrap(),
            gluing
        );
        assert_eq!(
            StalenessAssessmentV5::from_json_bytes(&seal_bytes, &seal).unwrap(),
            seal
        );

        for bytes in [&record_bytes, &gluing_bytes, &seal_bytes] {
            let mut value: Value = serde_json::from_slice(bytes).unwrap();
            value
                .as_object_mut()
                .unwrap()
                .insert("unknown".to_owned(), Value::Bool(true));
            let tampered = crate::canonical_json(&value).unwrap();
            if bytes.as_slice() == record_bytes.as_slice() {
                assert!(HistoricalRecordAssessmentV5::from_json_bytes(&tampered).is_err());
            } else if bytes.as_slice() == gluing_bytes.as_slice() {
                assert!(GluingFreshnessV5::from_json_bytes(&tampered).is_err());
            } else {
                assert!(StalenessAssessmentV5::from_json_bytes(&tampered, &seal).is_err());
            }
        }

        let mut body_collision: Value = serde_json::from_slice(&record_bytes).unwrap();
        body_collision["source_record_body_hash"] = serde_json::json!(sha(99));
        assert!(
            HistoricalRecordAssessmentV5::from_json_bytes(
                &crate::canonical_json(&body_collision).unwrap()
            )
            .is_err()
        );
    }

    #[test]
    fn assessment_time_is_exact_utc_calendar_seconds() {
        for valid in [
            "2024-02-29T23:59:59Z",
            "2000-02-29T00:00:00Z",
            "2026-08-12T01:02:03Z",
        ] {
            validate_assessment_time_v5(valid).unwrap();
        }
        for invalid in [
            "2026-08-12T01:02:03.0Z",
            "2026-08-12t01:02:03Z",
            "2026-08-12T01:02:03+00:00",
            "2023-02-29T00:00:00Z",
            "1900-02-29T00:00:00Z",
            "2026-13-01T00:00:00Z",
            "2026-04-31T00:00:00Z",
            "2026-08-12T24:00:00Z",
            "2026-08-12T23:60:00Z",
            "2026-08-12T23:59:60Z",
        ] {
            assert!(validate_assessment_time_v5(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn mapping_status_position_table_is_closed_for_direct_indirect_and_both() {
        let statuses = [
            MappingStatusV5::Preserved,
            MappingStatusV5::Modified,
            MappingStatusV5::Added,
            MappingStatusV5::Removed,
            MappingStatusV5::Split,
            MappingStatusV5::Merged,
            MappingStatusV5::Unresolved,
        ];
        for status in statuses {
            for (direct, indirect, expected_directness) in [
                (true, false, StalenessDirectnessV5::Direct),
                (false, true, StalenessDirectnessV5::Indirect),
                (true, true, StalenessDirectnessV5::DirectAndIndirect),
            ] {
                let (reasons, actual_directness) = reduce_mapping_position_v5(
                    status,
                    direct,
                    indirect,
                    StaleReasonV5::ContextChanged,
                );
                if status == MappingStatusV5::Preserved {
                    assert!(reasons.is_empty());
                    assert_eq!(actual_directness, StalenessDirectnessV5::NotApplicable);
                    continue;
                }
                assert_eq!(actual_directness, expected_directness);
                if direct {
                    assert!(reasons.contains(&match status {
                        MappingStatusV5::Modified => StaleReasonV5::ContextChanged,
                        MappingStatusV5::Added | MappingStatusV5::Removed => {
                            StaleReasonV5::TargetChanged
                        }
                        MappingStatusV5::Split
                        | MappingStatusV5::Merged
                        | MappingStatusV5::Unresolved => StaleReasonV5::MappingUnresolved,
                        MappingStatusV5::Preserved => unreachable!(),
                    }));
                }
                if indirect {
                    assert!(reasons.contains(&match status {
                        MappingStatusV5::Modified
                        | MappingStatusV5::Added
                        | MappingStatusV5::Removed => StaleReasonV5::DependencyChanged,
                        MappingStatusV5::Split
                        | MappingStatusV5::Merged
                        | MappingStatusV5::Unresolved => StaleReasonV5::MappingUnresolved,
                        MappingStatusV5::Preserved => unreachable!(),
                    }));
                }
            }
        }
    }

    #[test]
    fn all_twenty_one_historical_kinds_have_the_exact_adr_order() {
        let kinds = [
            HistoricalSourceRecordKindV4::Obligation,
            HistoricalSourceRecordKindV4::ReviewPlan,
            HistoricalSourceRecordKindV4::ContextEnvelope,
            HistoricalSourceRecordKindV4::Execution,
            HistoricalSourceRecordKindV4::Claim,
            HistoricalSourceRecordKindV4::ClaimAssessment,
            HistoricalSourceRecordKindV4::ArtifactRegistrationV3,
            HistoricalSourceRecordKindV4::ArtifactRegistrationV4,
            HistoricalSourceRecordKindV4::Evidence,
            HistoricalSourceRecordKindV4::EvidenceBinding,
            HistoricalSourceRecordKindV4::Verification,
            HistoricalSourceRecordKindV4::Decision,
            HistoricalSourceRecordKindV4::Finding,
            HistoricalSourceRecordKindV4::GluingInputDescriptor,
            HistoricalSourceRecordKindV4::ContextCover,
            HistoricalSourceRecordKindV4::Section,
            HistoricalSourceRecordKindV4::Restriction,
            HistoricalSourceRecordKindV4::GluingAttempt,
            HistoricalSourceRecordKindV4::GlobalCandidate,
            HistoricalSourceRecordKindV4::GluingObstruction,
            HistoricalSourceRecordKindV4::Coverage,
        ];
        let public = kinds
            .into_iter()
            .map(HistoricalRecordKindV5::from_internal)
            .collect::<Vec<_>>();
        assert!(public.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(
            crate::canonical_json(&public).unwrap(),
            br#"["obligation","review_plan","context_envelope","execution","claim","claim_assessment","artifact_registration_v3","artifact_registration_v4","evidence","evidence_binding","verification","decision","finding","gluing_input_descriptor","context_cover","section","restriction","gluing_attempt","global_candidate","gluing_obstruction","coverage"]"#
        );
    }

    #[test]
    fn staleness_id_preimage_and_streaming_digests_have_literal_oracles() {
        let (record, gluing, seal) = staleness_dto_fixture();
        assert_eq!(
            seal.id().as_str(),
            "staleness-assessment-v5:sha256:23d190767a22ac7d18e0b3072c8433bb1a0100306c278a072fa623651dd1f2b9"
        );
        let record_oracle = ContentHash::sha256(
            &crate::canonical_json(&vec![
                IdBodyHashV5::new(record.id().clone(), record.body_hash().unwrap()).unwrap(),
            ])
            .unwrap(),
        );
        let gluing_oracle = ContentHash::sha256(
            &crate::canonical_json(&vec![
                IdBodyHashV5::new(gluing.id().clone(), gluing.body_hash().unwrap()).unwrap(),
            ])
            .unwrap(),
        );
        assert_eq!(seal.record_set_digest(), &record_oracle);
        assert_eq!(seal.gluing_freshness_set_digest(), &gluing_oracle);
    }

    #[test]
    fn cross_run_fixture_programs_are_accepted_v3_and_bind_exact_cas_and_git_continuity() {
        let (source, target, source_bytes, target_bytes) =
            m6_distinct_s0_s1_program_fixture().expect("accepted M6 Program fixtures");
        let source_git = source
            .accepted_git_revision_closure()
            .expect("source accepted Git closure");
        let target_git = target
            .accepted_git_revision_closure()
            .expect("target accepted Git closure");
        assert_eq!(source_git.target_commit_oid(), target_git.base_commit_oid());
        assert_eq!(source_git.target_tree_hash(), target_git.base_tree_hash());
        assert_ne!(target_git.base_commit_oid(), target_git.target_commit_oid());
        assert_eq!(source.repository_id(), target.repository_id());
        assert_ne!(source.snapshot_id(), target.snapshot_id());
        assert_eq!(source_bytes, target_bytes);
        for (program, bytes_by_id) in [(&source, &source_bytes), (&target, &target_bytes)] {
            let file_ids = program
                .artifacts()
                .iter()
                .filter(|artifact| artifact.kind == "file")
                .map(|artifact| artifact.id.clone())
                .collect::<BTreeSet<_>>();
            assert_eq!(file_ids, bytes_by_id.keys().cloned().collect());
            for artifact in program
                .artifacts()
                .iter()
                .filter(|artifact| artifact.kind == "file")
            {
                assert_eq!(
                    artifact.content_hash.as_ref(),
                    Some(&ContentHash::sha256(&bytes_by_id[&artifact.id]))
                );
            }
        }
        ChangeMorphismV5::mapping_reservation_bytes_from_accepted_program_facts(&source, &target)
            .expect("accepted mapping facts");
    }

    fn replace_string(value: &mut Value, from: &str, to: &str) {
        match value {
            Value::String(value) if value == from => *value = to.to_owned(),
            Value::Array(values) => {
                for value in values {
                    replace_string(value, from, to);
                }
            }
            Value::Object(values) => {
                for value in values.values_mut() {
                    replace_string(value, from, to);
                }
            }
            _ => {}
        }
    }

    fn spaces() -> (ProgramSpace, ProgramSpace) {
        let mut source: Value = serde_json::from_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        source["snapshot"]["base_revision"] = Value::String(oid(9));
        source["snapshot"]["target_revision"] = Value::String(oid(10));
        source["snapshot"]["tree_hash"] = Value::String(tree(11).to_string());
        for artifact in source["artifacts"].as_array_mut().unwrap() {
            if artifact["id"] == "file:checkout-controller" {
                artifact["content_hash"] = Value::String(sha(44).to_string());
            }
        }
        let source = ProgramSpace::from_json_slice(&serde_json::to_vec(&source).unwrap()).unwrap();
        let target = distinct_s1_program_from(&source).unwrap();
        (source, target)
    }

    fn spaces_with_relation_chain(length: usize) -> (ProgramSpace, ProgramSpace) {
        let (source, target) = spaces();
        let mut source_value: Value =
            serde_json::from_slice(&crate::canonical_json(&source.streaming_ref()).unwrap())
                .unwrap();
        let mut target_value: Value =
            serde_json::from_slice(&crate::canonical_json(&target.streaming_ref()).unwrap())
                .unwrap();
        let template = source_value["relations"][0].clone();
        for index in 0..length {
            let relation_id = format!("relation:chain-{index:04}");
            let target_id = if index == 0 {
                "file:checkout-controller".to_owned()
            } else {
                format!("relation:chain-{:04}", index - 1)
            };
            let mut relation = template.clone();
            relation["id"] = Value::String(relation_id);
            relation["kind"] = Value::String("dependency_chain".to_owned());
            relation["source_id"] = Value::String("file:checkout-controller".to_owned());
            relation["target_ids"] = serde_json::json!([target_id]);
            relation["directed"] = Value::Bool(true);
            relation["attributes"] = serde_json::json!({});
            source_value["relations"]
                .as_array_mut()
                .unwrap()
                .push(relation.clone());
            target_value["relations"]
                .as_array_mut()
                .unwrap()
                .push(relation);
        }
        (
            ProgramSpace::from_json_slice(&serde_json::to_vec(&source_value).unwrap()).unwrap(),
            ProgramSpace::from_json_slice(&serde_json::to_vec(&target_value).unwrap()).unwrap(),
        )
    }

    fn spaces_with_mutual_relation_scc() -> (ProgramSpace, ProgramSpace) {
        let (source, target) = spaces();
        let mut source_value: Value =
            serde_json::from_slice(&crate::canonical_json(&source.streaming_ref()).unwrap())
                .unwrap();
        let mut target_value: Value =
            serde_json::from_slice(&crate::canonical_json(&target.streaming_ref()).unwrap())
                .unwrap();
        let template = source_value["relations"][0].clone();
        for (id_value, target_id) in [
            ("relation:cycle-a", "relation:cycle-b"),
            ("relation:cycle-b", "relation:cycle-a"),
        ] {
            let mut relation = template.clone();
            relation["id"] = Value::String(id_value.to_owned());
            relation["kind"] = Value::String("mutual_dependency".to_owned());
            relation["source_id"] = Value::String("file:checkout-controller".to_owned());
            relation["target_ids"] = serde_json::json!([target_id]);
            relation["directed"] = Value::Bool(true);
            relation["attributes"] = serde_json::json!({});
            source_value["relations"]
                .as_array_mut()
                .unwrap()
                .push(relation.clone());
            target_value["relations"]
                .as_array_mut()
                .unwrap()
                .push(relation);
        }
        (
            ProgramSpace::from_json_slice(&serde_json::to_vec(&source_value).unwrap()).unwrap(),
            ProgramSpace::from_json_slice(&serde_json::to_vec(&target_value).unwrap()).unwrap(),
        )
    }

    fn spaces_with_two_disjoint_relation_sccs() -> (ProgramSpace, ProgramSpace) {
        let (source, target) = spaces_with_mutual_relation_scc();
        let mut source_value: Value =
            serde_json::from_slice(&crate::canonical_json(&source.streaming_ref()).unwrap())
                .unwrap();
        let mut target_value: Value =
            serde_json::from_slice(&crate::canonical_json(&target.streaming_ref()).unwrap())
                .unwrap();
        let template = source_value["relations"][0].clone();
        for (id_value, target_id) in [
            ("relation:cycle-c", "relation:cycle-d"),
            ("relation:cycle-d", "relation:cycle-c"),
        ] {
            let mut relation = template.clone();
            relation["id"] = Value::String(id_value.to_owned());
            relation["kind"] = Value::String("mutual_dependency".to_owned());
            relation["source_id"] = Value::String("file:payment-repository".to_owned());
            relation["target_ids"] = serde_json::json!([target_id]);
            relation["directed"] = Value::Bool(true);
            relation["attributes"] = serde_json::json!({});
            source_value["relations"]
                .as_array_mut()
                .unwrap()
                .push(relation.clone());
            target_value["relations"]
                .as_array_mut()
                .unwrap()
                .push(relation);
        }
        (
            ProgramSpace::from_json_slice(&serde_json::to_vec(&source_value).unwrap()).unwrap(),
            ProgramSpace::from_json_slice(&serde_json::to_vec(&target_value).unwrap()).unwrap(),
        )
    }

    fn closure_input(source: &ProgramSpace, target: &ProgramSpace) -> IncrementalStructuralInputV5 {
        IncrementalStructuralInputV5 {
            repository_id: source.repository_id().clone(),
            repository_identity_hash: sha(1),
            source_run_id: id("run:source"),
            source_genesis_hash: sha(2),
            source_confirmed_offset: 100,
            source_tail_hash: sha(3),
            source_event_count: 10,
            source_snapshot_id: source.snapshot_id().clone(),
            source_universe_id: id("universe:source"),
            source_index_snapshot_hash: sha(4),
            source_authority_policy_revision_hash: sha(5),
            source_authority_replay_basis_digest: sha(6),
            source_resolved_target_commit_oid: oid(10),
            source_target_tree_hash: tree(11),
            source_gluing_bundle_id: id("event:source-gluing-bundle"),
            target_run_id: id("run:target"),
            target_genesis_hash: sha(7),
            target_predecessor_offset: 200,
            target_predecessor_tail_hash: sha(8),
            target_predecessor_event_count: 20,
            target_snapshot_id: target.snapshot_id().clone(),
            target_universe_id: id("universe:target"),
            target_predecessor_index_snapshot_hash: sha(9),
            target_authority_policy_revision_hash: sha(10),
            target_pre_incremental_authority_replay_basis_digest: sha(11),
            target_resolved_base_commit_oid: oid(10),
            target_base_tree_hash: tree(11),
            target_resolved_target_commit_oid: oid(12),
            target_target_tree_hash: tree(13),
        }
    }

    fn closure(source: &ProgramSpace, target: &ProgramSpace) -> IncrementalSourceClosureV5 {
        let proof = ValidatedIncrementalStructureV5::validate_store_projection(
            source,
            target,
            closure_input(source, target),
        )
        .unwrap();
        IncrementalSourceClosureV5::from_validated_structure(proof).unwrap()
    }

    fn anchors(space: &ProgramSpace) -> BTreeMap<StableId, RustSymbolAnchorV1> {
        space
            .artifacts()
            .iter()
            .filter(|artifact| rust_symbol(artifact))
            .enumerate()
            .map(|(index, artifact)| {
                (
                    artifact.id.clone(),
                    RustSymbolAnchorV1::new(
                        match artifact.kind.as_str() {
                            "method" => RustSymbolKindV1::Method,
                            "type" => RustSymbolKindV1::Type,
                            _ => RustSymbolKindV1::Function,
                        },
                        sha(100 + index),
                        sha(200 + index),
                    )
                    .unwrap(),
                )
            })
            .collect()
    }

    fn orders(space: &ProgramSpace) -> BTreeMap<StableId, Vec<StableId>> {
        space
            .relations()
            .iter()
            .map(|relation| {
                (
                    relation.id.clone(),
                    relation.target_ids.iter().cloned().collect(),
                )
            })
            .collect()
    }

    fn inputs(source: &ProgramSpace, target: &ProgramSpace) -> ValidatedIncrementalInputsV5 {
        ValidatedIncrementalInputsV5::fixture_from_parts(
            source,
            target,
            anchors(source),
            anchors(target),
            orders(source),
            orders(target),
            vec![],
        )
        .unwrap()
    }

    fn phase() -> (IncrementalSourceClosureV5, M6MappingPhaseV5) {
        let (source, target) = spaces();
        let closure = closure(&source, &target);
        let phase = ChangeMorphismV5::build_with_inputs(
            &closure,
            &source,
            &target,
            &inputs(&source, &target),
        )
        .unwrap();
        (closure, phase)
    }

    fn accepted_aggregate(program: ProgramSpace) -> ReviewAggregate {
        let (universe, obligations) = crate::MvpRulePack::synthesize(&program)
            .unwrap()
            .into_parts();
        ReviewAggregate::new(program, universe, obligations).unwrap()
    }

    fn correspondence_phase() -> (
        IncrementalSourceClosureV5,
        M6MappingPhaseV5,
        ReviewAggregate,
        ReviewAggregate,
        M6ObligationCorrespondencePhaseV5,
    ) {
        let (source_program, target_program) = spaces();
        let source = accepted_aggregate(source_program);
        let target = accepted_aggregate(target_program);
        let mut input = closure_input(source.program(), target.program());
        input.source_universe_id = source.universe().id().clone();
        input.target_universe_id = target.universe().id().clone();
        let proof = ValidatedIncrementalStructureV5::validate_store_projection(
            source.program(),
            target.program(),
            input,
        )
        .unwrap();
        let closure = IncrementalSourceClosureV5::from_validated_structure(proof).unwrap();
        let mappings = ChangeMorphismV5::build_with_inputs(
            &closure,
            source.program(),
            target.program(),
            &inputs(source.program(), target.program()),
        )
        .unwrap();
        let correspondence = ObligationCorrespondenceV5::derive_from_accepted_universes(
            &closure, &mappings, &source, &target,
        )
        .unwrap();
        (closure, mappings, source, target, correspondence)
    }

    #[test]
    fn structural_projection_has_no_boolean_authority_and_fails_closed() {
        let (source, target) = spaces();
        assert!(
            ValidatedIncrementalStructureV5::validate_store_projection(
                &source,
                &target,
                closure_input(&source, &target),
            )
            .is_ok()
        );
        let mut input = closure_input(&source, &target);
        input.source_gluing_bundle_id = id("gluing-attempt-v4:wrong");
        assert!(matches!(
            ValidatedIncrementalStructureV5::validate_store_projection(&source, &target, input),
            Err(M6Error::InvalidSourceClosure(_))
        ));
        let closure = closure(&source, &target);
        assert_eq!(closure.id().kind(), "incremental-source-closure-v5");
        assert_eq!(
            closure.input().source_target_tree_hash,
            closure.input().target_base_tree_hash
        );
    }

    #[test]
    fn builder_derives_complete_domains_statuses_and_stable_ids() {
        let (source, target) = spaces();
        let closure = closure(&source, &target);
        assert!(matches!(
            ChangeMorphismV5::derive_from_accepted_program_facts(&closure, &source, &target),
            Err(M6Error::MissingAcceptedMappingFact { .. })
        ));
        let phase = ChangeMorphismV5::build_with_inputs(
            &closure,
            &source,
            &target,
            &inputs(&source, &target),
        )
        .unwrap();
        assert_eq!(
            phase.morphism().source_domain_count(),
            source.known_ids().len() as u64
        );
        assert_eq!(
            phase.morphism().target_domain_count(),
            target.known_ids().len() as u64
        );
        assert_eq!(
            phase.morphism().mapping_count(),
            source.known_ids().len() as u64
        );
        assert_eq!(phase.morphism().status_counts().modified, 1);
        assert_eq!(
            phase.morphism().status_counts().preserved + 1,
            source.known_ids().len() as u64
        );
        assert_eq!(
            phase.morphism().source_ids(),
            &BTreeSet::from([closure.id().clone()])
        );
        assert_eq!(phase.morphism().id().kind(), "change-morphism-v5");
        assert!(phase.working_peak_upper_bound_bytes() > 0);
        assert!(phase.working_peak_upper_bound_bytes() <= MAX_M6_MAPPING_WORKING_BYTES);
        assert_eq!(
            phase
                .mappings()
                .iter()
                .find(|mapping| mapping.object_kind() == ProgramObjectKindV5::Snapshot)
                .unwrap()
                .status(),
            MappingStatusV5::Modified
        );
    }

    #[test]
    fn mapping_domain_binding_uses_streaming_program_fact_set_not_snapshot_id() {
        let (source, target) = spaces();
        let closure = closure(&source, &target);
        let mapping = ChangeMorphismV5::build_with_inputs(
            &closure,
            &source,
            &target,
            &inputs(&source, &target),
        )
        .unwrap();
        let (count, digest) = target.m6_known_id_domain().unwrap();
        assert_eq!(count, target.known_ids().len());
        assert_eq!(digest, *mapping.morphism().target_domain_digest());

        // Keep the accepted snapshot fields byte-for-byte identical while
        // changing one accepted fact ID.  This is the mixed-domain case that
        // snapshot-only phase validation would silently admit.
        let mut foreign: Value =
            serde_json::from_slice(&crate::canonical_json(&target.streaming_ref()).unwrap())
                .unwrap();
        foreign["relations"][0]["id"] = Value::String("relation:foreign-domain".to_owned());
        let foreign =
            ProgramSpace::from_json_slice(&serde_json::to_vec(&foreign).unwrap()).unwrap();
        assert_eq!(foreign.snapshot_id(), target.snapshot_id());
        assert!(matches!(
            validate_mapping_program_domains(&mapping, &source, &foreign),
            Err(M6Error::InvalidHistoricalTopology(
                "mapping ProgramSpace domain does not equal the pinned source/target facts"
            ))
        ));
    }

    #[test]
    fn staleness_phase_retained_oracle_charges_outer_and_body_vector_capacity() {
        let (_closure, mut mappings, _source, _target, mut correspondence) = correspondence_phase();
        let before_mapping_capacity = mappings.mappings.capacity();
        let before_entry_capacity = correspondence.entries.capacity();
        let before =
            staleness_mapping_correspondence_retained_bytes(&mappings, &correspondence).unwrap();
        mappings.mappings.reserve(32);
        correspondence.entries.reserve(32);
        let after_mapping_capacity = mappings.mappings.capacity();
        let after_entry_capacity = correspondence.entries.capacity();
        let after =
            staleness_mapping_correspondence_retained_bytes(&mappings, &correspondence).unwrap();
        assert!(after_mapping_capacity > before_mapping_capacity);
        assert!(after_entry_capacity > before_entry_capacity);
        assert_eq!(
            after.0 - before.0,
            (after_mapping_capacity - before_mapping_capacity)
                * std::mem::size_of::<ProgramMappingV5>()
        );
        assert_eq!(
            after.1 - before.1,
            (after_entry_capacity - before_entry_capacity)
                * std::mem::size_of::<ObligationCorrespondenceEntryV5>()
        );

        let mapping = mappings.mappings.first_mut().unwrap();
        let before = mapping.allocated_bytes();
        let before_capacity = mapping.source_body_hashes.capacity();
        mapping.source_body_hashes.reserve(32);
        let after_capacity = mapping.source_body_hashes.capacity();
        let after = mapping.allocated_bytes();
        assert!(after_capacity > before_capacity);
        assert_eq!(
            after - before,
            (after_capacity - before_capacity) * std::mem::size_of::<IdBodyHashV5>()
        );

        let entry = correspondence.entries.first_mut().unwrap();
        let before = entry.allocated_bytes();
        let before_capacity = entry.target_body_hashes.capacity();
        entry.target_body_hashes.reserve(32);
        let after_capacity = entry.target_body_hashes.capacity();
        let after = entry.allocated_bytes();
        assert!(after_capacity > before_capacity);
        assert_eq!(
            after - before,
            (after_capacity - before_capacity) * std::mem::size_of::<IdBodyHashV5>()
        );

        correspondence.entries = Vec::with_capacity(17);
        let (_, empty_correspondence) =
            staleness_mapping_correspondence_retained_bytes(&mappings, &correspondence).unwrap();
        assert_eq!(
            empty_correspondence,
            correspondence.correspondence.allocated_bytes()
                + correspondence.entries.capacity()
                    * std::mem::size_of::<ObligationCorrespondenceEntryV5>()
        );

        let (record, gluing, assessment) = staleness_dto_fixture();
        let phase = M6StalenessPhaseV5 {
            records: vec![record],
            gluing_freshness: vec![gluing],
            assessment,
            preservation_candidate_obligation_ids: BTreeSet::from([id("obligation:target")]),
            target_gluing_required: true,
            m5_dependent_successor_obligation_ids: BTreeSet::from([id("obligation:target")]),
            partial_rerun_seeds: BTreeMap::new(),
            target_plan_id: id("plan:target"),
            target_snapshot_id: id("snapshot:target"),
            target_run_id: id("run:target"),
            target_genesis_hash: sha(91),
            target_tail_hash: sha(92),
            target_event_count: 9,
            target_policy_revision_hash: sha(93),
            target_pre_incremental_basis_digest: sha(94),
            working_bytes: 123,
        };
        assert!(phase.retained_bytes() >= std::mem::size_of::<M6StalenessPhaseV5>());
        assert_eq!(phase.working_bytes(), 123);
    }

    #[test]
    fn correspondence_derives_complete_preserved_domains_and_exact_traces() {
        let (_closure, mappings, source, target, phase) = correspondence_phase();
        assert_eq!(
            phase.correspondence().source_domain_count(),
            source.universe().raw_denominator() as u64
        );
        assert_eq!(
            phase.correspondence().target_domain_count(),
            target.universe().raw_denominator() as u64
        );
        assert_eq!(
            phase.correspondence().entry_count(),
            phase.entries().len() as u64
        );
        assert_eq!(
            phase.correspondence().status_counts().preserved,
            source.universe().raw_denominator() as u64
        );
        assert_eq!(
            phase.correspondence().source_ids(),
            &BTreeSet::from([
                mappings.morphism().id().clone(),
                source.universe().id().clone(),
                target.universe().id().clone(),
            ])
        );
        let source_domain = phase
            .entries()
            .iter()
            .flat_map(|entry| entry.from_obligation_ids().iter().cloned())
            .collect::<BTreeSet<_>>();
        let target_domain = phase
            .entries()
            .iter()
            .flat_map(|entry| entry.to_obligation_ids().iter().cloned())
            .collect::<BTreeSet<_>>();
        assert_eq!(source_domain, *source.universe().obligation_ids());
        assert_eq!(target_domain, *target.universe().obligation_ids());
        for entry in phase.entries() {
            let expected_sources = std::iter::once(mappings.morphism().id().clone())
                .chain(entry.source_mapping_ids().iter().cloned())
                .chain(entry.predecessor_entry_ids().iter().cloned())
                .chain(entry.from_obligation_ids().iter().cloned())
                .chain(entry.to_obligation_ids().iter().cloned())
                .collect::<BTreeSet<_>>();
            assert_eq!(entry.source_ids(), &expected_sources);
            assert_eq!(entry.successor_obligation_ids(), entry.to_obligation_ids());
        }
        assert!(phase.working_peak_upper_bound_bytes() <= MAX_M6_CORRESPONDENCE_WORKING_BYTES);
    }

    #[test]
    fn correspondence_wire_is_strict_canonical_and_recomputed_byte_for_byte() {
        let (closure, mappings, source, target, phase) = correspondence_phase();
        let entry_bytes = phase
            .entries()
            .iter()
            .map(|entry| crate::canonical_json(entry).unwrap())
            .collect::<Vec<_>>();
        let seal_bytes = crate::canonical_json(phase.correspondence()).unwrap();
        phase
            .validate_replayed_canonical(&entry_bytes, &seal_bytes)
            .unwrap();
        let repeated = ObligationCorrespondenceV5::derive_from_accepted_universes(
            &closure, &mappings, &source, &target,
        )
        .unwrap();
        assert_eq!(repeated.entries(), phase.entries());
        assert_eq!(repeated.correspondence(), phase.correspondence());

        let mut tampered: Value = serde_json::from_slice(&entry_bytes[0]).unwrap();
        tampered["successor_obligation_ids"] = serde_json::json!([]);
        assert!(
            ObligationCorrespondenceEntryV5::from_json_bytes(
                &crate::canonical_json(&tampered).unwrap()
            )
            .is_err()
        );
        let mut unknown: Value = serde_json::from_slice(&entry_bytes[0]).unwrap();
        unknown["caller_authority"] = Value::Bool(true);
        assert!(
            ObligationCorrespondenceEntryV5::from_json_bytes(
                &crate::canonical_json(&unknown).unwrap()
            )
            .is_err()
        );
        let mut seal_tamper: Value = serde_json::from_slice(&seal_bytes).unwrap();
        seal_tamper["entry_count"] = serde_json::json!(999);
        assert!(
            ObligationCorrespondenceV5::from_json_bytes(
                &crate::canonical_json(&seal_tamper).unwrap(),
                phase.correspondence(),
            )
            .is_err()
        );
        let original = &phase.entries()[0];
        let mut trace_only_tamper = original.clone();
        trace_only_tamper.successor_obligation_ids.clear();
        assert_eq!(trace_only_tamper.id(), original.id());
        assert_ne!(
            trace_only_tamper.body_hash().unwrap(),
            original.body_hash().unwrap(),
            "successors are excluded from identity but protected by the complete body hash"
        );
        let mut identity_change = ObligationCorrespondenceEntryPartsV5 {
            morphism_id: original.morphism_id.clone(),
            from_obligation_ids: original.from_obligation_ids.clone(),
            to_obligation_ids: original.to_obligation_ids.clone(),
            status: MappingStatusV5::Modified,
            source_mapping_ids: original.source_mapping_ids.clone(),
            predecessor_entry_ids: original.predecessor_entry_ids.clone(),
            source_body_hashes: original.source_body_hashes.clone(),
            target_body_hashes: original.target_body_hashes.clone(),
        };
        if original.status() == MappingStatusV5::Modified {
            identity_change.status = MappingStatusV5::Preserved;
        }
        let changed_identity =
            ObligationCorrespondenceEntryV5::from_parts(identity_change).unwrap();
        assert_ne!(changed_identity.id(), original.id());
    }

    #[test]
    fn obligation_dependency_cycles_and_external_ids_are_typed_invalid_universe() {
        let (_closure, _mappings, source, _target, _phase) = correspondence_phase();
        let template = source.obligations().next().unwrap();
        let make = |id_value: &str, dependencies: &[&str]| {
            let mut value = serde_json::to_value(template).unwrap();
            value["id"] = Value::String(id_value.to_owned());
            value["depends_on"] = serde_json::json!(dependencies);
            value["normalized_depends_on"] = serde_json::json!(dependencies);
            serde_json::from_value::<Obligation>(value).unwrap()
        };
        let cyclic = [
            make("obligation:cycle-a", &["obligation:cycle-b"]),
            make("obligation:cycle-b", &["obligation:cycle-a"]),
        ];
        let cyclic_map = cyclic
            .iter()
            .map(|obligation| (obligation.id().clone(), obligation))
            .collect::<BTreeMap<_, _>>();
        assert!(matches!(
            validate_obligation_dependency_dag(&cyclic_map),
            Err(M6Error::InvalidObligationUniverse(
                "obligation dependency graph contains a cycle"
            ))
        ));

        let external = [make(
            "obligation:external-owner",
            &["obligation:outside-universe"],
        )];
        let external_map = external
            .iter()
            .map(|obligation| (obligation.id().clone(), obligation))
            .collect::<BTreeMap<_, _>>();
        assert!(matches!(
            validate_obligation_dependency_dag(&external_map),
            Err(M6Error::InvalidObligationUniverse(
                "obligation dependency is outside its accepted universe"
            ))
        ));
    }

    #[test]
    fn correspondence_rejects_constructor_valid_but_non_synthesized_obligation_bodies() {
        let (closure, mappings, source, target, _phase) = correspondence_phase();
        let mut obligations = target.obligations().cloned().collect::<Vec<_>>();
        let mut value = serde_json::to_value(&obligations[0]).unwrap();
        value["weight"] = serde_json::json!(99.0);
        obligations[0] = serde_json::from_value(value).unwrap();
        let forged = ReviewAggregate::new(
            target.program().clone(),
            target.universe().clone(),
            obligations,
        )
        .unwrap();
        assert!(matches!(
            ObligationCorrespondenceV5::derive_from_accepted_universes(
                &closure, &mappings, &source, &forged,
            ),
            Err(M6Error::InvalidObligationUniverse(
                "accepted obligation body differs from deterministic rule-pack synthesis"
            ))
        ));
    }

    #[test]
    fn correspondence_rejects_reordered_path_targets_before_cross_snapshot_normalization() {
        let (closure, mappings, source, target, _phase) = correspondence_phase();
        let mut obligations = target.obligations().cloned().collect::<Vec<_>>();
        let path_index = obligations
            .iter()
            .position(|obligation| obligation.target_refs().len() > 1)
            .expect("reference scenario must contain an ordered path obligation");
        let original_id = obligations[path_index].id().clone();
        let mut value = serde_json::to_value(&obligations[path_index]).unwrap();
        value["target_refs"].as_array_mut().unwrap().reverse();
        obligations[path_index] = serde_json::from_value(value).unwrap();
        assert_eq!(obligations[path_index].id(), &original_id);
        assert_eq!(
            obligations[path_index].normalized_target_refs(),
            target
                .obligations()
                .find(|obligation| obligation.id() == &original_id)
                .unwrap()
                .normalized_target_refs()
        );
        let forged = ReviewAggregate::new(
            target.program().clone(),
            target.universe().clone(),
            obligations,
        )
        .unwrap();
        assert!(matches!(
            ObligationCorrespondenceV5::derive_from_accepted_universes(
                &closure, &mappings, &source, &forged,
            ),
            Err(M6Error::InvalidObligationUniverse(
                "accepted obligation body differs from deterministic rule-pack synthesis"
            ))
        ));
    }

    #[test]
    fn deterministic_program_body_change_is_modified_and_policy_change_rebinds_seal() {
        let (_baseline_closure, _baseline_mappings, _source, _target, baseline) =
            correspondence_phase();
        let (source_program, target_program) = spaces();
        let mut target_value: Value = serde_json::from_slice(
            &crate::canonical_json(&target_program.streaming_ref()).unwrap(),
        )
        .unwrap();
        target_value["artifacts"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|artifact| artifact["id"] == "function:checkout-submit")
            .unwrap()["attributes"]["body_revision"] = Value::String("2".to_owned());
        target_value["profile"]["policy_version"] = Value::String("default@2".to_owned());
        let target_program =
            ProgramSpace::from_json_slice(&serde_json::to_vec(&target_value).unwrap()).unwrap();
        let source = accepted_aggregate(source_program);
        let target = accepted_aggregate(target_program);
        let mut input = closure_input(source.program(), target.program());
        input.source_universe_id = source.universe().id().clone();
        input.target_universe_id = target.universe().id().clone();
        let proof = ValidatedIncrementalStructureV5::validate_store_projection(
            source.program(),
            target.program(),
            input,
        )
        .unwrap();
        let closure = IncrementalSourceClosureV5::from_validated_structure(proof).unwrap();
        let mappings = ChangeMorphismV5::build_with_inputs(
            &closure,
            source.program(),
            target.program(),
            &inputs(source.program(), target.program()),
        )
        .unwrap();
        let changed = ObligationCorrespondenceV5::derive_from_accepted_universes(
            &closure, &mappings, &source, &target,
        )
        .unwrap();
        assert!(changed.correspondence().status_counts().modified > 0);
        assert_ne!(
            changed.correspondence().target_universe_id(),
            baseline.correspondence().target_universe_id()
        );
        assert_ne!(
            changed.correspondence().id(),
            baseline.correspondence().id()
        );
    }

    #[test]
    fn correspondence_dependency_stages_cross_255_without_truncation() {
        let (_closure, _mappings, source, _target, _phase) = correspondence_phase();
        let template = source.obligations().next().unwrap();
        let obligations = (0..300)
            .map(|index| {
                let mut value = serde_json::to_value(template).unwrap();
                let obligation_id = format!("obligation:chain-{index:04}");
                let dependencies = if index == 0 {
                    Vec::<String>::new()
                } else {
                    vec![format!("obligation:chain-{:04}", index - 1)]
                };
                value["id"] = Value::String(obligation_id);
                value["depends_on"] = serde_json::json!(dependencies);
                value["normalized_depends_on"] = serde_json::json!(dependencies);
                serde_json::from_value::<Obligation>(value).unwrap()
            })
            .collect::<Vec<_>>();
        let obligation_map = obligations
            .iter()
            .map(|obligation| (obligation.id().clone(), obligation))
            .collect::<BTreeMap<_, _>>();
        let depths = validate_obligation_dependency_dag(&obligation_map).unwrap();
        assert_eq!(depths[&id("obligation:chain-0299")], 299);
        let seeds = obligations
            .iter()
            .map(|obligation| ObligationComponentSeedV5 {
                from_ids: BTreeSet::from([obligation.id().clone()]),
                to_ids: BTreeSet::from([obligation.id().clone()]),
                stage: 0,
            })
            .collect();
        let staged =
            collapse_and_stage_obligation_components(seeds, &obligation_map, &obligation_map)
                .unwrap();
        assert_eq!(staged.last().unwrap().stage, 299);
    }

    #[test]
    fn correspondence_entries_close_split_merge_ambiguity_and_all_status_counts() {
        let (_closure, mappings, source, target, _phase) = correspondence_phase();
        let mut serial = 1_usize;
        fn next_ids(side: &str, count: usize, serial: &mut usize) -> BTreeSet<StableId> {
            (0..count)
                .map(|_| {
                    let value = id(&format!("obligation:{side}-{:02}", *serial));
                    *serial += 1;
                    value
                })
                .collect::<BTreeSet<_>>()
        }
        let shapes = [
            (MappingStatusV5::Preserved, 1, 1),
            (MappingStatusV5::Modified, 1, 1),
            (MappingStatusV5::Added, 0, 1),
            (MappingStatusV5::Removed, 1, 0),
            (MappingStatusV5::Split, 1, 2),
            (MappingStatusV5::Merged, 2, 1),
            (MappingStatusV5::Unresolved, 2, 2),
        ];
        let mut entries = shapes
            .into_iter()
            .map(|(status, from_count, to_count)| {
                let from_ids = next_ids("source", from_count, &mut serial);
                let to_ids = next_ids("target", to_count, &mut serial);
                let source_body_hashes = from_ids
                    .iter()
                    .map(|id| IdBodyHashV5::new(id.clone(), sha(serial)).unwrap())
                    .collect();
                serial += 1;
                let target_body_hashes = to_ids
                    .iter()
                    .map(|id| IdBodyHashV5::new(id.clone(), sha(serial)).unwrap())
                    .collect();
                serial += 1;
                ObligationCorrespondenceEntryV5::from_parts(ObligationCorrespondenceEntryPartsV5 {
                    morphism_id: mappings.morphism().id().clone(),
                    from_obligation_ids: from_ids,
                    to_obligation_ids: to_ids,
                    status,
                    source_mapping_ids: BTreeSet::new(),
                    predecessor_entry_ids: BTreeSet::new(),
                    source_body_hashes,
                    target_body_hashes,
                })
                .unwrap()
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.id().cmp(right.id()));
        let source_domain = entries
            .iter()
            .flat_map(|entry| entry.from_obligation_ids().iter().cloned())
            .collect::<BTreeSet<_>>();
        let target_domain = entries
            .iter()
            .flat_map(|entry| entry.to_obligation_ids().iter().cloned())
            .collect::<BTreeSet<_>>();
        let seal = ObligationCorrespondenceV5::seal_derived(
            mappings.morphism(),
            source.universe().id(),
            target.universe().id(),
            &entries,
            &source_domain,
            &target_domain,
        )
        .unwrap();
        assert_eq!(
            seal.status_counts(),
            &MappingStatusCountsV5 {
                preserved: 1,
                modified: 1,
                added: 1,
                removed: 1,
                split: 1,
                merged: 1,
                unresolved: 1,
            }
        );
        let mut incomplete_source_domain = source_domain.clone();
        incomplete_source_domain.pop_first();
        assert!(matches!(
            ObligationCorrespondenceV5::seal_derived(
                mappings.morphism(),
                source.universe().id(),
                target.universe().id(),
                &entries,
                &incomplete_source_domain,
                &target_domain,
            ),
            Err(M6Error::InvalidObligationUniverse(_))
        ));
    }

    #[test]
    fn obligation_candidate_key_is_sensitive_to_every_adr_field() {
        let (_closure, mappings, _source, target, _phase) = correspondence_phase();
        let obligation = target
            .obligations()
            .find(|obligation| !obligation.normalized_context_ids().is_empty())
            .unwrap();
        let successors = mappings
            .mappings()
            .iter()
            .flat_map(|mapping| {
                mapping
                    .from_ids()
                    .iter()
                    .cloned()
                    .map(move |id| (id, mapping.to_ids().clone()))
            })
            .collect::<BTreeMap<_, _>>();
        let target_domain = target.program().known_ids();
        let base = obligation_candidate_key(
            obligation,
            ObligationSideV5::Target,
            &target_domain,
            &successors,
        )
        .unwrap();
        for mutation in [
            "rule",
            "property_id",
            "property_version",
            "target_kind",
            "semantic_key",
            "target_refs",
            "context_ids",
            "generator_ids",
        ] {
            let mut value = serde_json::to_value(obligation).unwrap();
            match mutation {
                "rule" => value["version"]["rule"] = Value::String("changed.rule@2".to_owned()),
                "property_id" => {
                    value["property_id"] = Value::String("changed.property".to_owned())
                }
                "property_version" => value["property_version"] = Value::String("2".to_owned()),
                "target_kind" => value["target_kind"] = Value::String("changed_kind".to_owned()),
                "semantic_key" => value["semantic_key"] = Value::String("changed|key".to_owned()),
                "target_refs" => {
                    value["target_refs"] = serde_json::json!(["file:checkout-controller"]);
                    value["normalized_target_refs"] =
                        serde_json::json!(["file:checkout-controller"]);
                }
                "context_ids" => {
                    value["context_ids"] = serde_json::json!([]);
                    value["normalized_context_ids"] = serde_json::json!([]);
                }
                "generator_ids" => {
                    value["generator_ids"] = serde_json::json!(["file:checkout-controller"]);
                }
                _ => unreachable!(),
            }
            let changed: Obligation = serde_json::from_value(value).unwrap();
            let key = obligation_candidate_key(
                &changed,
                ObligationSideV5::Target,
                &target_domain,
                &successors,
            )
            .unwrap();
            assert_ne!(base, key, "candidate field {mutation} must be semantic");
        }
    }

    #[test]
    fn source_only_change_stays_one_candidate_and_is_modified_by_complete_body() {
        let (_closure, mappings, _source, target, _phase) = correspondence_phase();
        let obligation = target.obligations().next().unwrap();
        let target_domain = target.program().known_ids();
        let obligation_domain = target
            .obligations()
            .map(|obligation| obligation.id().clone())
            .collect::<BTreeSet<_>>();
        let successors = mappings
            .mappings()
            .iter()
            .flat_map(|mapping| {
                mapping
                    .from_ids()
                    .iter()
                    .cloned()
                    .map(move |id| (id, mapping.to_ids().clone()))
            })
            .collect::<BTreeMap<_, _>>();
        let replacement = target_domain
            .iter()
            .find(|id| !obligation.normalized_source_ids().contains(*id))
            .unwrap()
            .clone();
        let mut value = serde_json::to_value(obligation).unwrap();
        value["source_ids"] = serde_json::json!([replacement]);
        value["normalized_source_ids"] = value["source_ids"].clone();
        let changed: Obligation = serde_json::from_value(value).unwrap();

        let base_key = obligation_candidate_key(
            obligation,
            ObligationSideV5::Target,
            &target_domain,
            &successors,
        )
        .unwrap();
        let changed_key = obligation_candidate_key(
            &changed,
            ObligationSideV5::Target,
            &target_domain,
            &successors,
        )
        .unwrap();
        assert_eq!(
            base_key, changed_key,
            "source IDs are a complete-body equality field, not a quotient candidate key"
        );
        let base_hash = normalized_obligation_body_hash(
            obligation,
            ObligationSideV5::Target,
            &target_domain,
            &obligation_domain,
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .unwrap();
        let changed_hash = normalized_obligation_body_hash(
            &changed,
            ObligationSideV5::Target,
            &target_domain,
            &obligation_domain,
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .unwrap();
        assert_ne!(
            base_hash, changed_hash,
            "a one-to-one source-only change must classify Modified, never Preserved"
        );
        assert_eq!(
            one_to_one_obligation_status(true, true, &base_hash, &changed_hash),
            MappingStatusV5::Modified
        );
    }

    #[test]
    fn correspondence_count_byte_and_memory_bounds_are_exact_and_overflow_closed() {
        assert!(
            bounded(
                MAX_M6_OBLIGATIONS_PER_UNIVERSE,
                MAX_M6_OBLIGATIONS_PER_UNIVERSE,
                "fixture obligation bound",
            )
            .is_ok()
        );
        assert!(matches!(
            bounded(
                MAX_M6_OBLIGATIONS_PER_UNIVERSE + 1,
                MAX_M6_OBLIGATIONS_PER_UNIVERSE,
                "fixture obligation bound",
            ),
            Err(M6Error::Incomplete { .. })
        ));
        assert!(
            bounded(
                MAX_M6_CORRESPONDENCE_ENTRIES,
                MAX_M6_CORRESPONDENCE_ENTRIES,
                "fixture correspondence entry bound",
            )
            .is_ok()
        );
        assert!(matches!(
            bounded(
                MAX_M6_CORRESPONDENCE_ENTRIES + 1,
                MAX_M6_CORRESPONDENCE_ENTRIES,
                "fixture correspondence entry bound",
            ),
            Err(M6Error::Incomplete { .. })
        ));
        assert_eq!(
            preflight_event_line(MAX_M6_CORRESPONDENCE_DTO_BYTES - 1, 1).unwrap(),
            MAX_M6_CORRESPONDENCE_DTO_BYTES
        );
        assert!(matches!(
            preflight_event_line(MAX_M6_CORRESPONDENCE_DTO_BYTES, 1),
            Err(M6Error::Incomplete { .. })
        ));
        assert_eq!(
            checked_correspondence_working_add(MAX_M6_CORRESPONDENCE_WORKING_BYTES - 1, 1).unwrap(),
            MAX_M6_CORRESPONDENCE_WORKING_BYTES
        );
        assert!(matches!(
            checked_correspondence_working_add(MAX_M6_CORRESPONDENCE_WORKING_BYTES, 1),
            Err(M6Error::Incomplete { .. })
        ));
        assert!(matches!(
            checked_correspondence_working_add(usize::MAX, 1),
            Err(M6Error::Incomplete {
                observed: usize::MAX,
                ..
            })
        ));

        let (closure, mappings, source, target, phase) = correspondence_phase();
        let actual_oracle = correspondence_allocation_oracle(&mappings, &source, &target).unwrap();
        let actual_reservation = actual_oracle.reservation_bytes().unwrap();
        assert_eq!(
            phase.working_peak_upper_bound_bytes(),
            actual_reservation,
            "the phase exposes the preflight oracle reservation, not a post-hoc subset"
        );
        CORRESPONDENCE_AGGREGATE_VALIDATION_CALLS.with(|calls| calls.set(0));
        assert!(matches!(
            ObligationCorrespondenceV5::derive_with_working_limit(
                &closure,
                &mappings,
                &source,
                &target,
                actual_reservation - 1,
            ),
            Err(M6Error::Incomplete {
                operation: "M6 correspondence preflight working bytes",
                ..
            })
        ));
        CORRESPONDENCE_AGGREGATE_VALIDATION_CALLS.with(|calls| assert_eq!(calls.get(), 0));

        let exact_phase = ObligationCorrespondenceV5::derive_with_working_limit(
            &closure,
            &mappings,
            &source,
            &target,
            actual_reservation,
        )
        .unwrap();
        assert_eq!(
            exact_phase.correspondence(),
            phase.correspondence(),
            "the actual production reservation is inclusive"
        );
        CORRESPONDENCE_AGGREGATE_VALIDATION_CALLS.with(|calls| assert_eq!(calls.get(), 2));

        CORRESPONDENCE_AGGREGATE_VALIDATION_CALLS.with(|calls| calls.set(0));
        ObligationCorrespondenceV5::derive_from_accepted_universes(
            &closure, &mappings, &source, &target,
        )
        .unwrap();
        CORRESPONDENCE_AGGREGATE_VALIDATION_CALLS.with(|calls| assert_eq!(calls.get(), 2));
        let from_ids = (0..MAX_M6_CORRESPONDENCE_SIDE_IDS)
            .map(|index| id(&format!("obligation:bound-source-{index:02}")))
            .collect::<BTreeSet<_>>();
        let to_ids = (0..MAX_M6_CORRESPONDENCE_SIDE_IDS)
            .map(|index| id(&format!("obligation:bound-target-{index:02}")))
            .collect::<BTreeSet<_>>();
        let hashes = |ids: &BTreeSet<StableId>| {
            ids.iter()
                .enumerate()
                .map(|(index, id)| IdBodyHashV5::new(id.clone(), sha(900 + index)).unwrap())
                .collect::<Vec<_>>()
        };
        let exact =
            ObligationCorrespondenceEntryV5::from_parts(ObligationCorrespondenceEntryPartsV5 {
                morphism_id: mappings.morphism().id().clone(),
                from_obligation_ids: from_ids.clone(),
                to_obligation_ids: to_ids.clone(),
                status: MappingStatusV5::Unresolved,
                source_mapping_ids: BTreeSet::new(),
                predecessor_entry_ids: BTreeSet::new(),
                source_body_hashes: hashes(&from_ids),
                target_body_hashes: hashes(&to_ids),
            })
            .unwrap();
        assert_eq!(
            exact.from_obligation_ids().len(),
            MAX_M6_CORRESPONDENCE_SIDE_IDS
        );
        let mut over_from = from_ids;
        over_from.insert(id("obligation:bound-source-over"));
        assert!(matches!(
            ObligationCorrespondenceEntryV5::from_parts(ObligationCorrespondenceEntryPartsV5 {
                morphism_id: mappings.morphism().id().clone(),
                from_obligation_ids: over_from.clone(),
                to_obligation_ids: to_ids,
                status: MappingStatusV5::Unresolved,
                source_mapping_ids: BTreeSet::new(),
                predecessor_entry_ids: BTreeSet::new(),
                source_body_hashes: hashes(&over_from),
                target_body_hashes: Vec::new(),
            },),
            Err(M6Error::Incomplete { .. })
        ));

        let predecessor_ids = (0..MAX_M6_CORRESPONDENCE_PREDECESSOR_IDS)
            .map(|index| {
                id(&format!(
                    "obligation-correspondence-entry-v5:predecessor-{index:02}"
                ))
            })
            .collect::<BTreeSet<_>>();
        let one_source = BTreeSet::from([id("obligation:predecessor-bound-source")]);
        let one_target = BTreeSet::from([id("obligation:predecessor-bound-target")]);
        assert!(
            ObligationCorrespondenceEntryV5::from_parts(ObligationCorrespondenceEntryPartsV5 {
                morphism_id: mappings.morphism().id().clone(),
                from_obligation_ids: one_source.clone(),
                to_obligation_ids: one_target.clone(),
                status: MappingStatusV5::Modified,
                source_mapping_ids: BTreeSet::new(),
                predecessor_entry_ids: predecessor_ids.clone(),
                source_body_hashes: hashes(&one_source),
                target_body_hashes: hashes(&one_target),
            },)
            .is_ok()
        );
        let mut over_predecessors = predecessor_ids;
        over_predecessors.insert(id("obligation-correspondence-entry-v5:predecessor-over"));
        assert!(matches!(
            ObligationCorrespondenceEntryV5::from_parts(ObligationCorrespondenceEntryPartsV5 {
                morphism_id: mappings.morphism().id().clone(),
                from_obligation_ids: one_source.clone(),
                to_obligation_ids: one_target.clone(),
                status: MappingStatusV5::Modified,
                source_mapping_ids: BTreeSet::new(),
                predecessor_entry_ids: over_predecessors,
                source_body_hashes: hashes(&one_source),
                target_body_hashes: hashes(&one_target),
            },),
            Err(M6Error::Incomplete { .. })
        ));
    }

    #[test]
    fn component_reduction_and_count_object_close_all_seven_statuses() {
        let source_repository = ProgramNodeV5 {
            id: id("repository:source"),
            object_kind: ProgramObjectKindV5::Repository,
            body_hash: sha(1),
            data: NodeDataV5::Repository {
                identity: "same".to_owned(),
            },
        };
        let target_repository = ProgramNodeV5 {
            id: id("repository:target"),
            object_kind: ProgramObjectKindV5::Repository,
            body_hash: sha(2),
            data: NodeDataV5::Repository {
                identity: "same".to_owned(),
            },
        };
        let source_snapshot = ProgramNodeV5 {
            id: id("snapshot:source"),
            object_kind: ProgramObjectKindV5::Snapshot,
            body_hash: sha(3),
            data: NodeDataV5::Snapshot,
        };
        let target_snapshot = ProgramNodeV5 {
            id: id("snapshot:target"),
            object_kind: ProgramObjectKindV5::Snapshot,
            body_hash: sha(4),
            data: NodeDataV5::Snapshot,
        };
        let source_nodes = BTreeMap::from([
            (source_repository.id.clone(), &source_repository),
            (source_snapshot.id.clone(), &source_snapshot),
        ]);
        let target_nodes = BTreeMap::from([
            (target_repository.id.clone(), &target_repository),
            (target_snapshot.id.clone(), &target_snapshot),
        ]);
        let successors = BTreeMap::from([
            (
                source_repository.id.clone(),
                BTreeSet::from([target_repository.id.clone()]),
            ),
            (
                source_snapshot.id.clone(),
                BTreeSet::from([target_snapshot.id.clone()]),
            ),
        ]);
        let seed = |from_ids, to_ids, object_kind, candidate_key_kind| ComponentSeedV5 {
            object_kind,
            candidate_key_kind,
            from_ids,
            to_ids,
            stage: 0,
        };
        let statuses = [
            component_status(
                &seed(
                    BTreeSet::from([source_repository.id.clone()]),
                    BTreeSet::from([target_repository.id.clone()]),
                    ProgramObjectKindV5::Repository,
                    CandidateKeyKindV5::RepositoryIdentity,
                ),
                &source_nodes,
                &target_nodes,
                &successors,
            ),
            component_status(
                &seed(
                    BTreeSet::from([source_snapshot.id.clone()]),
                    BTreeSet::from([target_snapshot.id.clone()]),
                    ProgramObjectKindV5::Snapshot,
                    CandidateKeyKindV5::SnapshotPair,
                ),
                &source_nodes,
                &target_nodes,
                &successors,
            ),
            component_status(
                &seed(
                    BTreeSet::new(),
                    BTreeSet::from([id("artifact:added")]),
                    ProgramObjectKindV5::Artifact,
                    CandidateKeyKindV5::NoCandidate,
                ),
                &source_nodes,
                &target_nodes,
                &successors,
            ),
            component_status(
                &seed(
                    BTreeSet::from([id("artifact:removed")]),
                    BTreeSet::new(),
                    ProgramObjectKindV5::Artifact,
                    CandidateKeyKindV5::NoCandidate,
                ),
                &source_nodes,
                &target_nodes,
                &successors,
            ),
            component_status(
                &seed(
                    BTreeSet::from([id("artifact:s")]),
                    BTreeSet::from([id("artifact:t1"), id("artifact:t2")]),
                    ProgramObjectKindV5::Artifact,
                    CandidateKeyKindV5::SamePath,
                ),
                &source_nodes,
                &target_nodes,
                &successors,
            ),
            component_status(
                &seed(
                    BTreeSet::from([id("artifact:s1"), id("artifact:s2")]),
                    BTreeSet::from([id("artifact:t")]),
                    ProgramObjectKindV5::Artifact,
                    CandidateKeyKindV5::SamePath,
                ),
                &source_nodes,
                &target_nodes,
                &successors,
            ),
            component_status(
                &seed(
                    BTreeSet::from([id("artifact:s1"), id("artifact:s2")]),
                    BTreeSet::from([id("artifact:t1"), id("artifact:t2")]),
                    ProgramObjectKindV5::Artifact,
                    CandidateKeyKindV5::SamePath,
                ),
                &source_nodes,
                &target_nodes,
                &successors,
            ),
        ];
        assert_eq!(
            statuses,
            [
                MappingStatusV5::Preserved,
                MappingStatusV5::Modified,
                MappingStatusV5::Added,
                MappingStatusV5::Removed,
                MappingStatusV5::Split,
                MappingStatusV5::Merged,
                MappingStatusV5::Unresolved,
            ]
        );
        let mut counts = MappingStatusCountsV5::default();
        for status in statuses {
            counts.record(status);
        }
        assert_eq!(
            serde_json::to_value(counts).unwrap(),
            serde_json::json!({
                "added": 1, "merged": 1, "modified": 1, "preserved": 1,
                "removed": 1, "split": 1, "unresolved": 1
            })
        );
    }

    #[test]
    fn ambiguous_endpoint_correspondence_propagates_unresolved_to_one_to_one_relation() {
        let source_relation = ProgramNodeV5 {
            id: id("relation:source-ambiguous"),
            object_kind: ProgramObjectKindV5::Relation,
            body_hash: sha(10),
            data: NodeDataV5::Relation {
                kind: "calls".to_owned(),
                directed: true,
                source_id: id("function:source"),
                ordered_target_ids: vec![id("function:callee-source")],
                base_hash: sha(11),
                provenance: NormalizedProvenanceV5 {
                    stable_hash: sha(20),
                    revision: NormalizedRevisionRefV5::Absent,
                    content: NormalizedContentRefV5::Absent,
                    local: NormalizedLocalRefV5::Absent,
                },
            },
        };
        let target_relation = ProgramNodeV5 {
            id: id("relation:target-ambiguous"),
            object_kind: ProgramObjectKindV5::Relation,
            body_hash: sha(12),
            data: NodeDataV5::Relation {
                kind: "calls".to_owned(),
                directed: true,
                source_id: id("function:target-a"),
                ordered_target_ids: vec![id("function:callee-target")],
                base_hash: sha(11),
                provenance: NormalizedProvenanceV5 {
                    stable_hash: sha(20),
                    revision: NormalizedRevisionRefV5::Absent,
                    content: NormalizedContentRefV5::Absent,
                    local: NormalizedLocalRefV5::Absent,
                },
            },
        };
        let seed = ComponentSeedV5 {
            object_kind: ProgramObjectKindV5::Relation,
            candidate_key_kind: CandidateKeyKindV5::MappedDirectedEndpoints,
            from_ids: BTreeSet::from([source_relation.id.clone()]),
            to_ids: BTreeSet::from([target_relation.id.clone()]),
            stage: 10,
        };
        let source_by_id = BTreeMap::from([(source_relation.id.clone(), &source_relation)]);
        let target_by_id = BTreeMap::from([(target_relation.id.clone(), &target_relation)]);
        let successors = BTreeMap::from([
            (
                id("function:source"),
                BTreeSet::from([id("function:target-a"), id("function:target-b")]),
            ),
            (
                id("function:callee-source"),
                BTreeSet::from([id("function:callee-target")]),
            ),
        ]);
        assert!(relation_candidate(
            &source_relation,
            &target_relation,
            &successors
        ));
        assert_eq!(
            component_status(&seed, &source_by_id, &target_by_id, &successors),
            MappingStatusV5::Unresolved
        );
    }

    #[test]
    fn same_stable_id_without_candidate_remains_side_qualified_removed_and_added() {
        let source = ProgramNodeV5 {
            id: id("custom:same-id"),
            object_kind: ProgramObjectKindV5::Artifact,
            body_hash: sha(1),
            data: NodeDataV5::Artifact {
                kind: "custom".to_owned(),
                label: "source".to_owned(),
                language: None,
                path: None,
                location: None,
                content_hash: None,
                anchor: None,
                base_hash: sha(2),
                provenance: NormalizedProvenanceV5 {
                    stable_hash: sha(20),
                    revision: NormalizedRevisionRefV5::Absent,
                    content: NormalizedContentRefV5::Absent,
                    local: NormalizedLocalRefV5::Absent,
                },
                change_fact_ids: BTreeSet::new(),
                symbol_ref: None,
            },
        };
        let target = ProgramNodeV5 {
            id: source.id.clone(),
            object_kind: ProgramObjectKindV5::Artifact,
            body_hash: sha(3),
            data: NodeDataV5::Artifact {
                kind: "custom".to_owned(),
                label: "target".to_owned(),
                language: None,
                path: None,
                location: None,
                content_hash: None,
                anchor: None,
                base_hash: sha(4),
                provenance: NormalizedProvenanceV5 {
                    stable_hash: sha(20),
                    revision: NormalizedRevisionRefV5::Absent,
                    content: NormalizedContentRefV5::Absent,
                    local: NormalizedLocalRefV5::Absent,
                },
                change_fact_ids: BTreeSet::new(),
                symbol_ref: None,
            },
        };
        let mut consumed_source = BTreeSet::new();
        let mut consumed_target = BTreeSet::new();
        let mut seeds = Vec::new();
        finish_unmatched(
            &[&source],
            &[&target],
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Artifact,
            1,
            &mut seeds,
        );
        assert_eq!(seeds.len(), 2);
        assert!(seeds.iter().any(|seed| {
            seed.from_ids == BTreeSet::from([source.id.clone()]) && seed.to_ids.is_empty()
        }));
        assert!(seeds.iter().any(|seed| {
            seed.from_ids.is_empty() && seed.to_ids == BTreeSet::from([target.id.clone()])
        }));
    }

    #[test]
    fn snapshot_scoped_git_provenance_normalizes_but_extractor_identity_does_not() {
        let (source_space, target_space) = spaces();
        let path = "src/checkout_controller.rs";
        let source_blob = sha(700);
        let target_blob = sha(701);
        let source_paths = BTreeMap::from([(
            path.to_owned(),
            SnapshotPathArtifactV5 {
                id: id("file:source"),
                content_hash: Some(source_blob.clone()),
            },
        )]);
        let target_paths = BTreeMap::from([(
            path.to_owned(),
            SnapshotPathArtifactV5 {
                id: id("file:target"),
                content_hash: Some(target_blob.clone()),
            },
        )]);
        let provenance = |space: &ProgramSpace, blob: ContentHash, extractor: &str| {
            crate::Provenance::accepted_deterministic(
                crate::SourceRef::new(
                    "git",
                    space.repository_identity(),
                    Some(space.target_revision().to_owned()),
                    Some(blob),
                    Some(path.to_owned()),
                )
                .unwrap(),
                extractor,
                Some("1.0.0".to_owned()),
                None,
            )
            .unwrap()
        };
        let source_provenance = normalize_provenance(
            &source_space,
            &provenance(&source_space, source_blob.clone(), "rust-syn@1"),
            Some(path),
            Some(&source_blob),
            &source_paths,
        )
        .unwrap();
        let target_provenance = normalize_provenance(
            &target_space,
            &provenance(&target_space, target_blob.clone(), "rust-syn@1"),
            Some(path),
            Some(&target_blob),
            &target_paths,
        )
        .unwrap();
        let successors = BTreeMap::from([(id("file:source"), BTreeSet::from([id("file:target")]))]);
        assert!(normalized_provenance_equal(
            &source_provenance,
            &target_provenance,
            &successors
        ));

        let mutated = normalize_provenance(
            &target_space,
            &provenance(&target_space, target_blob.clone(), "rust-syn@2"),
            Some(path),
            Some(&target_blob),
            &target_paths,
        )
        .unwrap();
        assert!(!normalized_provenance_equal(
            &source_provenance,
            &mutated,
            &successors
        ));

        let external = crate::Provenance::accepted_deterministic(
            crate::SourceRef::new(
                "external",
                "https://evidence.invalid/source",
                Some(source_space.target_revision().to_owned()),
                Some(source_space.incremental_tree_hash_for_store().clone()),
                Some(path.to_owned()),
            )
            .unwrap(),
            "external-parser@1",
            None,
            None,
        )
        .unwrap();
        let external = normalize_provenance(
            &source_space,
            &external,
            Some(path),
            Some(&source_blob),
            &source_paths,
        )
        .unwrap();
        assert!(matches!(
            external.revision,
            NormalizedRevisionRefV5::Exact(_)
        ));
        assert!(matches!(external.content, NormalizedContentRefV5::Exact(_)));
        assert!(matches!(external.local, NormalizedLocalRefV5::Exact(_)));
    }

    #[test]
    fn rust_equal_key_ambiguity_is_split_without_canonical_first_choice() {
        let (source, _) = spaces();
        let mut target_value: Value = serde_json::from_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        target_value["snapshot"]["base_revision"] = Value::String(oid(10));
        target_value["snapshot"]["target_revision"] = Value::String(oid(12));
        target_value["snapshot"]["tree_hash"] = Value::String(tree(13).to_string());
        replace_string(
            &mut target_value,
            "snapshot:double-submit-v1",
            "snapshot:double-submit-v2",
        );
        let artifacts = target_value["artifacts"].as_array_mut().unwrap();
        let mut duplicate = artifacts
            .iter()
            .find(|value| value["id"] == "function:checkout-submit")
            .unwrap()
            .clone();
        duplicate["id"] = Value::String("function:checkout-submit-duplicate".to_owned());
        artifacts.push(duplicate);
        let target =
            ProgramSpace::from_json_slice(&serde_json::to_vec(&target_value).unwrap()).unwrap();
        let closure = closure(&source, &target);
        let mut target_anchors = anchors(&target);
        let original = target_anchors[&id("function:checkout-submit")].clone();
        target_anchors.insert(id("function:checkout-submit-duplicate"), original);
        let proof = ValidatedIncrementalInputsV5::fixture_from_parts(
            &source,
            &target,
            anchors(&source),
            target_anchors,
            orders(&source),
            orders(&target),
            vec![],
        )
        .unwrap();
        let phase =
            ChangeMorphismV5::build_with_inputs(&closure, &source, &target, &proof).unwrap();
        let split = phase
            .mappings()
            .iter()
            .find(|mapping| mapping.status() == MappingStatusV5::Split)
            .unwrap();
        assert_eq!(split.from_ids().len(), 1);
        assert_eq!(split.to_ids().len(), 2);
        assert_eq!(split.successor_ids(), split.to_ids());
    }

    #[test]
    fn relation_sidecar_order_is_semantic_and_never_set_order() {
        let (source, target) = spaces();
        let closure = closure(&source, &target);
        let source_orders = orders(&source);
        let mut target_orders = orders(&target);
        let relation_id = target
            .relations()
            .iter()
            .find(|relation| relation.target_ids.len() > 1)
            .unwrap()
            .id
            .clone();
        target_orders.get_mut(&relation_id).unwrap().reverse();
        let proof = ValidatedIncrementalInputsV5::fixture_from_parts(
            &source,
            &target,
            anchors(&source),
            anchors(&target),
            source_orders,
            target_orders,
            vec![],
        )
        .unwrap();
        let phase =
            ChangeMorphismV5::build_with_inputs(&closure, &source, &target, &proof).unwrap();
        assert!(phase.mappings().iter().any(|mapping| mapping.object_kind()
            == ProgramObjectKindV5::Relation
            && mapping.status() == MappingStatusV5::Removed));
        assert!(phase.mappings().iter().any(|mapping| mapping.object_kind()
            == ProgramObjectKindV5::Relation
            && mapping.status() == MappingStatusV5::Added));
    }

    #[test]
    fn relation_dependency_waves_cross_legacy_32_and_255_without_stage_collision() {
        for length in [25_usize, 256] {
            let (source, target) = spaces_with_relation_chain(length);
            let closure = closure(&source, &target);
            let phase = ChangeMorphismV5::build_with_inputs(
                &closure,
                &source,
                &target,
                &inputs(&source, &target),
            )
            .unwrap();
            let last_id = id(&format!("relation:chain-{:04}", length - 1));
            let previous_id = id(&format!("relation:chain-{:04}", length - 2));
            let last = phase
                .mappings()
                .iter()
                .find(|mapping| mapping.from_ids().contains(&last_id))
                .unwrap();
            let previous_mapping_id = phase
                .mappings()
                .iter()
                .find(|mapping| mapping.from_ids().contains(&previous_id))
                .unwrap()
                .id()
                .clone();
            let file_mapping_id = phase
                .mappings()
                .iter()
                .find(|mapping| mapping.from_ids().contains(&id("file:checkout-controller")))
                .unwrap()
                .id()
                .clone();
            assert_eq!(last.status(), MappingStatusV5::Preserved);
            assert_eq!(
                last.predecessor_mapping_ids(),
                &BTreeSet::from([file_mapping_id, previous_mapping_id])
            );
        }
    }

    #[test]
    fn mutually_referential_relation_scc_is_one_unresolved_component() {
        let (source, target) = spaces_with_mutual_relation_scc();
        let closure = closure(&source, &target);
        let phase = ChangeMorphismV5::build_with_inputs(
            &closure,
            &source,
            &target,
            &inputs(&source, &target),
        )
        .unwrap();
        let cycle_ids = BTreeSet::from([id("relation:cycle-a"), id("relation:cycle-b")]);
        let cycle = phase
            .mappings()
            .iter()
            .find(|mapping| mapping.from_ids() == &cycle_ids)
            .expect("the cyclic fixed point must not split into removed/added records");
        assert_eq!(cycle.to_ids(), &cycle_ids);
        assert_eq!(cycle.status(), MappingStatusV5::Unresolved);
        let file_mapping_id = phase
            .mappings()
            .iter()
            .find(|mapping| mapping.from_ids().contains(&id("file:checkout-controller")))
            .unwrap()
            .id()
            .clone();
        assert_eq!(
            cycle.predecessor_mapping_ids(),
            &BTreeSet::from([file_mapping_id])
        );
    }

    #[test]
    fn asymmetric_relation_scc_is_atomic_with_all_external_predecessors() {
        let (source, target) = spaces_with_mutual_relation_scc();
        let make_asymmetric = |space: ProgramSpace| {
            let mut value: Value =
                serde_json::from_slice(&crate::canonical_json(&space.streaming_ref()).unwrap())
                    .unwrap();
            value["relations"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|relation| relation["id"] == "relation:cycle-b")
                .unwrap()["source_id"] = Value::String("file:payment-repository".to_owned());
            ProgramSpace::from_json_slice(&serde_json::to_vec(&value).unwrap()).unwrap()
        };
        let source = make_asymmetric(source);
        let target = make_asymmetric(target);
        let closure = closure(&source, &target);
        let phase = ChangeMorphismV5::build_with_inputs(
            &closure,
            &source,
            &target,
            &inputs(&source, &target),
        )
        .unwrap();
        let cycle_ids = BTreeSet::from([id("relation:cycle-a"), id("relation:cycle-b")]);
        let cycle = phase
            .mappings()
            .iter()
            .find(|mapping| mapping.from_ids() == &cycle_ids)
            .expect("Tarjan SCC membership must be the atomic mapping unit");
        assert_eq!(cycle.to_ids(), &cycle_ids);
        assert_eq!(cycle.status(), MappingStatusV5::Unresolved);
        let external_predecessors = ["file:checkout-controller", "file:payment-repository"]
            .into_iter()
            .map(|file_id| {
                phase
                    .mappings()
                    .iter()
                    .find(|mapping| mapping.from_ids().contains(&id(file_id)))
                    .unwrap()
                    .id()
                    .clone()
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(cycle.predecessor_mapping_ids(), &external_predecessors);
    }

    #[test]
    fn disjoint_same_kind_sccs_with_distinct_external_topology_do_not_merge() {
        let (source, target) = spaces_with_two_disjoint_relation_sccs();
        let closure = closure(&source, &target);
        let phase = ChangeMorphismV5::build_with_inputs(
            &closure,
            &source,
            &target,
            &inputs(&source, &target),
        )
        .unwrap();
        let first = BTreeSet::from([id("relation:cycle-a"), id("relation:cycle-b")]);
        let second = BTreeSet::from([id("relation:cycle-c"), id("relation:cycle-d")]);
        assert!(phase.mappings().iter().any(|mapping| {
            mapping.from_ids() == &first
                && mapping.to_ids() == &first
                && mapping.status() == MappingStatusV5::Unresolved
        }));
        assert!(phase.mappings().iter().any(|mapping| {
            mapping.from_ids() == &second
                && mapping.to_ids() == &second
                && mapping.status() == MappingStatusV5::Unresolved
        }));
        assert!(!phase.mappings().iter().any(|mapping| {
            mapping.from_ids().is_superset(&first) && mapping.from_ids().is_superset(&second)
        }));
    }

    #[test]
    fn scc_condensation_stages_dependencies_before_dependents() {
        let (source, target) = spaces_with_two_disjoint_relation_sccs();
        let add_condensation_edge = |space: ProgramSpace| {
            let mut value: Value =
                serde_json::from_slice(&crate::canonical_json(&space.streaming_ref()).unwrap())
                    .unwrap();
            let relation = value["relations"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|relation| relation["id"] == "relation:cycle-c")
                .unwrap();
            relation["target_ids"] = serde_json::json!(["relation:cycle-d", "relation:cycle-a"]);
            ProgramSpace::from_json_slice(&serde_json::to_vec(&value).unwrap()).unwrap()
        };
        let source = add_condensation_edge(source);
        let target = add_condensation_edge(target);
        let source_nodes = program_nodes(
            &source,
            ProgramSideV5::Source,
            &anchors(&source),
            &orders(&source),
            &[],
        )
        .unwrap()
        .0;
        let relation_nodes = source_nodes
            .iter()
            .filter(|node| node.object_kind == ProgramObjectKindV5::Relation)
            .collect::<Vec<_>>();
        let cycle_ids = BTreeSet::from([
            id("relation:cycle-a"),
            id("relation:cycle-b"),
            id("relation:cycle-c"),
            id("relation:cycle-d"),
        ]);
        let components = relation_scc_components(&relation_nodes, &cycle_ids);
        let depths = relation_scc_depths(&relation_nodes, &components).unwrap();
        let depth_by_id = components
            .iter()
            .zip(depths)
            .flat_map(|(component, depth)| component.iter().cloned().map(move |id| (id, depth)))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(depth_by_id[&id("relation:cycle-a")], 0);
        assert_eq!(depth_by_id[&id("relation:cycle-c")], 1);

        let closure = closure(&source, &target);
        let phase = ChangeMorphismV5::build_with_inputs(
            &closure,
            &source,
            &target,
            &inputs(&source, &target),
        )
        .unwrap();
        let dependency_ids = BTreeSet::from([id("relation:cycle-a"), id("relation:cycle-b")]);
        let dependent_ids = BTreeSet::from([id("relation:cycle-c"), id("relation:cycle-d")]);
        let dependency = phase
            .mappings()
            .iter()
            .find(|mapping| mapping.from_ids() == &dependency_ids)
            .unwrap();
        let dependent = phase
            .mappings()
            .iter()
            .find(|mapping| mapping.from_ids() == &dependent_ids)
            .unwrap();
        assert_eq!(dependency.status(), MappingStatusV5::Unresolved);
        assert_eq!(dependent.status(), MappingStatusV5::Unresolved);
        assert!(
            dependent
                .predecessor_mapping_ids()
                .contains(dependency.id()),
            "dependent SCC must retain the dependency SCC mapping as source trace"
        );
    }

    #[test]
    fn ambiguity_grouping_recomputes_condensation_depth_before_source_trace() {
        let (source, target) = spaces();
        let add_relations = |space: ProgramSpace| {
            let mut value: Value =
                serde_json::from_slice(&crate::canonical_json(&space.streaming_ref()).unwrap())
                    .unwrap();
            let template = value["relations"][0].clone();
            for (relation_id, kind, target_ids) in [
                (
                    "relation:group-d1",
                    "ambiguity_group",
                    serde_json::json!(["relation:group-d1"]),
                ),
                (
                    "relation:group-e",
                    "ambiguity_group",
                    serde_json::json!(["relation:group-e"]),
                ),
                (
                    "relation:group-d2",
                    "ambiguity_group",
                    serde_json::json!(["relation:group-e"]),
                ),
                (
                    "relation:group-a",
                    "ambiguity_dependent",
                    serde_json::json!(["relation:group-a", "relation:group-d1"]),
                ),
            ] {
                let mut relation = template.clone();
                relation["id"] = Value::String(relation_id.to_owned());
                relation["kind"] = Value::String(kind.to_owned());
                relation["source_id"] = Value::String("file:checkout-controller".to_owned());
                relation["target_ids"] = target_ids;
                relation["directed"] = Value::Bool(true);
                relation["attributes"] = serde_json::json!({});
                value["relations"].as_array_mut().unwrap().push(relation);
            }
            ProgramSpace::from_json_slice(&serde_json::to_vec(&value).unwrap()).unwrap()
        };
        let source = add_relations(source);
        let target = add_relations(target);
        let closure = closure(&source, &target);
        let phase = ChangeMorphismV5::build_with_inputs(
            &closure,
            &source,
            &target,
            &inputs(&source, &target),
        )
        .unwrap();

        let dependency_ids = BTreeSet::from([
            id("relation:group-d1"),
            id("relation:group-d2"),
            id("relation:group-e"),
        ]);
        let dependent_ids = BTreeSet::from([id("relation:group-a")]);
        let dependency = phase
            .mappings()
            .iter()
            .find(|mapping| mapping.from_ids() == &dependency_ids)
            .expect("equal-signature raw SCCs must remain one ambiguity group");
        let dependent = phase
            .mappings()
            .iter()
            .find(|mapping| mapping.from_ids() == &dependent_ids)
            .expect("dependent relation must retain its own mapping component");

        assert_eq!(dependency.to_ids(), &dependency_ids);
        assert_eq!(dependency.status(), MappingStatusV5::Unresolved);
        assert!(
            dependent
                .predecessor_mapping_ids()
                .contains(dependency.id()),
            "the final grouped condensation DAG must stage A strictly after its D1 dependency and retain that mapping as source trace"
        );
    }

    #[test]
    fn grouping_induced_seed_cycle_collapses_to_one_atomic_unresolved_component() {
        let (source, target) = spaces();
        let add_relations = |space: ProgramSpace| {
            let mut value: Value =
                serde_json::from_slice(&crate::canonical_json(&space.streaming_ref()).unwrap())
                    .unwrap();
            let template = value["relations"][0].clone();
            for (relation_id, target_id) in [
                ("relation:quotient-x1", "relation:quotient-y1"),
                ("relation:quotient-x2", "file:checkout-controller"),
                ("relation:quotient-y1", "file:checkout-controller"),
                ("relation:quotient-y2", "relation:quotient-x2"),
            ] {
                let mut relation = template.clone();
                relation["id"] = Value::String(relation_id.to_owned());
                relation["kind"] = Value::String("quotient_cycle_fixture".to_owned());
                relation["source_id"] = Value::String("file:checkout-controller".to_owned());
                relation["target_ids"] = serde_json::json!([target_id]);
                relation["directed"] = Value::Bool(true);
                relation["attributes"] = serde_json::json!({});
                value["relations"].as_array_mut().unwrap().push(relation);
            }
            ProgramSpace::from_json_slice(&serde_json::to_vec(&value).unwrap()).unwrap()
        };
        let source = add_relations(source);
        let target = add_relations(target);
        let source_program_nodes = program_nodes(
            &source,
            ProgramSideV5::Source,
            &anchors(&source),
            &orders(&source),
            &[],
        )
        .unwrap()
        .0;
        let target_program_nodes = program_nodes(
            &target,
            ProgramSideV5::Target,
            &anchors(&target),
            &orders(&target),
            &[],
        )
        .unwrap()
        .0;
        let source_relations = source_program_nodes
            .iter()
            .filter(|node| node.object_kind == ProgramObjectKindV5::Relation)
            .collect::<Vec<_>>();
        let target_relations = target_program_nodes
            .iter()
            .filter(|node| node.object_kind == ProgramObjectKindV5::Relation)
            .collect::<Vec<_>>();
        let x_ids = BTreeSet::from([id("relation:quotient-x1"), id("relation:quotient-x2")]);
        let y_ids = BTreeSet::from([id("relation:quotient-y1"), id("relation:quotient-y2")]);
        let make_seed = |ids: BTreeSet<StableId>| ComponentSeedV5 {
            object_kind: ProgramObjectKindV5::Relation,
            candidate_key_kind: CandidateKeyKindV5::MappedDirectedEndpoints,
            from_ids: ids.clone(),
            to_ids: ids,
            stage: 0,
        };

        let collapsed = collapse_and_stage_grouped_relation_seeds(
            vec![make_seed(x_ids.clone()), make_seed(y_ids.clone())],
            &source_relations,
            &target_relations,
            17,
        )
        .unwrap();

        assert_eq!(collapsed.len(), 1);
        assert_eq!(
            collapsed[0].from_ids,
            x_ids.union(&y_ids).cloned().collect()
        );
        assert_eq!(collapsed[0].to_ids, collapsed[0].from_ids);
        assert_eq!(collapsed[0].stage, 17);
    }

    #[test]
    fn mismatched_relation_scc_topology_is_not_cross_connected() {
        let (source, target) = spaces_with_mutual_relation_scc();
        let mut target_value: Value =
            serde_json::from_slice(&crate::canonical_json(&target.streaming_ref()).unwrap())
                .unwrap();
        let relations = target_value["relations"].as_array_mut().unwrap();
        let template = relations
            .iter()
            .find(|relation| relation["id"] == "relation:cycle-a")
            .unwrap()
            .clone();
        relations
            .iter_mut()
            .find(|relation| relation["id"] == "relation:cycle-b")
            .unwrap()["target_ids"] = serde_json::json!(["relation:cycle-c"]);
        let mut third = template;
        third["id"] = Value::String("relation:cycle-c".to_owned());
        third["target_ids"] = serde_json::json!(["relation:cycle-a"]);
        relations.push(third);
        let target =
            ProgramSpace::from_json_slice(&serde_json::to_vec(&target_value).unwrap()).unwrap();
        let closure = closure(&source, &target);
        let phase = ChangeMorphismV5::build_with_inputs(
            &closure,
            &source,
            &target,
            &inputs(&source, &target),
        )
        .unwrap();
        for relation_id in [id("relation:cycle-a"), id("relation:cycle-b")] {
            assert!(phase.mappings().iter().any(|mapping| {
                mapping.from_ids() == &BTreeSet::from([relation_id.clone()])
                    && mapping.to_ids().is_empty()
                    && mapping.status() == MappingStatusV5::Removed
            }));
        }
        assert!(!phase.mappings().iter().any(|mapping| {
            !mapping.from_ids().is_empty()
                && !mapping.to_ids().is_empty()
                && (mapping.from_ids().contains(&id("relation:cycle-a"))
                    || mapping.from_ids().contains(&id("relation:cycle-b")))
        }));
    }

    #[test]
    fn cyclic_relation_endpoint_order_is_part_of_topology_signature() {
        let (source, target) = spaces_with_mutual_relation_scc();
        let add_external_endpoint = |space: ProgramSpace| {
            let mut value: Value =
                serde_json::from_slice(&crate::canonical_json(&space.streaming_ref()).unwrap())
                    .unwrap();
            for relation in value["relations"].as_array_mut().unwrap().iter_mut() {
                if matches!(
                    relation["id"].as_str(),
                    Some("relation:cycle-a" | "relation:cycle-b")
                ) {
                    relation["target_ids"] = serde_json::json!([
                        relation["target_ids"][0].clone(),
                        "file:payment-repository"
                    ]);
                }
            }
            ProgramSpace::from_json_slice(&serde_json::to_vec(&value).unwrap()).unwrap()
        };
        let source = add_external_endpoint(source);
        let target = add_external_endpoint(target);
        let closure = closure(&source, &target);
        let source_orders = orders(&source);
        let mut target_orders = orders(&target);
        for relation_id in [id("relation:cycle-a"), id("relation:cycle-b")] {
            target_orders.get_mut(&relation_id).unwrap().reverse();
        }
        let proof = ValidatedIncrementalInputsV5::fixture_from_parts(
            &source,
            &target,
            anchors(&source),
            anchors(&target),
            source_orders,
            target_orders,
            vec![],
        )
        .unwrap();
        let phase =
            ChangeMorphismV5::build_with_inputs(&closure, &source, &target, &proof).unwrap();
        assert!(!phase.mappings().iter().any(|mapping| {
            !mapping.from_ids().is_empty()
                && !mapping.to_ids().is_empty()
                && mapping.from_ids().contains(&id("relation:cycle-a"))
        }));
    }

    #[test]
    fn copied_git_fact_is_traced_but_never_becomes_a_rename_edge() {
        let (source, _) = spaces();
        let mut target_value: Value = serde_json::from_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        target_value["snapshot"]["base_revision"] = Value::String(oid(10));
        target_value["snapshot"]["target_revision"] = Value::String(oid(12));
        target_value["snapshot"]["tree_hash"] = Value::String(tree(13).to_string());
        replace_string(
            &mut target_value,
            "snapshot:double-submit-v1",
            "snapshot:double-submit-v2",
        );
        let artifacts = target_value["artifacts"].as_array_mut().unwrap();
        let original = artifacts
            .iter_mut()
            .find(|value| value["id"] == "file:checkout-controller")
            .unwrap();
        original["content_hash"] = Value::String(sha(44).to_string());
        let mut copied = original.clone();
        copied["id"] = Value::String("file:checkout-controller-copy".to_owned());
        copied["label"] = Value::String("src/checkout_controller_copy.rs".to_owned());
        copied["location"]["path"] = Value::String("src/checkout_controller_copy.rs".to_owned());
        artifacts.push(copied);
        let target =
            ProgramSpace::from_json_slice(&serde_json::to_vec(&target_value).unwrap()).unwrap();
        let copy_fact = GitChangeFactV5::fixture_from_parts(
            id("git-change-fact-v5:copy"),
            GitChangeKindV5::Copied,
            id("file:checkout-controller"),
            id("file:checkout-controller-copy"),
            "src/checkout_controller.rs".to_owned(),
            "src/checkout_controller_copy.rs".to_owned(),
            sha(44),
            GIT_CHANGE_PROVENANCE_V1,
        )
        .unwrap();
        let _rename_shape = GitChangeFactV5::fixture_from_parts(
            id("git-change-fact-v5:rename-shape"),
            GitChangeKindV5::Renamed,
            id("file:checkout-controller"),
            id("file:checkout-controller-copy"),
            "src/checkout_controller.rs".to_owned(),
            "src/checkout_controller_copy.rs".to_owned(),
            sha(44),
            GIT_CHANGE_PROVENANCE_V1,
        )
        .unwrap();
        let proof = ValidatedIncrementalInputsV5::fixture_from_parts(
            &source,
            &target,
            anchors(&source),
            anchors(&target),
            orders(&source),
            orders(&target),
            vec![copy_fact],
        )
        .unwrap();
        let closure = closure(&source, &target);
        let phase =
            ChangeMorphismV5::build_with_inputs(&closure, &source, &target, &proof).unwrap();
        let added = phase
            .mappings()
            .iter()
            .find(|mapping| {
                mapping
                    .to_ids()
                    .contains(&id("file:checkout-controller-copy"))
            })
            .unwrap();
        assert_eq!(added.status(), MappingStatusV5::Added);
        assert!(added.successor_ids().is_empty());
        assert!(added.source_ids().contains(&id("git-change-fact-v5:copy")));
    }

    #[test]
    fn exact_git_rename_fact_forms_the_only_rename_edge() {
        let (source, _) = spaces();
        let mut target_value: Value = serde_json::from_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        target_value["snapshot"]["base_revision"] = Value::String(oid(10));
        target_value["snapshot"]["target_revision"] = Value::String(oid(12));
        target_value["snapshot"]["tree_hash"] = Value::String(tree(13).to_string());
        replace_string(
            &mut target_value,
            "snapshot:double-submit-v1",
            "snapshot:double-submit-v2",
        );
        let artifacts = target_value["artifacts"].as_array_mut().unwrap();
        let artifact = artifacts
            .iter_mut()
            .find(|value| value["id"] == "file:checkout-controller")
            .unwrap();
        artifact["content_hash"] = Value::String(sha(44).to_string());
        artifact["label"] = Value::String("src/renamed_checkout_controller.rs".to_owned());
        artifact["location"]["path"] =
            Value::String("src/renamed_checkout_controller.rs".to_owned());
        let mut provenance = artifact["provenance"].clone();
        provenance["extraction_method"] = Value::String(GIT_CHANGE_PROVENANCE_V1.to_owned());
        artifacts.push(serde_json::json!({
            "attributes": {
                "base_path": "src/checkout_controller.rs",
                "change_kind": "renamed",
                "target_path": "src/renamed_checkout_controller.rs"
            },
            "id": "change:accepted-rename",
            "kind": "custom",
            "label": "renamed src/renamed_checkout_controller.rs",
            "provenance": provenance
        }));
        let target =
            ProgramSpace::from_json_slice(&serde_json::to_vec(&target_value).unwrap()).unwrap();
        let rename = target
            .artifacts()
            .iter()
            .find(|artifact| artifact.id == id("change:accepted-rename"))
            .and_then(|artifact| {
                GitChangeFactV5::from_accepted_artifact(artifact, &source, &target).unwrap()
            })
            .unwrap();
        let proof = ValidatedIncrementalInputsV5::fixture_from_parts(
            &source,
            &target,
            anchors(&source),
            anchors(&target),
            orders(&source),
            orders(&target),
            vec![rename],
        )
        .unwrap();
        let closure = closure(&source, &target);
        let phase =
            ChangeMorphismV5::build_with_inputs(&closure, &source, &target, &proof).unwrap();
        let renamed = phase
            .mappings()
            .iter()
            .find(|mapping| mapping.from_ids().contains(&id("file:checkout-controller")))
            .unwrap();
        assert_eq!(
            renamed.candidate_key_kind(),
            CandidateKeyKindV5::GitRenameSameContent
        );
        assert_eq!(renamed.status(), MappingStatusV5::Preserved);
        assert_eq!(
            renamed.change_fact_ids(),
            &BTreeSet::from([id("change:accepted-rename")])
        );
    }

    #[test]
    fn strict_wire_decode_rejects_unknown_order_duplicate_and_identity_tamper() {
        let (closure, phase) = phase();
        let closure_bytes = crate::canonical_json(&closure).unwrap();
        assert_eq!(
            IncrementalSourceClosureV5::from_json_bytes(&closure_bytes, &closure).unwrap(),
            closure
        );
        let mapping = phase.mappings().first().unwrap();
        let bytes = crate::canonical_json(mapping).unwrap();
        assert_eq!(ProgramMappingV5::from_json_bytes(&bytes).unwrap(), *mapping);
        let mut value: Value = serde_json::from_slice(&bytes).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("unknown".to_owned(), Value::Bool(true));
        assert!(
            ProgramMappingV5::from_json_bytes(&crate::canonical_json(&value).unwrap()).is_err()
        );
        let morphism_bytes = crate::canonical_json(phase.morphism()).unwrap();
        let mapping_bytes = phase
            .mappings()
            .iter()
            .map(|mapping| crate::canonical_json(mapping).unwrap())
            .collect::<Vec<_>>();
        phase
            .validate_replayed_canonical(&mapping_bytes, &morphism_bytes)
            .unwrap();
        let mut reordered = mapping_bytes.clone();
        reordered.swap(0, 1);
        assert!(
            phase
                .validate_replayed_canonical(&reordered, &morphism_bytes)
                .is_err()
        );
        assert_eq!(
            ChangeMorphismV5::from_json_bytes(&morphism_bytes, phase.morphism()).unwrap(),
            *phase.morphism()
        );
        let mut value: Value = serde_json::from_slice(&morphism_bytes).unwrap();
        value["mapping_count"] = Value::from(0);
        assert!(
            ChangeMorphismV5::from_json_bytes(
                &crate::canonical_json(&value).unwrap(),
                phase.morphism()
            )
            .is_err()
        );

        let split_hashes = vec![
            IdBodyHashV5::new(id("artifact:b"), sha(2)).unwrap(),
            IdBodyHashV5::new(id("artifact:a"), sha(1)).unwrap(),
        ];
        assert!(
            validate_body_hash_records(
                "test",
                &BTreeSet::from([id("artifact:a"), id("artifact:b")]),
                &split_hashes,
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<IdBodyHashV5>(serde_json::json!({
                "body_hash": tree(1),
                "id": "artifact:a"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<IdBodyHashV5>(serde_json::json!({
                "body_hash": sha(1),
                "id": "artifact:a",
                "unknown": true
            }))
            .is_err()
        );
    }

    #[test]
    fn allocation_and_event_line_bounds_are_exact_and_overflow_closed() {
        #[derive(Serialize)]
        struct ExactDto {
            a: String,
        }
        for (limit, operation) in [
            (MAX_M6_CLOSURE_DTO_BYTES, "closure"),
            (MAX_M6_MAPPING_DTO_BYTES, "mapping"),
            (MAX_M6_MORPHISM_DTO_BYTES, "morphism"),
        ] {
            bounded(limit, limit, operation).unwrap();
            assert_eq!(
                bounded(limit + 1, limit, operation).unwrap_err(),
                M6Error::Incomplete {
                    operation,
                    limit,
                    observed: limit + 1,
                }
            );
        }
        assert_eq!(
            preflight_event_line(MAX_M6_EVENT_LINE_BYTES - 1, 1).unwrap(),
            MAX_M6_EVENT_LINE_BYTES
        );
        assert_eq!(
            preflight_event_line(MAX_M6_EVENT_LINE_BYTES, 1).unwrap_err(),
            M6Error::Incomplete {
                operation: "M6 event-line bytes",
                limit: MAX_M6_EVENT_LINE_BYTES,
                observed: MAX_M6_EVENT_LINE_BYTES + 1,
            }
        );
        assert_eq!(
            preflight_event_line(usize::MAX, 1).unwrap_err(),
            M6Error::Incomplete {
                operation: "M6 event-line bytes",
                limit: MAX_M6_EVENT_LINE_BYTES,
                observed: usize::MAX,
            }
        );
        assert_eq!(
            checked_working_add(MAX_M6_MAPPING_WORKING_BYTES, 0).unwrap(),
            MAX_M6_MAPPING_WORKING_BYTES
        );
        assert!(checked_working_add(MAX_M6_MAPPING_WORKING_BYTES, 1).is_err());
        assert!(checked_working_add(usize::MAX, 1).is_err());
        let exact = ExactDto {
            a: "x".repeat(MAX_M6_EVENT_LINE_BYTES - 9),
        };
        assert_eq!(
            crate::canonical_json(&exact).unwrap().len() + 1,
            MAX_M6_EVENT_LINE_BYTES
        );
        bounded_event_dto(&exact, MAX_M6_MAPPING_DTO_BYTES, "exact DTO plus LF").unwrap();
        let plus_one = ExactDto {
            a: "x".repeat(MAX_M6_EVENT_LINE_BYTES - 8),
        };
        assert!(
            bounded_event_dto(&plus_one, MAX_M6_MAPPING_DTO_BYTES, "exact DTO plus LF").is_err()
        );
    }

    #[test]
    fn staleness_combined_budget_is_inclusive_and_overflow_closed() {
        assert_eq!(staleness_source_working_limit(19, 7, 12).unwrap(), 7);
        assert!(matches!(
            staleness_source_working_limit(18, 7, 12),
            Err(M6Error::Incomplete {
                operation: "M6 staleness combined working bytes",
                limit: 18,
                observed: 19,
            })
        ));
        assert!(matches!(
            staleness_source_working_limit(usize::MAX, usize::MAX, 1),
            Err(M6Error::Incomplete {
                operation: "M6 staleness combined working bytes",
                observed: usize::MAX,
                ..
            })
        ));
        assert!(matches!(
            staleness_source_working_limit(0, 0, 1),
            Err(M6Error::Incomplete {
                operation: "M6 staleness combined working bytes",
                observed: usize::MAX,
                ..
            })
        ));
    }

    #[test]
    fn staleness_phase_retention_uses_actual_sealed_mapping_and_correspondence() {
        let (_closure, mapping, _source, _target, correspondence) = correspondence_phase();
        let (mapping_bytes, correspondence_bytes) =
            staleness_mapping_correspondence_retained_bytes(&mapping, &correspondence).unwrap();
        let expected_mapping = mapping
            .mappings()
            .iter()
            .fold(mapping.morphism().allocated_bytes(), |total, item| {
                total.checked_add(item.allocated_bytes()).unwrap()
            })
            + mapping.mappings.capacity() * std::mem::size_of::<ProgramMappingV5>();
        let expected_correspondence = correspondence.entries().iter().fold(
            correspondence.correspondence().allocated_bytes(),
            |total, item| total.checked_add(item.allocated_bytes()).unwrap(),
        ) + correspondence.entries.capacity()
            * std::mem::size_of::<ObligationCorrespondenceEntryV5>();
        assert_eq!(mapping_bytes, expected_mapping);
        assert_eq!(correspondence_bytes, expected_correspondence);
        assert!(mapping_bytes > 0 && correspondence_bytes > 0);
    }

    #[test]
    fn seal_accepts_exact_8192_mappings_and_4096_domains_with_streamed_digest() {
        let (source, target) = spaces();
        let closure = closure(&source, &target);
        let source_domain = (0..MAX_M6_PROGRAM_DOMAIN_IDS)
            .map(|index| id(&format!("custom:source-{index:04}")))
            .collect::<BTreeSet<_>>();
        let target_domain = (0..MAX_M6_PROGRAM_DOMAIN_IDS)
            .map(|index| id(&format!("custom:target-{index:04}")))
            .collect::<BTreeSet<_>>();
        let mut mappings = Vec::with_capacity(MAX_M6_MAPPINGS);
        for source_id in &source_domain {
            mappings.push(
                ProgramMappingV5::from_parts(ProgramMappingPartsV5 {
                    source_closure_id: closure.id().clone(),
                    source_snapshot_id: source.snapshot_id().clone(),
                    target_snapshot_id: target.snapshot_id().clone(),
                    object_kind: ProgramObjectKindV5::Artifact,
                    from_ids: BTreeSet::from([source_id.clone()]),
                    to_ids: BTreeSet::new(),
                    status: MappingStatusV5::Removed,
                    candidate_key_kind: CandidateKeyKindV5::NoCandidate,
                    source_body_hashes: vec![IdBodyHashV5::new(source_id.clone(), sha(1)).unwrap()],
                    target_body_hashes: Vec::new(),
                    change_fact_ids: BTreeSet::new(),
                    predecessor_mapping_ids: BTreeSet::new(),
                })
                .unwrap(),
            );
        }
        for target_id in &target_domain {
            mappings.push(
                ProgramMappingV5::from_parts(ProgramMappingPartsV5 {
                    source_closure_id: closure.id().clone(),
                    source_snapshot_id: source.snapshot_id().clone(),
                    target_snapshot_id: target.snapshot_id().clone(),
                    object_kind: ProgramObjectKindV5::Artifact,
                    from_ids: BTreeSet::new(),
                    to_ids: BTreeSet::from([target_id.clone()]),
                    status: MappingStatusV5::Added,
                    candidate_key_kind: CandidateKeyKindV5::NoCandidate,
                    source_body_hashes: Vec::new(),
                    target_body_hashes: vec![IdBodyHashV5::new(target_id.clone(), sha(2)).unwrap()],
                    change_fact_ids: BTreeSet::new(),
                    predecessor_mapping_ids: BTreeSet::new(),
                })
                .unwrap(),
            );
        }
        mappings.sort_by(|left, right| left.id().cmp(right.id()));
        let morphism =
            ChangeMorphismV5::seal_derived(&closure, &mappings, &source_domain, &target_domain)
                .unwrap();
        assert_eq!(morphism.mapping_count(), MAX_M6_MAPPINGS as u64);
        assert_eq!(
            morphism.source_domain_count(),
            MAX_M6_PROGRAM_DOMAIN_IDS as u64
        );
        assert_eq!(
            morphism.target_domain_count(),
            MAX_M6_PROGRAM_DOMAIN_IDS as u64
        );
        assert!(matches!(
            bounded(MAX_M6_MAPPINGS + 1, MAX_M6_MAPPINGS, "M6 program mappings"),
            Err(M6Error::Incomplete { observed, .. }) if observed == MAX_M6_MAPPINGS + 1
        ));
    }

    #[test]
    fn hard_canonical_ids_are_independent_fixtures() {
        let (closure, phase) = phase();
        assert_eq!(
            closure.id().as_str(),
            "incremental-source-closure-v5:sha256:44cc90ad0a8a09c4ecb4a6d547ea7039495f6739bdfa11f4490ec0572148af2b"
        );
        assert_eq!(
            phase.morphism().id().as_str(),
            "change-morphism-v5:sha256:ffdd345adc7dcc87d448f0ae8d2e9b1bf770fd2364aefbb104f2f003359f5ef2"
        );
        assert_eq!(
            phase.mappings().first().unwrap().id().as_str(),
            "program-mapping-v5:sha256:04a21071bd5b70720917396e1ccd4024a62982e84d3ec1892c6ad671129ee459"
        );
    }

    #[test]
    fn successor_normalization_rewrites_only_declared_reference_leaves() {
        let replacements = BTreeMap::from([("run:old".to_owned(), "run:new".to_owned())]);
        let source_execution = serde_json::json!({
            "id": "execution:one",
            "run_id": "run:old",
            "future_token_id": "run:old",
            "tool_calls": [{"argument": "run:old", "artifact_id": "run:old"}],
            "summary": "run:old"
        });
        let target_execution = serde_json::json!({
            "id": "execution:one",
            "run_id": "run:new",
            "future_token_id": "run:new",
            "tool_calls": [{"argument": "run:new", "artifact_id": "run:new"}],
            "summary": "run:new"
        });
        assert!(!exact_substituted_successor_v5(
            HistoricalSourceRecordKindV4::Execution,
            &source_execution,
            &target_execution,
            &replacements,
            &BTreeSet::new(),
        ));

        let obligation_source = serde_json::json!({
            "id": "obligation:one",
            "normalized_target_refs": ["run:old"],
            "summary": "unchanged"
        });
        let obligation_target = serde_json::json!({
            "id": "obligation:one",
            "normalized_target_refs": ["run:new"],
            "summary": "unchanged"
        });
        assert!(exact_substituted_successor_v5(
            HistoricalSourceRecordKindV4::Obligation,
            &obligation_source,
            &obligation_target,
            &replacements,
            &BTreeSet::new(),
        ));

        let cases = [
            (HistoricalSourceRecordKindV4::Obligation, "id"),
            (HistoricalSourceRecordKindV4::ReviewPlan, "id"),
            (HistoricalSourceRecordKindV4::ContextEnvelope, "id"),
            (HistoricalSourceRecordKindV4::Execution, "id"),
            (HistoricalSourceRecordKindV4::Claim, "id"),
            (HistoricalSourceRecordKindV4::ClaimAssessment, "claim_id"),
            (HistoricalSourceRecordKindV4::ArtifactRegistrationV3, "id"),
            (HistoricalSourceRecordKindV4::ArtifactRegistrationV4, "id"),
            (HistoricalSourceRecordKindV4::Evidence, "id"),
            (HistoricalSourceRecordKindV4::EvidenceBinding, "id"),
            (HistoricalSourceRecordKindV4::Verification, "id"),
            (HistoricalSourceRecordKindV4::Decision, "id"),
            (HistoricalSourceRecordKindV4::Finding, "id"),
            (HistoricalSourceRecordKindV4::GluingInputDescriptor, "id"),
            (HistoricalSourceRecordKindV4::ContextCover, "id"),
            (HistoricalSourceRecordKindV4::Section, "id"),
            (HistoricalSourceRecordKindV4::Restriction, "id"),
            (HistoricalSourceRecordKindV4::GluingAttempt, "id"),
            (HistoricalSourceRecordKindV4::GlobalCandidate, "id"),
            (HistoricalSourceRecordKindV4::GluingObstruction, "id"),
        ];
        for (kind, field) in cases {
            let source = serde_json::json!({
                field: "run:old",
                "future_token_id": "run:old",
                "nested": { field: "run:old" },
            });
            let target = serde_json::json!({
                field: "run:new",
                "future_token_id": "run:old",
                "nested": { field: "run:old" },
            });
            assert!(
                exact_substituted_successor_v5(
                    kind,
                    &source,
                    &target,
                    &replacements,
                    &BTreeSet::new(),
                ),
                "{kind:?} must substitute its declared root reference leaf"
            );
            let widened = serde_json::json!({
                field: "run:new",
                "future_token_id": "run:old",
                "nested": { field: "run:new" },
            });
            assert!(
                !exact_substituted_successor_v5(
                    kind,
                    &source,
                    &widened,
                    &replacements,
                    &BTreeSet::new(),
                ),
                "{kind:?} must not rewrite a nested same-name field"
            );
        }

        let source = serde_json::json!({
            "id": "run:old",
            "summary": "before",
            "confidence": 0.1,
            "artifact_hash": "sha256:old",
            "policy": {"id": "run:old"},
            "model": {"id": "run:old"},
            "outcome": {"id": "run:old"},
            "tool_calls": [{"id": "run:old", "argument": "run:old"}],
            "future_extension": {"source_ids": ["run:old"]},
        });
        for (field, changed) in [
            ("summary", serde_json::json!("after")),
            ("confidence", serde_json::json!(0.2)),
            ("artifact_hash", serde_json::json!("sha256:new")),
            ("policy", serde_json::json!({"id": "run:new"})),
            ("model", serde_json::json!({"id": "run:new"})),
            ("outcome", serde_json::json!({"id": "run:new"})),
            (
                "tool_calls",
                serde_json::json!([{"id": "run:new", "argument": "run:new"}]),
            ),
            (
                "future_extension",
                serde_json::json!({"source_ids": ["run:new"]}),
            ),
        ] {
            let mut target = source.clone();
            target["id"] = serde_json::json!("run:new");
            target[field] = changed;
            assert!(
                !exact_substituted_successor_v5(
                    HistoricalSourceRecordKindV4::Execution,
                    &source,
                    &target,
                    &replacements,
                    &BTreeSet::new(),
                ),
                "{field} must remain byte-equal"
            );
        }
    }

    #[test]
    fn successor_substitution_resorts_set_paths_but_preserves_ordered_vectors() {
        let replacements = BTreeMap::from([
            ("obligation:a".to_owned(), "obligation:y".to_owned()),
            ("obligation:z".to_owned(), "obligation:b".to_owned()),
            ("artifact:a".to_owned(), "artifact:y".to_owned()),
            ("artifact:z".to_owned(), "artifact:b".to_owned()),
            ("context:a".to_owned(), "context:y".to_owned()),
            ("context:z".to_owned(), "context:b".to_owned()),
        ]);
        let empty = BTreeSet::new();
        let canonical = |value: Value| {
            serde_json::from_slice::<Value>(&crate::canonical_json(&value).unwrap()).unwrap()
        };

        let root_source = canonical(serde_json::json!({
            "obligation_ids": ["obligation:a", "obligation:z"]
        }));
        let root_target = canonical(serde_json::json!({
            "obligation_ids": ["obligation:b", "obligation:y"]
        }));
        assert!(exact_substituted_successor_v5(
            HistoricalSourceRecordKindV4::Claim,
            &root_source,
            &root_target,
            &replacements,
            &empty,
        ));

        let wave_source = canonical(serde_json::json!({
            "waves": [{"obligation_ids": ["obligation:a", "obligation:z"]}]
        }));
        let wave_target = canonical(serde_json::json!({
            "waves": [{"obligation_ids": ["obligation:b", "obligation:y"]}]
        }));
        assert!(exact_substituted_successor_v5(
            HistoricalSourceRecordKindV4::ReviewPlan,
            &wave_source,
            &wave_target,
            &replacements,
            &empty,
        ));

        for field in ["unknowns", "losses"] {
            let nested_source = canonical(serde_json::json!({
                (field): [{"source_ids": ["artifact:a", "artifact:z"]}]
            }));
            let nested_target = canonical(serde_json::json!({
                (field): [{"source_ids": ["artifact:b", "artifact:y"]}]
            }));
            assert!(exact_substituted_successor_v5(
                HistoricalSourceRecordKindV4::ContextEnvelope,
                &nested_source,
                &nested_target,
                &replacements,
                &empty,
            ));
        }

        let ordered_source = canonical(serde_json::json!({
            "context_pair": ["context:a", "context:z"]
        }));
        let ordered_target = canonical(serde_json::json!({
            "context_pair": ["context:y", "context:b"]
        }));
        let incorrectly_sorted_target = canonical(serde_json::json!({
            "context_pair": ["context:b", "context:y"]
        }));
        assert!(exact_substituted_successor_v5(
            HistoricalSourceRecordKindV4::Restriction,
            &ordered_source,
            &ordered_target,
            &replacements,
            &empty,
        ));
        assert!(!exact_substituted_successor_v5(
            HistoricalSourceRecordKindV4::Restriction,
            &ordered_source,
            &incorrectly_sorted_target,
            &replacements,
            &empty,
        ));
    }

    #[test]
    fn successor_substitution_resorts_only_closed_object_array_containers() {
        let replacements = BTreeMap::from([
            ("obligation:a".to_owned(), "obligation:y".to_owned()),
            ("obligation:z".to_owned(), "obligation:b".to_owned()),
            ("artifact:a".to_owned(), "artifact:y".to_owned()),
            ("artifact:z".to_owned(), "artifact:b".to_owned()),
        ]);
        let empty = BTreeSet::new();
        let canonical = |value: Value| {
            serde_json::from_slice::<Value>(&crate::canonical_json(&value).unwrap()).unwrap()
        };

        for field in ["risk_breakdown", "deferred"] {
            let source = canonical(serde_json::json!({
                (field): [
                    {"id": "obligation:a", "detail": "first"},
                    {"id": "obligation:z", "detail": "second"}
                ]
            }));
            let target = canonical(serde_json::json!({
                (field): [
                    {"id": "obligation:b", "detail": "second"},
                    {"id": "obligation:y", "detail": "first"}
                ]
            }));
            assert!(exact_substituted_successor_v5(
                HistoricalSourceRecordKindV4::ReviewPlan,
                &source,
                &target,
                &replacements,
                &empty,
            ));
        }

        for field in ["included_sources", "excluded_sources"] {
            let source = canonical(serde_json::json!({
                (field): [
                    {"artifact_id": "artifact:a", "detail": "first"},
                    {"artifact_id": "artifact:z", "detail": "second"}
                ]
            }));
            let target = canonical(serde_json::json!({
                (field): [
                    {"artifact_id": "artifact:b", "detail": "second"},
                    {"artifact_id": "artifact:y", "detail": "first"}
                ]
            }));
            assert!(exact_substituted_successor_v5(
                HistoricalSourceRecordKindV4::ContextEnvelope,
                &source,
                &target,
                &replacements,
                &empty,
            ));
        }

        let ordered_source = canonical(serde_json::json!({
            "waves": [
                {"obligation_ids": ["obligation:a"], "wave_index": 0},
                {"obligation_ids": ["obligation:z"], "wave_index": 1}
            ]
        }));
        let ordered_target = canonical(serde_json::json!({
            "waves": [
                {"obligation_ids": ["obligation:y"], "wave_index": 0},
                {"obligation_ids": ["obligation:b"], "wave_index": 1}
            ]
        }));
        let incorrectly_resorted_target = canonical(serde_json::json!({
            "waves": [
                {"obligation_ids": ["obligation:b"], "wave_index": 1},
                {"obligation_ids": ["obligation:y"], "wave_index": 0}
            ]
        }));
        assert!(exact_substituted_successor_v5(
            HistoricalSourceRecordKindV4::ReviewPlan,
            &ordered_source,
            &ordered_target,
            &replacements,
            &empty,
        ));
        assert!(!exact_substituted_successor_v5(
            HistoricalSourceRecordKindV4::ReviewPlan,
            &ordered_source,
            &incorrectly_resorted_target,
            &replacements,
            &empty,
        ));
    }

    #[test]
    fn successor_normalization_covers_closed_nested_canonical_paths_only() {
        // This is the actual canonical ReviewPlan emitted by the roots-bound
        // target fixture, not a hand-written schema approximation.
        let fixture = crate::m6_test_support::DistinctS0S1StalenessFixture::new().unwrap();
        let source_plan = fixture.target_plan_json_for_test();
        assert!(source_plan["waves"].is_array());
        assert!(source_plan["risk_breakdown"].is_array());
        assert!(source_plan["deferred"].is_array());
        let old = source_plan["waves"][0]["obligation_ids"][0]
            .as_str()
            .expect("actual plan wave obligation ID")
            .to_owned();
        let new = "obligation:successor-nested".to_owned();
        let replacements = BTreeMap::from([(old.clone(), new.clone())]);
        let mut target_plan = source_plan.clone();
        let mut changed = false;
        for wave in target_plan["waves"].as_array_mut().unwrap() {
            for id in wave["obligation_ids"].as_array_mut().unwrap() {
                if id.as_str() == Some(old.as_str()) {
                    *id = Value::String(new.clone());
                    changed = true;
                }
            }
            wave["obligation_ids"]
                .as_array_mut()
                .unwrap()
                .sort_by(|left, right| left.as_str().unwrap().cmp(right.as_str().unwrap()));
        }
        for field in ["risk_breakdown", "deferred"] {
            for member in target_plan[field].as_array_mut().unwrap() {
                if member["id"].as_str() == Some(old.as_str()) {
                    member["id"] = Value::String(new.clone());
                    changed = true;
                }
            }
            target_plan[field]
                .as_array_mut()
                .unwrap()
                .sort_by(|left, right| {
                    left["id"]
                        .as_str()
                        .unwrap()
                        .cmp(right["id"].as_str().unwrap())
                });
        }
        assert!(
            changed,
            "actual plan must expose at least one mapped nested ID"
        );
        assert!(exact_substituted_successor_v5(
            HistoricalSourceRecordKindV4::ReviewPlan,
            &source_plan,
            &target_plan,
            &replacements,
            &BTreeSet::new(),
        ));
        let mut bad_plan = target_plan.clone();
        bad_plan["waves"][0]["future_extension"]["obligation_ids"] = serde_json::json!([new]);
        let mut source_with_extension = source_plan.clone();
        source_with_extension["waves"][0]["future_extension"]["obligation_ids"] =
            serde_json::json!([old]);
        assert!(!exact_substituted_successor_v5(
            HistoricalSourceRecordKindV4::ReviewPlan,
            &source_with_extension,
            &bad_plan,
            &replacements,
            &BTreeSet::new(),
        ));

        let source_context = serde_json::json!({
            "id": "context-envelope:old",
            "included_sources": [{"registration_id": "obligation:old", "artifact_id": "obligation:old", "content_hash": "sha256:unchanged"}],
            "excluded_sources": [{"artifact_id": "obligation:old", "reason": "budget"}],
            "unknowns": [{"source_ids": ["obligation:old"], "description": "unchanged"}],
            "losses": [{"source_ids": ["obligation:old"], "description": "unchanged", "affected_properties": ["p"]}],
        });
        let context_replacements =
            BTreeMap::from([("obligation:old".to_owned(), "obligation:new".to_owned())]);
        let mut target_context = source_context.clone();
        for field in ["registration_id", "artifact_id"] {
            target_context["included_sources"][0][field] = serde_json::json!("obligation:new");
        }
        target_context["excluded_sources"][0]["artifact_id"] = serde_json::json!("obligation:new");
        target_context["unknowns"][0]["source_ids"] = serde_json::json!(["obligation:new"]);
        target_context["losses"][0]["source_ids"] = serde_json::json!(["obligation:new"]);
        assert!(exact_substituted_successor_v5(
            HistoricalSourceRecordKindV4::ContextEnvelope,
            &source_context,
            &target_context,
            &context_replacements,
            &BTreeSet::new(),
        ));
        let mut bad_context = target_context.clone();
        bad_context["included_sources"][0]["content_hash"] = serde_json::json!("sha256:changed");
        assert!(!exact_substituted_successor_v5(
            HistoricalSourceRecordKindV4::ContextEnvelope,
            &source_context,
            &bad_context,
            &context_replacements,
            &BTreeSet::new(),
        ));

        let source_registration = serde_json::json!({
            "id": "registration-v4:old",
            "source": {
                "kind": "external_harness_witness",
                "run_id": "obligation:old",
                "snapshot_id": "obligation:old",
                "claim_id": "obligation:old",
                "repository_id": "obligation:old",
                "test_artifact_id": "obligation:old",
                "universe_id": "obligation:old",
                "claim_body_hash": "sha256:unchanged",
            },
        });
        let mut target_registration = source_registration.clone();
        for field in [
            "run_id",
            "snapshot_id",
            "claim_id",
            "repository_id",
            "test_artifact_id",
            "universe_id",
        ] {
            target_registration["source"][field] = serde_json::json!("obligation:new");
        }
        assert!(exact_substituted_successor_v5(
            HistoricalSourceRecordKindV4::ArtifactRegistrationV4,
            &source_registration,
            &target_registration,
            &context_replacements,
            &BTreeSet::new(),
        ));
        target_registration["source"]["claim_body_hash"] = serde_json::json!("sha256:changed");
        assert!(!exact_substituted_successor_v5(
            HistoricalSourceRecordKindV4::ArtifactRegistrationV4,
            &source_registration,
            &target_registration,
            &context_replacements,
            &BTreeSet::new(),
        ));
    }
}
