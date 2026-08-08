//! Deterministic M0/M1 domain core for ReviewGraphen.
//!
//! The public model maintains strict boundaries between accepted program facts,
//! review claims, evidence, verification, and human decisions. It intentionally
//! stops before ingestion, reviewer execution, verification execution, gluing,
//! and staleness propagation milestones.

mod canonical;
mod coverage;
mod error;
mod event;
mod id;
mod program;
mod projection;
mod review;
mod synthesize;

pub use canonical::{CanonicalJson, canonical_hash, canonical_json, canonical_json_value};
pub use coverage::{Coverage, CoverageMeasure, Ratio};
pub use error::{DomainError, Result};
pub use event::{
    Event, EventAdmissions, EventCommand, EventEnvelope, EventLog, EvidenceBindingAdmission,
    VerificationAdmission,
};
pub use id::{ContentHash, IdRegistry, StableId, VersionTuple};
pub use program::{
    AdapterDescriptor, AdapterStatus, Artifact, CapabilityDeclaration, CapabilityState, Evidence,
    EvidenceAdmission, EvidenceDetails, EvidenceSnapshotAdmission, Extraction, InformationLoss,
    Invariant, Limitation, LimitationKind, Location, MigrationLoss, MigrationRecord,
    ProfileDescriptor, ProgramSpace, ProgramSpaceBuilder, Provenance, Relation,
    RepositoryDescriptor, ReviewContext, Severity, SnapshotDescriptor, SourceRef,
    migrate_program_space_v1_to_v2,
};
pub use projection::{AuditProjection, HumanProjection, Projection, ProjectionKind, ReviewReport};
pub use review::{
    ClaimDisposition, ClaimPolarity, Decision, DecisionAdmission, DecisionAuthority,
    DecisionOutcome, EvidenceBinding, EvidenceRelation, Finding, FindingStatus, FindingTrace,
    Freshness, Obligation, ObligationLifecycle, ReviewAggregate, ReviewClaim, ReviewStatus,
    TrustedHumanAdmission, Verification, VerificationOutcome,
};
pub use synthesize::{
    ExclusionRecord, MvpRulePack, ObligationBundle, ObligationContract, RuleDescriptor,
    UniverseDescriptor,
};
