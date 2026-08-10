//! Deterministic M0/M1 domain core for ReviewGraphen.
//!
//! The public model maintains strict boundaries between accepted program facts,
//! review claims, evidence, verification, and human decisions. It intentionally
//! stops before ingestion, reviewer execution, verification execution, gluing,
//! and staleness propagation milestones.

mod canonical;
mod context;
mod coverage;
mod error;
mod event;
mod id;
mod planning;
mod program;
mod projection;
mod review;
mod source;
mod synthesize;

// M1 exercises crate-private legacy replay scaffolding. Keeping it inside the
// crate prevents that compatibility builder from becoming a public API.
#[cfg(test)]
extern crate self as reviewgraphen_core;
#[cfg(test)]
#[path = "../tests/m1.rs"]
mod m1_tests;

pub use canonical::{CanonicalJson, canonical_hash, canonical_json, canonical_json_value};
pub use context::{
    BuiltContextProjection, ContextBuildSession, ContextError, ContextPolicyV1,
    ContextSourceRequest, EnvelopeLoss, EnvelopeUnknown, ExcerptRange, ExcludedSourceRef,
    ExclusionReason, ReviewContextEnvelope, SourceArtifactRef, prepare_context,
};
pub use coverage::{Coverage, CoverageMeasure, Ratio};
pub use error::{DomainError, PlanningError, Result};
pub use event::{
    ArtifactRegistered, ArtifactSensitivity, ArtifactSource, DecodedPayload, Event,
    EventAdmissions, EventCommand, EventContractVersion, EventEnvelope, EventLog,
    EventStreamGenesis, EventViewAccounting, EvidenceBindingAdmission, OfflineProjectionState,
    ProjectedContextEnvelopeMetadata, ProjectedFindingMetadata, RunGenesisManifest,
    RunGenesisSnapshot, SnapshotSourceRecordEntry, SnapshotSourcesRecorded, UnreconciledRecordKind,
    UnreconciledRecordMetadata, ValidatedEvent, ValidatedEventView, VerificationAdmission,
    VerifiedV2Genesis, preflight_index_genesis_json_structure,
};
pub use id::{ContentHash, IdRegistry, StableId, VersionTuple};
pub use planning::{
    DeferralReason, PlanBudget, PlannerPolicyV1, ReviewPlan, RiskDescriptor, ScheduleWave, plan,
};
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
pub use source::{SnapshotSourceBundle, SnapshotSourceEntry};
pub use synthesize::{
    ExclusionRecord, MvpRulePack, ObligationBundle, ObligationContract, RuleDescriptor,
    UniverseDescriptor,
};
