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
mod execution;
mod harness_source_v1;
mod id;
mod m4;
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
    ArtifactRegistered, ArtifactRegisteredV3, ArtifactSensitivity, ArtifactSource,
    ArtifactSourceV3, AuthorityArtifactResolverV3, AuthorityReplayBasisV3, AuthorityTrustRootsV3,
    DecisionInputV3, DecodedPayload, Event, EventAdmissions, EventCommand, EventContractVersion,
    EventEnvelope, EventLog, EventReplayLimits, EventStreamGenesis, EventViewAccounting,
    EvidenceBindingAdmission, ExpectedVerificationAttemptV3, ExternalWitnessAdmissionV3,
    FixtureExecutionReceiptV1, FixtureRegistrationResumeAuthorityV3, HarnessTrustRootInputV3,
    HumanAuthorityCapabilityV3, HumanTrustGrantInputV3, OfflineProjectionState,
    ProjectedContextEnvelopeMetadata, ProjectedFindingMetadata, RunGenesisManifest,
    RunGenesisManifestV3, RunGenesisSnapshot, SnapshotSourceRecordEntry, SnapshotSourcesRecorded,
    StaticVerificationAttemptInspectionV3, TrustedFixtureHarnessV1, UnreconciledRecordKind,
    UnreconciledRecordMetadata, ValidatedArtifactRegistrationV3, ValidatedDecisionV3,
    ValidatedEvent, ValidatedEventView, ValidatedFindingV3, ValidatedVerificationBundleV3,
    VerificationAdmission, VerificationAttemptStageV3, VerificationBundleReceiptV3,
    VerificationBundleResumeAuthorityV3, VerifiedV2Genesis, VerifierArtifactRoleV3,
    preflight_index_genesis_json_structure,
};
pub use execution::{
    AbstentionReason, ExecutionClaimInputV2, ExecutionClaimV2, ExecutionOutcome, ExecutionRecord,
    ExecutionRecordInput, FAKE_REVIEWER_ID, FAKE_REVIEWER_KIND, FIXTURE_PROMPT_TEMPLATE_VERSION,
    MAX_D2_RAW_REVIEWER_BYTES, MAX_D2_RESOLVED_SOURCE_BYTES, MAX_D2_WORKING_BYTES,
    MalformedOutputReason, NO_TOOLS_POLICY_VERSION, NO_TOOLS_SYSTEM_PROMPT_VERSION,
    ResolvedSourceBufferAccounting, ValidatedExecutionBundle,
};
pub use id::{ContentHash, IdRegistry, StableId, VersionTuple};
pub use m4::{
    AssessmentDispositionV3, AssessmentReviewStatusV3, AuthorityScopeDescriptorV3,
    AuthorityScopeV3, ClaimAssessmentV3, DecisionOutcomeV3, DecisionV3, EvidenceBindingV3,
    EvidenceKindV3, EvidenceObservationV3, EvidenceRelationV3, EvidenceV3, FINDING_PROJECTION_ID,
    FIXTURE_DESCRIPTOR_ID, FIXTURE_HARNESS_ID, FIXTURE_HARNESS_REVISION,
    FIXTURE_HARNESS_SOURCE_HASH, FIXTURE_MEDIA_TYPE, FIXTURE_PROCEDURE_ID,
    FIXTURE_TEST_ARTIFACT_ID, FIXTURE_WITNESS_HASH, FindingStatusV3, FindingV3,
    FixedFixtureResultV1, M4_PROPERTY_ID, M4Error, M4Result, M4SensitivityV3, STATIC_DESCRIPTOR_ID,
    STATIC_PROCEDURE_ID, StaticApplicabilityV1, StaticFactEvaluationV1, StaticFactInputV1,
    StaticFactResultV1, StaticRecordProposalV1, StaticScopeV3, VerificationOutcomeV3,
    VerificationV3, VerifierDescriptorV3, VerifierProcedureV3, evaluate_static_fact_v1,
};
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
    ClaimAuthorKind, ClaimDisposition, ClaimPolarity, ContextSourceRegistration, Decision,
    DecisionAdmission, DecisionAuthority, DecisionOutcome, EvidenceBinding, EvidenceRelation,
    FakeAttemptState, Finding, FindingStatus, FindingTrace, Freshness, LegacyClaimV1, Obligation,
    ObligationLifecycle, RawArtifactRegistration, ReviewAggregate, ReviewClaim, ReviewStatus,
    TrustedHumanAdmission, Verification, VerificationOutcome,
};
pub use source::{SnapshotSourceBundle, SnapshotSourceEntry};
pub use synthesize::{
    ExclusionRecord, MvpRulePack, ObligationBundle, ObligationContract, RuleDescriptor,
    UniverseDescriptor,
};
