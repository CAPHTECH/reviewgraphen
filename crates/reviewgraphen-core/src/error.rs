use crate::StableId;
use thiserror::Error;

/// Result type used by the validated ReviewGraphen domain boundary.
pub type Result<T> = std::result::Result<T, DomainError>;

/// Typed failures for invalid canonical state or illegal domain operations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
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
}
