use crate::StableId;
use thiserror::Error;

/// Closed failure categories emitted by deterministic D1 planning.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PlanningError {
    #[error("input obligation lifecycle is not generated")]
    NonGenerated,
    #[error("input weight is not finite and positive")]
    InvalidWeight,
    #[error("dependency graph contains a cycle")]
    Cycle,
    #[error("input contains a duplicate dependency")]
    DuplicateDependency,
    #[error("input contains a duplicate obligation")]
    DuplicateObligation,
    #[error("input ID kind is invalid")]
    InvalidIdKind,
    #[error("invalid fixed planning budget")]
    InvalidBudget,
    #[error("invalid planning input")]
    InvalidInput,
}

/// Result type used by the validated ReviewGraphen domain boundary.
pub type Result<T> = std::result::Result<T, DomainError>;

/// Typed failures for invalid canonical state or illegal domain operations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    /// A closed deterministic D1 planning failure.
    #[error("planning failure: {0}")]
    Planning(PlanningError),

    /// A bounded deterministic operation could not represent its complete input.
    #[error("{operation} exceeds limit {limit} (observed {observed})")]
    Incomplete {
        operation: &'static str,
        limit: usize,
        observed: usize,
    },
    /// An ID does not follow the stable `kind:payload` grammar.
    #[error("invalid stable ID `{value}`: {reason}")]
    InvalidId { value: String, reason: String },

    /// A hash is not a supported, minimally sized content hash.
    #[error("invalid content hash `{value}")]
    InvalidHash { value: String },

    /// A required field or collection was empty.
    #[error("{field} must not be empty")]
    EmptyField { field: &'static str },

    /// The same identifier was used for non-identical canonical records.
    #[error("ID collision for `{id}`")]
    IdCollision { id: StableId },

    /// A record refers to a source that is not in the relevant space.
    #[error("{owner} `{owner_id}` has dangling reference `{reference}`")]
    DanglingReference {
        owner: &'static str,
        owner_id: StableId,
        reference: StableId,
    },

    /// An operation attempted an impossible lifecycle or disposition change.
    #[error("illegal {axis} transition for `{id}`: {from} -> {to}")]
    IllegalTransition {
        axis: &'static str,
        id: StableId,
        from: String,
        to: String,
    },

    /// A record violates a cross-space trust or traceability rule.
    #[error("validation failure: {0}")]
    Validation(String),

    /// A durable snapshot-source entry does not match the accepted file
    /// artifact it claims to retain.
    #[error("invalid snapshot source bundle entry `{artifact_id}`: {reason}")]
    InvalidSnapshotSourceBundle {
        artifact_id: StableId,
        reason: String,
    },

    /// JSON was malformed or did not match the supported manual adapter shape.
    #[error("JSON adapter failure: {0}")]
    Json(String),

    /// A ProgramSpace input declared the superseded v1 schema; normal
    /// parsing requires an explicit migration to the current schema instead.
    #[error("ProgramSpace input schema `{detected}` requires migration to `{required}`")]
    MigrationRequired { detected: String, required: String },

    /// A ProgramSpace input declared an unknown, missing, or non-string
    /// schema discriminator.
    #[error("unsupported ProgramSpace input schema (detected: {detected:?})")]
    UnsupportedSchema { detected: Option<String> },

    /// Canonical JSON serialization failed.
    #[error("canonical JSON serialization failed: {0}")]
    CanonicalJson(String),

    /// An event sequence was not append-only or replayable.
    #[error("invalid event sequence: {0}")]
    EventSequence(String),

    /// Event-v3 authority state was routed through a legacy replay surface.
    #[error("event-v3 authority replay is required for {operation}")]
    EventV3AuthorityReplayRequired { operation: &'static str },

    /// A canonical v3 prefix failed authority validation at one exact event.
    #[error("event-v3 authority replay refused at sequence {event_sequence}: {reason}")]
    AuthorityReplayRefused { event_sequence: u64, reason: String },

    /// A fresh or replayed authority object was bound to another policy.
    #[error("event-v3 authority policy revision mismatch")]
    AuthorityPolicyMismatch,

    /// A caller-retained replay basis no longer names the exact log tail.
    #[error("event-v3 authority replay basis does not match the current stream tail")]
    AuthorityReplayBasisMismatch,

    /// Fixture authority had no exact claim-bound harness trust root.
    #[error("event-v3 harness trust root is missing")]
    HarnessTrustRootMissing,

    /// Human authority had no exact scoped and valid trust grant.
    #[error("event-v3 human trust root is missing")]
    HumanTrustRootMissing,

    /// A sealed verifier bundle was changed or used at another position.
    #[error("event-v3 verification bundle does not match its sealed scope")]
    VerificationBundleMismatch,

    /// A witness admission was changed, reused, or moved to another position.
    #[error("event-v3 external witness admission does not match its sealed scope")]
    WitnessAdmissionMismatch,

    /// A human decision admission was changed or moved to another position.
    #[error("event-v3 decision admission does not match its sealed scope")]
    DecisionAdmissionMismatch,
}
