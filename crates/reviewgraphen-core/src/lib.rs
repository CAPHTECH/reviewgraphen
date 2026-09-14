//! Deterministic ReviewGraphen domain and event core.
//!
//! The public model maintains strict boundaries between accepted program facts,
//! review claims, evidence, verification, and human decisions. Its accepted
//! contracts extend through source-bound M6 incremental mapping, obligation
//! correspondence, and property-sensitive staleness assessment.

mod canonical;
mod context;
mod context_validation_oracle;
mod coverage;
mod error;
mod event;
mod execution;
mod harness_source_v1;
mod id;
mod m4;
mod m5;
#[allow(dead_code)] // M6's crate-only proof seam is consumed by the subsequent Event/Store slice.
pub mod m6;
#[cfg(test)]
mod m6_test_support;
mod planning;
pub mod profile;
mod program;
mod projection;
mod responsibility_family;
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
    BuiltContextProjection, BuiltContextSubjectWindowsV2, BuiltContextSubjectWindowsV3,
    BuiltContextSubjectWindowsV4, ContextBuildEffect, ContextBuildProbe, ContextBuildSession,
    ContextBuildTrace, ContextDenominatorCommitmentV3, ContextError, ContextKnownCardinalityV3,
    ContextLatentCardinalityV3, ContextMaterializedSourceV3, ContextPolicyV1, ContextSourceRequest,
    ContextSubjectBindingErrorV2, ContextSubjectLossV2, ContextSubjectLossV3,
    ContextSubjectOutcomeV3, ContextSubjectOutcomeV4, ContextSubjectWindowsPolicyV2,
    ContextSubjectWindowsPolicyV3, ContextSubjectWindowsPolicyV4, ContextSubjectWindowsSessionV2,
    ContextSubjectWindowsSessionV3, ContextSubjectWindowsSessionV4,
    ContextSubjectWindowsV3ValidationError, ContextSupportLossSummaryV3, ContextValidationBasisV3,
    ContextValidationBasisV4, ContextWindowCandidateV2, ContextWindowInputV2,
    ContextWindowLossReasonV2, ContextWindowRoleV2, ContextWindowRoleV4, ContextWindowV2,
    ContextWindowV3, ContextWindowV4, EnvelopeLoss, EnvelopeUnknown, ExcerptRange,
    ExcludedSourceRef, ExclusionReason, ReviewContextEnvelope,
    SemanticallyValidatedContextSubjectWindowsV3, SemanticallyValidatedContextSubjectWindowsV4,
    SourceArtifactRef, ValidatedContextSubjectWindowsV3, WireValidatedContextSubjectWindowsV3,
    WireValidatedContextSubjectWindowsV4, prepare_context, prepare_subject_windows_v2,
    prepare_subject_windows_v2_with_probe, prepare_subject_windows_v3,
    prepare_subject_windows_v3_with_probe, prepare_subject_windows_v4,
    prepare_subject_windows_v4_for_obligation, resolve_subject_windows_v2,
    validate_subject_windows_v3_against_basis, validate_subject_windows_v3_read_only,
    validate_subject_windows_v3_wire_read_only, validate_subject_windows_v4_against_basis,
    validate_subject_windows_v4_wire_read_only,
};
pub use coverage::{Coverage, CoverageMeasure, Ratio};
pub use error::{DomainError, PlanningError, Result};
pub use event::{
    ArtifactRegistered, ArtifactRegisteredV3, ArtifactRegistrationReceiptV4,
    ArtifactRegistrationV3AtV4Admission, ArtifactRegistrationV3AtV4Receipt, ArtifactRegistrationV4,
    ArtifactSensitivity, ArtifactSource, ArtifactSourceV3, ArtifactSourceV4,
    AuthorityArtifactResolverV3, AuthorityArtifactResolverV4, AuthorityArtifactResolverV5,
    AuthorityHarnessBindingV3Tuple, AuthorityHumanGrantV3Tuple, AuthorityReplayBasisV3,
    AuthorityReplayBasisV4, AuthorityTrustRootsV3, AuthorityTrustRootsV4, AuthorityTrustRootsV5,
    BorrowedArtifactRegistrationProjectionV3, BorrowedArtifactRegistrationProjectionV4,
    BorrowedArtifactSourceProjectionV3, BorrowedArtifactSourceProjectionV4,
    BorrowedClaimAssessmentIterV4, BorrowedClaimAssessmentProjectionV4,
    BorrowedContextCoverProjectionV4, BorrowedContextEnvelopeProjectionV4,
    BorrowedContextExcerptProjectionV4, BorrowedContextExcludedSourceIterV4,
    BorrowedContextExcludedSourceProjectionV4, BorrowedContextIncludedSourceIterV4,
    BorrowedContextIncludedSourceProjectionV4, BorrowedContextLossIterV4,
    BorrowedContextLossProjectionV4, BorrowedContextPolicyLossDescriptionIterV4,
    BorrowedContextPolicyLossDescriptionProjectionV4, BorrowedContextPolicyProjectionV4,
    BorrowedContextUnknownIterV4, BorrowedContextUnknownProjectionV4, BorrowedDecisionProjectionV3,
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
    EventContractVersion, EventEnvelope, EventLog, EventLogV4, EventLogV5, EventReplayLimits,
    EventStreamGenesis, EventViewAccounting, EvidenceAdmissionV4, EvidenceBindingAdmission,
    EvidenceBindingAdmissionV4, ExpectedAuthorityEventV4, ExpectedVerificationAttemptV3,
    ExternalWitnessAdmissionV3, ExternalWitnessAdmissionV4, FindingReceiptV4, FixedHumanDecisionV5,
    FixtureExecutionReceiptV1, FixtureExecutionReceiptV4, FixtureRegistrationResumeAuthorityV3,
    GLUING_INPUT_MEDIA_TYPE_V4, GenesisReproduction, GenesisReproductionMismatch,
    GluingBundleReceiptV4, GluingInputTrustBindingV4, GluingM6AppendLineV5, GluingM6ContinuationV5,
    HarnessTrustRootInputV3, HumanAuthorityCapabilityV3, HumanDecisionRequestV4,
    HumanM6AppendLineV5, HumanM6ContinuationV5, HumanTrustGrantInputV3, InheritedD2EventReceiptV4,
    M5CompletedGluingProfileV4, M5DoubleSubmitAssignmentsV4, M5GluingProfileBasisV4,
    M5GluingProfileHostMaterialV4, M5GluingProfileInformationLossV4, M5GluingProfileInputV4,
    M5GluingProfileInspectionV4, M5GluingWorkRequestV5, M5M6AppendLineV5, M5M6ContinuationV5,
    M6HumanWorkRequestV5, M6NativeHarnessWorkRequestV5, M6ReviewerRuntimeRequestV5,
    M6ReviewerWorkRequestV5, M6StaticWorkRequestV5, NativeM6AppendLineV5, NativeM6ContinuationV5,
    OfflineProjectionState, OpaqueSessionIdentityV4, PartialM6AppendLineV5,
    PostPartialM6AppendLineV5, PostPartialM6ContinuationV5, PreD2M6AppendLinesV5, PreD2M6SessionV5,
    PreparedGluingM6AppendV5, PreparedHumanM6AppendV5, PreparedInheritedD2EventV4,
    PreparedM5M6AppendV5, PreparedNativeM6AppendV5, PreparedPartialM6AppendV5,
    PreparedPostPartialM6AppendV5, PreparedPreD2M6AppendV5, PreparedTerminalCompletedV5,
    PreparedVerificationBundleResumeV4, ProjectedContextEnvelopeMetadata, ProjectedFindingMetadata,
    RecomputedTerminalM6V5, RecoveredM4BundleV4Session,
    ReplayedV5PreIncrementalStructuralPrefixState, RunGenesisBootstrapRequestV4,
    RunGenesisManifest, RunGenesisManifestV3, RunGenesisManifestV4, RunGenesisSnapshot,
    SnapshotSourceRecordEntry, SnapshotSourcesRecorded, StaticVerificationAttemptInspectionV3,
    TerminalMarkerAppendV5, TerminalProofV5, TrustedFixtureHarnessV1, TrustedFixtureHarnessV4,
    TrustedGluingInputAdmissionV4, TrustedGluingInputSourceV4, TrustedHumanAdmissionV4,
    UnreconciledRecordKind, UnreconciledRecordMetadata, V5GluingInputTrustInput,
    V5IndexProjectionRecord, V5InheritedReportProjection, V5ProjectionEventWitness,
    V5StructuralPrefixCoordinates, V5StructuralPrefixStoreFacts, V5TerminalIndexFacts,
    V5TerminalReviewClosure, V5TypedProjectionRecord, ValidatedArtifactRegistrationV3,
    ValidatedDecisionV3, ValidatedDecisionV4, ValidatedEvent, ValidatedEventView,
    ValidatedFindingV3, ValidatedFindingV4, ValidatedFixedM6ReviewerResultV5,
    ValidatedGluingBundleV4, ValidatedM6StaticResultV5, ValidatedVerificationBundleV3,
    ValidatedVerificationBundleV4, VerificationAdmission, VerificationAdmissionV4,
    VerificationAttemptStageV3, VerificationBundleReceiptV3, VerificationBundleReceiptV4,
    VerificationBundleRecoveryV4, VerificationBundleRequestV4, VerificationBundleResumeAuthorityV3,
    VerificationBundleResumeAuthorityV4, VerifiedTerminalReceiptV5, VerifiedV2Genesis,
    VerifierArtifactRoleV3, preflight_index_genesis_json_structure,
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
    VerificationV3, VerifierDescriptorV3, VerifierProcedureV3,
    evaluate_static_fact_result_from_input_v1, evaluate_static_fact_v1,
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
pub use m6::{
    ActionPrerequisiteV5, ArtifactRegistrationV5, CandidateKeyKindV5, ChangeMorphismV5,
    ExistingTargetRecordV5, GluingClaimBindingStatusV5, GluingClaimBindingV5, GluingFreshnessV5,
    GluingRerunActionKindV5, GluingRerunActionV5, GluingRerunPlanSealV5, GluingRerunReasonV5,
    GluingRerunSubjectKindV5, HistoricalAssessmentStatusV5, HistoricalRecordAssessmentV5,
    HistoricalRecordKindV5, IdBodyHashV5, IncrementalSourceClosureReportProjectionV5,
    IncrementalSourceClosureV5, M6Error, M6MappingPhaseV5, M6ObligationCorrespondencePhaseV5,
    M6PreservationPhaseV5, M6Result, M6StalenessPhaseV5, MAX_M6_CANONICAL_BYTES,
    MAX_M6_CLOSURE_DTO_BYTES, MAX_M6_CORRESPONDENCE_DTO_BYTES, MAX_M6_CORRESPONDENCE_ENTRIES,
    MAX_M6_CORRESPONDENCE_PREDECESSOR_IDS, MAX_M6_CORRESPONDENCE_SIDE_IDS,
    MAX_M6_CORRESPONDENCE_WORKING_BYTES, MAX_M6_EVENT_LINE_BYTES, MAX_M6_GLUE_FRESHNESS_RECORDS,
    MAX_M6_HISTORICAL_ASSESSMENTS, MAX_M6_MAPPING_DTO_BYTES, MAX_M6_MAPPING_LINK_IDS,
    MAX_M6_MAPPING_SIDE_IDS, MAX_M6_MAPPING_WORKING_BYTES, MAX_M6_MAPPINGS,
    MAX_M6_MORPHISM_DTO_BYTES, MAX_M6_OBLIGATIONS_PER_UNIVERSE, MAX_M6_PROGRAM_DOMAIN_IDS,
    MAX_M6_RELATION_VISITS, MVP_PROPERTY_IMPACT_POLICY_V5, MappingStatusCountsV5, MappingStatusV5,
    OBLIGATION_CORRESPONDENCE_POLICY_V5, ObligationCorrespondenceEntryV5,
    ObligationCorrespondenceV5, PROGRAM_MAPPING_POLICY_V5, PartialRerunActionKindV5,
    PartialRerunActionV5, PartialRerunPlanV5, PartialRerunSubjectKindV5,
    PreservationArtifactRoleV5, PreservationArtifactV5, PreservationBundleV5,
    PreservationEvidenceV5, PreservationInputParamsV1, PreservationInputV1, PreservationResultV1,
    PreservationVerificationV5, ProgramMappingV5, ProgramObjectKindV5, RUST_SYMBOL_ANCHOR_V1,
    RustSymbolAnchorV1, RustSymbolKindV1, StaleReasonV5, StalenessAssessmentV5,
    StalenessDirectnessV5, UntrustedIncrementalMappingProposalV5,
    derive_untrusted_incremental_mapping_proposal_v5,
};
pub use planning::{
    DeferralReason, PlanBudget, PlannerPolicyV1, ReviewPlan, RiskDescriptor, ScheduleWave, plan,
};
pub use program::{
    AdapterDescriptor, AdapterStatus, Artifact, CapabilityDeclaration, CapabilityState, Evidence,
    EvidenceAdmission, EvidenceDetails, EvidenceSnapshotAdmission, Extraction,
    GitRevisionClosureV1, IncrementalFactsV1, InformationLoss, Invariant, Limitation,
    LimitationKind, Location, MigrationLoss, MigrationRecord, ProfileDescriptor, ProgramSpace,
    ProgramSpaceBuilder, Provenance, RUST_SYMBOL_ANCHOR_EXTRACTOR_V1,
    RUST_SYMBOL_ANCHOR_SYN_VERSION_V1, Relation, RepositoryDescriptor, ReviewContext, Severity,
    SnapshotDescriptor, SourceRef, migrate_program_space_v1_to_v2,
};
pub use projection::{AuditProjection, HumanProjection, Projection, ProjectionKind, ReviewReport};
pub use responsibility_family::{
    AcceptedResponsibilityFamilyStateV1, FamilyAcceptanceV1, FamilyAuthorityV1, FamilyContractV1,
    FamilyExtractorV1, FamilyMaintenanceDecisionV1, FamilyMemberV1,
    RESPONSIBILITY_FAMILY_STATE_V1_SCHEMA,
};
pub use review::{
    ClaimAuthorKind, ClaimDisposition, ClaimPolarity, ContextSourceRegistration, Decision,
    DecisionAdmission, DecisionAuthority, DecisionOutcome, EvidenceBinding, EvidenceRelation,
    FakeAttemptState, Finding, FindingStatus, FindingTrace, Freshness, LegacyClaimV1, Obligation,
    ObligationLifecycle, RawArtifactRegistration, ReviewAggregate, ReviewClaim, ReviewStatus,
    TrustedHumanAdmission, Verification, VerificationOutcome,
};
pub use source::{SnapshotSourceBundle, SnapshotSourceEntry};
pub use synthesize::{
    DTwoLayerCoverage, ExclusionRecord, MvpRulePack, ObligationBundle, ObligationBundleV3,
    ObligationContract, ObligationContractV3, RuleCoverageV3, RuleDescriptor, SingleLayerCoverage,
    UniverseDescriptor, UniverseDescriptorV3, plan_resolved_target_obligations,
};
