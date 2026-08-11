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
mod m5;
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
    ArtifactRegistered, ArtifactRegisteredV3, ArtifactRegistrationReceiptV4,
    ArtifactRegistrationV3AtV4Admission, ArtifactRegistrationV3AtV4Receipt, ArtifactRegistrationV4,
    ArtifactSensitivity, ArtifactSource, ArtifactSourceV3, ArtifactSourceV4,
    AuthorityArtifactResolverV3, AuthorityArtifactResolverV4, AuthorityHarnessBindingV3Tuple,
    AuthorityHumanGrantV3Tuple, AuthorityReplayBasisV3, AuthorityReplayBasisV4,
    AuthorityTrustRootsV3, AuthorityTrustRootsV4, BorrowedArtifactRegistrationProjectionV3,
    BorrowedArtifactRegistrationProjectionV4, BorrowedArtifactSourceProjectionV3,
    BorrowedArtifactSourceProjectionV4, BorrowedClaimAssessmentIterV4,
    BorrowedClaimAssessmentProjectionV4, BorrowedContextCoverProjectionV4,
    BorrowedContextEnvelopeProjectionV4, BorrowedContextExcerptProjectionV4,
    BorrowedContextExcludedSourceIterV4, BorrowedContextExcludedSourceProjectionV4,
    BorrowedContextIncludedSourceIterV4, BorrowedContextIncludedSourceProjectionV4,
    BorrowedContextLossIterV4, BorrowedContextLossProjectionV4,
    BorrowedContextPolicyLossDescriptionIterV4, BorrowedContextPolicyLossDescriptionProjectionV4,
    BorrowedContextPolicyProjectionV4, BorrowedContextUnknownIterV4,
    BorrowedContextUnknownProjectionV4, BorrowedDecisionProjectionV3,
    BorrowedEvidenceBindingProjectionV3, BorrowedEvidenceProjectionV3,
    BorrowedExecutionClaimIterV4, BorrowedExecutionClaimProjectionV4,
    BorrowedExecutionOutcomeProjectionV4, BorrowedExecutionSettingIterV4,
    BorrowedExecutionSettingProjectionV4, BorrowedFindingProjectionV3,
    BorrowedGlobalCandidateProjectionV4, BorrowedGluingAttemptProjectionV4,
    BorrowedGluingBundleProjectionV4, BorrowedGluingInputDescriptorProjectionV4,
    BorrowedGluingObstructionProjectionV4, BorrowedObligationTransitionProjectionV4,
    BorrowedPlanBudgetProjectionV4, BorrowedPlanDeferredIterV4, BorrowedPlanDeferredProjectionV4,
    BorrowedPlanRiskIterV4, BorrowedPlanRiskProjectionV4, BorrowedPlanWaveIterV4,
    BorrowedPlanWaveProjectionV4, BorrowedProjectionPayloadRefV4, BorrowedProjectionPayloadV4,
    BorrowedProjectionScalarV4, BorrowedRestrictionIterV4, BorrowedRestrictionProjectionV4,
    BorrowedReviewExecutionProjectionV4, BorrowedReviewPlanProjectionV4,
    BorrowedRunGenesisManifestProjectionV4, BorrowedSectionIterV4, BorrowedSectionProjectionV4,
    BorrowedSnapshotSourceEntryIterV4, BorrowedSnapshotSourceEntryProjectionV4,
    BorrowedSnapshotSourcesProjectionV4, BorrowedStableIdSetIterV4, BorrowedStableIdSliceIterV4,
    BorrowedStaticStrIterV4, BorrowedStringSetIterV4, BorrowedStringSliceIterV4,
    BorrowedV4EventMetadata, BorrowedVerificationProjectionV3, DecisionAdmissionV4,
    DecisionInputV3, DecisionReceiptV4, DecodedPayload, Event, EventAdmissions, EventCommand,
    EventContractVersion, EventEnvelope, EventLog, EventLogV4, EventReplayLimits,
    EventStreamGenesis, EventViewAccounting, EvidenceAdmissionV4, EvidenceBindingAdmission,
    EvidenceBindingAdmissionV4, ExpectedAuthorityEventV4, ExpectedVerificationAttemptV3,
    ExternalWitnessAdmissionV3, ExternalWitnessAdmissionV4, FindingReceiptV4,
    FixtureExecutionReceiptV1, FixtureExecutionReceiptV4, FixtureRegistrationResumeAuthorityV3,
    GLUING_INPUT_MEDIA_TYPE_V4, GluingBundleReceiptV4, GluingInputTrustBindingV4,
    HarnessTrustRootInputV3, HumanAuthorityCapabilityV3, HumanDecisionRequestV4,
    HumanTrustGrantInputV3, InheritedD2EventReceiptV4, M5DoubleSubmitAssignmentsV4,
    M5GluingProfileBasisV4, M5GluingProfileHostMaterialV4, M5GluingProfileInformationLossV4,
    M5GluingProfileInputV4, M5GluingProfileInspectionV4, OfflineProjectionState,
    OpaqueSessionIdentityV4, PreparedInheritedD2EventV4, PreparedVerificationBundleResumeV4,
    ProjectedContextEnvelopeMetadata, ProjectedFindingMetadata, RecoveredM4BundleV4Session,
    RunGenesisBootstrapRequestV4, RunGenesisManifest, RunGenesisManifestV3, RunGenesisManifestV4,
    RunGenesisSnapshot, SnapshotSourceRecordEntry, SnapshotSourcesRecorded,
    StaticVerificationAttemptInspectionV3, TrustedFixtureHarnessV1, TrustedFixtureHarnessV4,
    TrustedGluingInputAdmissionV4, TrustedGluingInputSourceV4, TrustedHumanAdmissionV4,
    UnreconciledRecordKind, UnreconciledRecordMetadata, ValidatedArtifactRegistrationV3,
    ValidatedDecisionV3, ValidatedDecisionV4, ValidatedEvent, ValidatedEventView,
    ValidatedFindingV3, ValidatedFindingV4, ValidatedGluingBundleV4, ValidatedVerificationBundleV3,
    ValidatedVerificationBundleV4, VerificationAdmission, VerificationAdmissionV4,
    VerificationAttemptStageV3, VerificationBundleReceiptV3, VerificationBundleReceiptV4,
    VerificationBundleRecoveryV4, VerificationBundleRequestV4, VerificationBundleResumeAuthorityV3,
    VerificationBundleResumeAuthorityV4, VerifiedV2Genesis, VerifierArtifactRoleV3,
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
pub use m5::{
    AssignmentCompatibilityV4, AssignmentValueV4, ContextCoverV4, DOUBLE_SUBMIT_ASSIGNMENT_KEY,
    DOUBLE_SUBMIT_GLUING_DESCRIPTOR_ID, DOUBLE_SUBMIT_INVARIANT_ID,
    DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID, DOUBLE_SUBMIT_PROFILE_ID, DOUBLE_SUBMIT_PROPERTY_ID,
    DOUBLE_SUBMIT_REQUIRED_OVERLAP_ID, DOUBLE_SUBMIT_UI_CONTEXT_ID, GlobalCandidateV4,
    GluingAttemptV4, GluingBundleV4, GluingInputDescriptorV4, GluingObstructionKindV4,
    GluingObstructionV4, GluingRequiredResolutionV4, GluingResultV4, M5Error, M5Result,
    M5SeverityV4, MAX_M5_ATTEMPT_SOURCE_IDS, MAX_M5_BUNDLE_CANONICAL_BYTES,
    MAX_M5_CLAIM_SOURCE_IDS, MAX_M5_CONTEXT_MEMBER_IDS, MAX_M5_COVER_DOMAIN_IDS,
    MAX_M5_COVER_SOURCE_IDS, MAX_M5_DESCRIPTOR_CANONICAL_BYTES,
    MAX_M5_DESCRIPTOR_QUALIFICATION_IDS, MAX_M5_OVERLAP_IDS, MAX_M5_PROFILE_SOURCE_IDS,
    MAX_M5_PROFILE_SOURCE_RETAINED_BYTES, MAX_M5_REQUIRED_CONTEXTS, MAX_M5_RESTRICTION_SOURCE_IDS,
    MAX_M5_SECTION_SOURCE_IDS, MAX_M5_SECTION_TRACE_IDS, MAX_M5_SECTIONS,
    MAX_M5_SELECTED_OBLIGATIONS, MAX_M5_STABLE_ID_BYTES, MAX_M5_TRACE_IDS, RestrictionV4,
    SectionV4,
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
