# ADR 0023: M6 Source-Bound Incremental Review and Staleness

- Status: Accepted
- Date: 2026-08-10
- Scope: Defines M6 over Accepted M4 and the exact Accepted M5 contract fixed by
  ADR 0022: a two-run v5 session, deterministic ProgramSpace change morphism,
  obligation correspondence, property-sensitive invalidation, target-snapshot
  preservation verification, native rerun, fresh target gluing, index v6,
  report v5, and a source-bound gate. It does not add a provider adapter,
  generic semantic-diff engine, arbitrary tool runner, cross-repository mapping,
  automatic human acceptance, or mutation of source history.

## Context

M4 and M5 are intentionally single-snapshot. An old `Passed` verification,
human decision, finding, gluing result, report, or index row says nothing by
itself about a new snapshot. The M1 `Freshness` reduction compares snapshot IDs
only. Git rename facts preserve path history but do not establish symbol,
property, evidence, policy, or gluing preservation.

M6 must make partial invalidation and rerun scope deterministic without turning
source authority into target authority. That requires an exact source prefix, a verified commit transition, closed
correspondence and impact policies, immutable historical assessments, and new
target-snapshot records for every current numerator or gate conclusion.

## Decision

### 1. Exact baseline, versioning, and migration

This ADR cannot become Accepted until ADR 0022 is Accepted unchanged with:

```text
source event: reviewgraphen.review_event.v4
source index: reviewgraphen.index_projection.v5 (PRAGMA user_version = 5)
source report: reviewgraphen.review.report.v4
```

M6 introduces:

```text
target event: reviewgraphen.review_event.v5
target index: reviewgraphen.index_projection.v6 (PRAGMA user_version = 6)
target report: reviewgraphen.review.report.v5
```

If an ADR 0022 DTO, identity preimage, registration boundary, event kind,
authority type, limit, or version changes, this ADR must be revised and reviewed.
There is no event upcast, in-place SQLite migration, `ALTER TABLE`, report
import, or authority translation. Source v4 and target v5 journals remain
homogeneous. An older index request returns
`RebuildRequired { found, required: 6 }` without changing the old image.

Every new v5 event DTO is strict, has `deny_unknown_fields`, a displayed schema
discriminator, closed enums, and sorted/unique set arrays. It contains no
`body_hash`. Unless a smaller preimage is stated, its ID is
`StableId::derived(kind, canonical complete DTO except schema/id)`. Index/report
derive `body_hash=sha256(canonical complete DTO including schema/id)`; callers
never supply it.

The `kind` literal is not inferred. The complete V5 ID-kind table is:

| DTO | StableId kind |
| --- | --- |
| IncrementalSourceClosureV5 | `incremental-source-closure-v5` |
| ProgramMappingV5 | `program-mapping-v5` |
| ChangeMorphismV5 | `change-morphism-v5` |
| ObligationCorrespondenceEntryV5 | `obligation-correspondence-entry-v5` |
| ObligationCorrespondenceV5 | `obligation-correspondence-v5` |
| HistoricalRecordAssessmentV5 | `historical-record-assessment-v5` |
| StalenessAssessmentV5 | `staleness-assessment-v5` |
| GluingFreshnessV5 | `gluing-freshness-v5` |
| ArtifactRegistrationV5 | `registration-v5` |
| PreservationEvidenceV5 | `preservation-evidence-v5` |
| PreservationVerificationV5 | `preservation-verification-v5` |
| PartialRerunActionV5 | `partial-rerun-action-v5` |
| PartialRerunPlanV5 | `partial-rerun-plan-v5` |
| GluingRerunActionV5 | `gluing-rerun-action-v5` |
| GluingRerunPlanSealV5 | `gluing-rerun-plan-v5` |
| IncrementalGateV5 (report only) | `incremental-gate-v5` |

### 2. Required source and resolved revision closure

The source must be one confirmed v4 run in the same admitted `StoreRoot`, with
complete D1/D2/M4 state, exactly two trusted `ArtifactRegistrationV4` gluing
inputs, and exactly one complete `gluing_bundle_recorded_v4`. An incomplete or
M5-free source is `IncompleteSourceM5Baseline`; M6 never invents a historical
gluing result. The target predecessor must contain a complete S1
ProgramSpace/profile/universe/plan. Source and target repository identities are
equal; run and snapshot IDs differ; both snapshots are clean.

The deterministic Git adapter supplies resolved full commit OIDs and tree
hashes, not caller revision strings:

```text
S0.resolved_target_commit_oid == S1.resolved_base_commit_oid
S0.target_tree_hash           == S1.base_tree_hash
S1.resolved_target_commit_oid != S1.resolved_base_commit_oid
```

All OIDs are 40 lowercase hexadecimal SHA-1 object IDs for this MVP and all tree
hashes are exact `git:<40 lowercase hex>` values. The S0 commit/tree must equal
the durable source repository artifact attributes; S1 base/target commits and
trees must equal the durable target change-family/repository attributes produced
by the same Git adapter. The opener rederives those equalities from accepted
ProgramSpace bodies and registered snapshot-source CAS bytes. A branch name,
abbreviated OID, host assertion, equal display revision, or equal tree without
equal resolved commit is insufficient.

```rust
IncrementalSourceClosureV5 {
  schema: "reviewgraphen.incremental_source_closure.v5",
  id, repository_id, repository_identity_hash,
  source_run_id, source_genesis_hash, source_confirmed_offset,
  source_tail_hash, source_event_count, source_snapshot_id,
  source_universe_id, source_index_snapshot_hash,
  source_authority_policy_revision_hash,
  source_authority_replay_basis_digest,
  source_resolved_target_commit_oid, source_target_tree_hash,
  source_gluing_bundle_id,
  target_run_id, target_genesis_hash, target_predecessor_offset,
  target_predecessor_tail_hash, target_predecessor_event_count,
  target_snapshot_id, target_universe_id,
  target_predecessor_index_snapshot_hash,
  target_authority_policy_revision_hash,
  target_pre_incremental_authority_replay_basis_digest,
  target_resolved_base_commit_oid, target_base_tree_hash,
  target_resolved_target_commit_oid, target_target_tree_hash
}
```

The identity preimage is every field except schema/id. Event/index contracts are
fixed by the v4/v5 enclosing types and are not caller strings. Offsets count
canonical JSONL bytes including LF. Snapshot hashes cover complete typed index
snapshots. `repository_identity_hash` hashes canonical repository ID plus the
adapter's canonical repository identity.

The source prefix stays immutable input if its journal later grows. Every target
append occurs only while the dual-run session holds both locks (or after reopen
has reacquired both), proves the recorded source offset/count/tail and target
predecessor/current tail, and ignores later source bytes only after the pinned
prefix is proven. Changed/truncated/partial source bytes return
`HistoricalPrefixMismatch`.

### 3. Frozen registrations and the new preservation registration

V5 inherits, byte-for-byte and without widening, both old registration payloads
and tables:

- `artifact_registered(ArtifactRegistrationV3)` and `artifact_registrations`;
- `artifact_registered_v4(ArtifactRegistrationV4)` and
  `artifact_registrations_v4`.

The exact current ADR 0022 shape is quoted here as a dependency, not redefined:

```rust
ArtifactRegistrationV4 {
  schema: "reviewgraphen.artifact_registration.v4",
  id, run_id, cas_hash, media_type, size, sensitivity,
  source: ArtifactSourceV4,
}
ArtifactSourceV4 =
  RunGenesis { kind: "run_genesis", run_id }
| SnapshotIngest { kind: "snapshot_ingest", adapter_id, run_id, snapshot_id }
| ReviewerExecution { kind: "reviewer_execution", execution_id, reviewer_id, run_id }
| VerifierArtifact { kind: "verifier_artifact", claim_id, descriptor_id,
    procedure_version, role: Input|Output, run_id }
| ExternalHarnessWitness { kind: "external_harness_witness", claim_body_hash,
    claim_id, descriptor_id, genesis_hash, harness_id, harness_revision,
    harness_source_hash, policy_revision_hash, procedure_version, property_id,
    repository_id, repository_source_hash, run_id, snapshot_id,
    test_artifact_id, universe_id }
| GluingInput { kind: "gluing_input", descriptor_id, profile_descriptor_id,
    policy_revision_hash, repository_id, repository_source_hash, run_id,
    genesis_hash, snapshot_id, universe_id, plan_id, context_id,
    descriptor_hash, descriptor_size, descriptor_media_type,
    descriptor_sensitivity };
```

Its identity remains `StableId::derived("registration-v4", canonical
{cas_hash,media_type,run_id,sensitivity,size,source})`. Preservation is never
inserted into that enum. Fresh target M5 inputs still use exact
`ArtifactRegistrationV4/GluingInput` records at v5 positions and the frozen M5
bundle predicate.

M6 adds a separate registration only for preservation artifacts:

```rust
ArtifactRegistrationV5 {
  schema: "reviewgraphen.artifact_registration.v5",
  id, run_id, cas_hash, media_type, size,
  sensitivity: CanonicalState,
  source: PreservationArtifactV5,
}
PreservationArtifactV5 {
  kind: "preservation_artifact", run_id, target_snapshot_id,
  target_obligation_id, source_run_id, source_verification_id,
  descriptor_id, procedure_version, role: Input|Output,
}
```

Its identity is `StableId::derived("registration-v5", canonical
{cas_hash,media_type,run_id,sensitivity,size,source})`. The only event is
`artifact_registered_v5`, actor
`verifier:reviewgraphen.structural_preservation@1`, and the only index table is
`artifact_registrations_v5`. Input/output outer tuples must equal their exact
CAS bytes and nested role. No raw v5 registration append is public. Report v5
has a separate required `artifact_registrations_v5` array.

### 4. Dual-run v5 session and replay authority

```rust
ReplayedV5RunSession {
  session_identity: OpaqueSessionIdentity,
  store_root_identity, source_shared_lock, target_exclusive_lock,
  source_run_id, source_genesis_hash, source_confirmed_offset,
  source_tail_hash, source_event_count,
  target_run_id, target_genesis_hash, target_confirmed_tail_hash,
  target_confirmed_event_count, target_next_sequence,
  source: ReplayedV4Aggregate, target: ReplayedV5Aggregate,
  state: Editable | NonEditable,
}
PreIncrementalAuthorityReplayBasisV5 {
  schema: "reviewgraphen.pre_incremental_authority_replay_basis.v5",
  target_run_id, target_genesis_hash, target_confirmed_tail_hash,
  target_confirmed_event_count, target_next_sequence,
  policy_revision_hash,
  inherited_m4_entries: Vec<AuthorityReplayEntryV3AtV5>,
  gluing_input_entries: Vec<GluingInputReplayEntryV4AtV5>,
  basis_digest,
}
AuthorityReplayBasisV5 {
  schema: "reviewgraphen.authority_replay_basis.v5",
  source_closure_id, pre_incremental_basis_digest, source_basis_digest,
  target_run_id, target_genesis_hash, target_confirmed_tail_hash,
  target_confirmed_event_count, target_next_sequence,
  policy_revision_hash,
  inherited_m4_entries: Vec<AuthorityReplayEntryV3AtV5>,
  gluing_input_entries: Vec<GluingInputReplayEntryV4AtV5>,
  preservation_registration_entries: Vec<PreservationRegistrationReplayEntryV5>,
  preservation_verification_entries: Vec<PreservationReplayEntryV5>,
  basis_digest,
}
AuthorityTrustRootsV5 {
  policy_revision_hash, repository_id, repository_source_hash,
  allowed_harness_bindings: BTreeSet<AuthorityHarnessBindingV3Tuple>,
  human_grants: BTreeSet<AuthorityHumanGrantV3Tuple>,
  allowed_gluing_input_bindings: BTreeSet<GluingInputTrustBindingV4AtV5>,
  allowed_preservation_bindings: BTreeSet<PreservationTrustBindingV5>,
}
```

`PreIncrementalAuthorityReplayBasisV5` deliberately has no source closure ID.
Its digest is computed over the complete displayed object except
`basis_digest`, and the closure records that digest. Appending the durable
closure consumes the pre-basis and returns the normal
`AuthorityReplayBasisV5`, whose `source_closure_id` is now known and whose tail,
count, next sequence and digest include the closure event. No object contains an
ID whose own preimage contains that object's digest.
Normal-basis `source_basis_digest` equals the pinned source V4 replay-basis
digest; `pre_incremental_basis_digest` equals the consumed pre-basis digest.

The V3 tuple field sets are exactly ADR 0021; the gluing tuple is exactly ADR
0022 plus its actual v5 envelope predecessor/sequence. Entry records contain
event sequence/ID/kind, record ID/body hash, predecessor hash, complete trust
binding digest, and v5 position digest. A preservation entry additionally binds
both v5 registration IDs, evidence/verification IDs, CAS tuples, descriptor and
procedure. `basis_digest` hashes the complete basis except itself. Trust-root
values are copied into a new host capability; V3/V4 capabilities are not
accepted or translated.

The new binding field sets are exact:

```rust
GluingInputTrustBindingV4AtV5 {
  policy_revision_hash, repository_id, repository_source_hash,
  run_id, genesis_hash, snapshot_id, universe_id, plan_id,
  profile_descriptor_id, context_id, descriptor_id,
  descriptor_hash, descriptor_size, descriptor_media_type,
  descriptor_sensitivity, registration_id,
  source: ArtifactSourceV4::GluingInput,
  predecessor_event_hash, event_sequence,
}
PreservationTrustBindingV5 {
  policy_revision_hash, source_closure_id,
  source_run_id, source_verification_id,
  target_run_id, target_genesis_hash, target_snapshot_id,
  target_obligation_id, descriptor_id, procedure_version,
  input_hash, input_size, input_media_type,
  output_hash, output_size, output_media_type,
  input_registration_id, output_registration_id,
  predecessor_event_hash, first_event_sequence,
}
PreservationReplayEntryV5 {
  source_closure_id, source_verification_id,
  target_obligation_id, input_registration_id, output_registration_id,
  evidence_id, verification_id, descriptor_id, procedure_version,
  input_hash, input_size, input_media_type,
  output_hash, output_size, output_media_type,
  input_event_sequence, input_event_id,
  output_event_sequence, output_event_id,
  verification_event_sequence, verification_event_id,
  predecessor_event_hash, trust_binding_digest, v5_position_digest,
}
PreservationRegistrationReplayEntryV5 {
  event_sequence, event_id, registration_id, registration_body_hash,
  role: Input|Output, target_obligation_id, source_verification_id,
  cas_hash, size, media_type, source: PreservationArtifactV5,
  predecessor_event_hash, trust_binding_digest, v5_position_digest,
}
TrustedGluingInputAdmissionV4AtV5 {
  session_identity, action_id, source_closure_id,
  policy_revision_hash, binding: GluingInputTrustBindingV4AtV5,
  descriptor: GluingInputDescriptorV4,
  registration: ArtifactRegistrationV4,
  cas_hash, cas_size, cas_media_type, cas_sensitivity,
  actor: "engine:reviewgraphen.m5_gluing_input@1",
  predecessor_event_hash, event_sequence
}
TrustedPreservationAdmissionV5 =
  InputRegistration {
    session_identity, source_closure_id, policy_revision_hash,
    binding: PreservationTrustBindingV5,
    registration: ArtifactRegistrationV5,
    predecessor_event_hash, event_sequence
  }
| OutputRegistration {
    session_identity, source_closure_id, policy_revision_hash,
    binding: PreservationTrustBindingV5,
    input_registration_id, registration: ArtifactRegistrationV5,
    predecessor_event_hash, event_sequence
  }
| Verification {
    session_identity, source_closure_id, policy_revision_hash,
    binding: PreservationTrustBindingV5,
    input_registration_id, output_registration_id,
    evidence: PreservationEvidenceV5,
    verification: PreservationVerificationV5,
    predecessor_event_hash, event_sequence
  };
PreservationAdmissionRequestV5 =
  InputRegistration(ArtifactRegistrationV5)
| OutputRegistration(ArtifactRegistrationV5)
| Verification(PreservationEvidenceV5, PreservationVerificationV5);
```

The two registration event positions and verification position are consecutive
within that obligation and are derived from `first_event_sequence`; the entry's
`predecessor_event_hash` is the predecessor of the input registration.
Both trusted admission types have private fields, are nonserializable,
non-deserializable, non-Clone and one-shot. While both locks are held, session
minting finds one byte-equal binding in `AuthorityTrustRootsV5`, decodes and
hashes the exact CAS object, verifies the displayed DTO/outer tuple/actor/tail,
and returns the admission. Append consumes it. A trust-root binding itself,
caller source object or serialized admission is never accepted as append
authority.

The store exposes exactly:

```rust
EventJournalPair::fresh_v5_session(
  StoreRoot, SourceRunIdentity, NewTargetRun, &AuthorityTrustRootsV4,
  &AuthorityTrustRootsV5
) -> Result<(ReplayedV5RunSession, PreIncrementalAuthorityReplayBasisV5)>;
EventJournalPair::replayed_pre_incremental_v5_session(
  StoreRoot, SourceRunIdentity, TargetRunIdentity,
  &AuthorityTrustRootsV4, &AuthorityTrustRootsV5
) -> Result<(ReplayedV5RunSession, PreIncrementalAuthorityReplayBasisV5)>;
EventJournalPair::recover_pre_incremental_v5_session(
  StoreRoot, SourceRunIdentity, TargetRunIdentity,
  &AuthorityTrustRootsV4, &AuthorityTrustRootsV5
) -> Result<(ReplayedV5RunSession, PreIncrementalAuthorityReplayBasisV5)>;
EventJournalPair::replayed_v5_session(
  StoreRoot, SourceRunIdentity, TargetRunIdentity,
  &AuthorityTrustRootsV4, &AuthorityTrustRootsV5
) -> Result<(ReplayedV5RunSession, AuthorityReplayBasisV5)>;
EventJournalPair::recover_replayed_v5_session(
  StoreRoot, SourceRunIdentity, TargetRunIdentity,
  &AuthorityTrustRootsV4, &AuthorityTrustRootsV5
) -> Result<(ReplayedV5RunSession, AuthorityReplayBasisV5)>;
EventJournalPair::recover_v5_phase(
  StoreRoot, SourceRunIdentity, TargetRunIdentity,
  &AuthorityTrustRootsV4, &AuthorityTrustRootsV5
) -> Result<RecoveredV5Session>;
RecoveredV5Session =
  PreIncremental(ReplayedV5RunSession, PreIncrementalAuthorityReplayBasisV5)
| Incremental(ReplayedV5RunSession, AuthorityReplayBasisV5);
```

Fresh creates a v5 genesis and required target predecessor through ordinary
admitted ingestion/planning. The two pre-incremental APIs require that no closure
event exists; the two normal APIs require exactly one durable closure and verify
its pre-basis digest. Replay scans both confirmed prefixes and rechecks every
V4/V5 authority event from CAS and actual position. Recover first performs
canonical tail recovery on target, then the identical scan; source is never
recovered or written by M6. Missing roots, mismatched CAS, uncertain source, a
wrong phase, or prefix mismatch returns a typed refusal and no editable session.
`RecoveredV5Session` is private-field, nonserializable and exists only to resolve
post-sync uncertainty about whether the closure landed; all later callers use
the phase-specific APIs.

Both locks are acquired in ascending canonical `(run_id, lock_role)` order,
regardless of caller order, and held for mint/append/snapshot. Every prepared
mapping seal, correspondence seal, assessment, v4 gluing-input registration,
v5 preservation registration/bundle, rerun action/plan, inherited M4 bundle,
decision/finding, and target M5 bundle is private-field, nonserializable,
non-Clone, one-shot, and binds both prefixes, session identity, policy revision,
target tail, and next sequence.

Fresh M4/human/M5/preservation work is available only through v5 session
methods. They consume new `TrustedFixtureHarnessV5`, `TrustedHumanAdmissionV5`,
`TrustedGluingInputAdmissionV4AtV5`, or
`TrustedPreservationAdmissionV5` capabilities minted from the exact V5 roots:

```rust
mint_reviewer_registration_v3_at_v5(action_id, ReviewerArtifactRequestV5,
  &AuthorityReplayBasisV5) -> PreparedArtifactRegistrationV3AtV5;
append_artifact_registration_v3_at_v5(PreparedArtifactRegistrationV3AtV5,
  &mut AuthorityReplayBasisV5);
admit_external_witness_registration_v3_at_v5(action_id,
  TrustedFixtureHarnessV5, ExternalWitnessRequestV5,
  &AuthorityReplayBasisV5) -> PreparedArtifactRegistrationV3AtV5;
mint_verifier_registration_v3_at_v5(action_id, claim_id, role,
  VerifierArtifactRequestV5, &AuthorityReplayBasisV5)
  -> PreparedArtifactRegistrationV3AtV5;

mint_verification_bundle_v5(action_id, VerificationBundleRequestV5,
  &AuthorityReplayBasisV5) -> PreparedVerificationBundleV5;
append_verification_bundle_v5(PreparedVerificationBundleV5,
  &mut AuthorityReplayBasisV5);
recover_verification_bundle_resume_v5(claim_id, &AuthorityTrustRootsV5)
  -> VerificationBundleResumeAuthorityV5;
resume_verification_bundle_v5(VerificationBundleResumeAuthorityV5,
  &mut AuthorityReplayBasisV5);
mint_decision_v5(action_id, TrustedHumanAdmissionV5, HumanDecisionRequestV5,
  &AuthorityReplayBasisV5) -> PreparedDecisionV5;
append_decision_v5(PreparedDecisionV5, &mut AuthorityReplayBasisV5);

mint_trusted_gluing_input_admission_v4_at_v5(action_id,
  GluingInputDescriptorV4, ArtifactRegistrationV4,
  &AuthorityTrustRootsV5, &AuthorityReplayBasisV5)
  -> TrustedGluingInputAdmissionV4AtV5;
append_artifact_registration_v4_at_v5(TrustedGluingInputAdmissionV4AtV5,
  &mut AuthorityReplayBasisV5);

mint_trusted_preservation_admission_v5(target_obligation_id,
  PreservationAdmissionRequestV5,
  &AuthorityTrustRootsV5, &AuthorityReplayBasisV5)
  -> TrustedPreservationAdmissionV5;
append_artifact_registration_v5(TrustedPreservationAdmissionV5::InputRegistration|
  TrustedPreservationAdmissionV5::OutputRegistration,
  &mut AuthorityReplayBasisV5);
append_preservation_verification_v5(
  TrustedPreservationAdmissionV5::Verification,
  &mut AuthorityReplayBasisV5);
```

The M4 record bodies and `ArtifactRegistrationV4` body remain frozen; only their
envelope positions and admission types are v5. There is no raw registration,
decision, finding, bundle, mapping, seal, or action append API.

All deterministic M6 and post-plan mutation methods are also closed and exact:

```rust
mint_incremental_source_closure_v5(&PreIncrementalAuthorityReplayBasisV5)
  -> PreparedIncrementalSourceClosureV5;
append_incremental_source_closure_v5(PreparedIncrementalSourceClosureV5,
  PreIncrementalAuthorityReplayBasisV5)
  -> (IncrementalSourceClosureReceiptV5, AuthorityReplayBasisV5);

mint_program_mapping_v5(mapping_id, &AuthorityReplayBasisV5)
  -> PreparedProgramMappingV5;
append_program_mapping_v5(PreparedProgramMappingV5, &mut AuthorityReplayBasisV5);
mint_change_morphism_seal_v5(&AuthorityReplayBasisV5)
  -> PreparedChangeMorphismSealV5;
append_change_morphism_seal_v5(PreparedChangeMorphismSealV5,
  &mut AuthorityReplayBasisV5);

mint_obligation_correspondence_entry_v5(entry_id, &AuthorityReplayBasisV5)
  -> PreparedObligationCorrespondenceEntryV5;
append_obligation_correspondence_entry_v5(
  PreparedObligationCorrespondenceEntryV5, &mut AuthorityReplayBasisV5);
mint_obligation_correspondence_seal_v5(&AuthorityReplayBasisV5)
  -> PreparedObligationCorrespondenceSealV5;
append_obligation_correspondence_seal_v5(
  PreparedObligationCorrespondenceSealV5, &mut AuthorityReplayBasisV5);

mint_historical_record_assessment_v5(source_record_id, &AuthorityReplayBasisV5)
  -> PreparedHistoricalRecordAssessmentV5;
append_historical_record_assessment_v5(
  PreparedHistoricalRecordAssessmentV5, &mut AuthorityReplayBasisV5);
mint_gluing_freshness_v5(source_attempt_id, &AuthorityReplayBasisV5)
  -> PreparedGluingFreshnessV5;
append_gluing_freshness_v5(PreparedGluingFreshnessV5,
  &mut AuthorityReplayBasisV5);
mint_staleness_assessment_seal_v5(&AuthorityReplayBasisV5)
  -> PreparedStalenessAssessmentSealV5;
append_staleness_assessment_seal_v5(PreparedStalenessAssessmentSealV5,
  &mut AuthorityReplayBasisV5);

mint_partial_rerun_action_v5(action_id, &AuthorityReplayBasisV5)
  -> PreparedPartialRerunActionV5;
append_partial_rerun_action_v5(PreparedPartialRerunActionV5,
  &mut AuthorityReplayBasisV5);
mint_partial_rerun_plan_seal_v5(&AuthorityReplayBasisV5)
  -> PreparedPartialRerunPlanSealV5;
append_partial_rerun_plan_seal_v5(PreparedPartialRerunPlanSealV5,
  &mut AuthorityReplayBasisV5);

mint_context_envelope_v5(action_id, &AuthorityReplayBasisV5)
  -> PreparedContextEnvelopeV5;
append_context_envelope_v5(PreparedContextEnvelopeV5,
  &mut AuthorityReplayBasisV5);
prepare_obligation_transition_at_v5(action_id, obligation_id,
  next: Planned|InProgress|Completed, &AuthorityReplayBasisV5)
  -> PreparedObligationTransitionAtV5;
append_obligation_transition_at_v5(PreparedObligationTransitionAtV5,
  &mut AuthorityReplayBasisV5);
mint_execution_bundle_v5(action_id, &AuthorityReplayBasisV5)
  -> PreparedExecutionBundleV5;
append_execution_bundle_v5(PreparedExecutionBundleV5,
  &mut AuthorityReplayBasisV5);
mint_gluing_rerun_action_v5(planning_scope_id, action_id,
  &AuthorityReplayBasisV5) -> PreparedGluingRerunActionV5;
append_gluing_rerun_action_v5(PreparedGluingRerunActionV5,
  &mut AuthorityReplayBasisV5);
mint_gluing_rerun_plan_seal_v5(planning_scope_id,
  &AuthorityReplayBasisV5) -> PreparedGluingRerunPlanSealV5;
append_gluing_rerun_plan_seal_v5(PreparedGluingRerunPlanSealV5,
  &mut AuthorityReplayBasisV5);
mint_finding_v5(action_id, &AuthorityReplayBasisV5) -> PreparedFindingV5;
append_finding_v5(PreparedFindingV5, &mut AuthorityReplayBasisV5);
mint_gluing_bundle_v4_at_v5(registration_ids, &AuthorityReplayBasisV5)
  -> PreparedGluingBundleV4AtV5;
append_gluing_bundle_v4_at_v5(PreparedGluingBundleV4AtV5,
  &mut AuthorityReplayBasisV5);
```

The previously listed V5 registration, preservation, verification, decision and
verification-resume methods are the only authority-bearing companions to this
list. `PreparedObligationTransitionAtV5` has private fields, is nonserializable,
non-deserializable, non-Clone and one-shot, and binds the action/obligation,
exact prior and next lifecycle, session identity, both confirmed prefixes,
source closure, policy revision, target tail, next sequence and current basis
digest. Minting recomputes the frozen lifecycle legality and required adjacent
D2 step; append consumes it, writes the unchanged inherited
`obligation_transition` body at a V5 envelope position, and advances
tail/count/next-sequence/basis exactly once without adding an authority replay
entry. A raw transition command or V1-V4
prepared transition is never accepted. Single-event uncertainty is resolved only by
`recover_replayed_v5_session`, which returns the landed prefix or its absence;
the consumed prepared value is never retried. The only partial multi-event
resume is `recover_verification_bundle_resume_v5` followed by
`resume_verification_bundle_v5`. Context/execution/M5/preservation records are
single atomic payload events; their preceding durable registrations remain
independent valid prefixes and are rediscovered on recovery.

Every successful append, including authority-free M6 records and the atomic M5
bundle, advances target tail/count/next sequence and recomputes the v5 basis.
Authority-bearing appends also add exactly their replay entries. Confirmed
pre-durability failure changes neither basis nor exposed session. Uncertainty
consumes the value, leaves the caller basis unchanged, makes the session
noneditable, and requires recovery. Interrupted inherited M4 bundles use a new
`VerificationBundleResumeAuthorityV5`, reconstructed only from V5 roots, exact
CAS, source closure, and remaining v3-body events at v5 positions. No retry or
authority follows from a serialized basis.

### 5. Semantic anchors and deterministic mapping

The sole symbol anchor is `reviewgraphen.rust_symbol_anchor@1`:

```json
{"descriptor":"reviewgraphen.rust_symbol_anchor@1","language":"rust",
 "symbol_kind":"function","signature_shape_hash":"sha256:<64 lowercase hex>",
 "normalized_body_hash":"sha256:<64 lowercase hex>"}
```

`symbol_kind` is `function|method|type`. The versioned Rust extractor hashes a
canonical parsed signature/body after removing spans/comments/formatting only;
identifiers, paths, visibility, qualifiers, ABI, generics and types remain.
Extractor ID/set, syn version, and anchor version must match S0/S1. File hashes,
labels, line overlap, Git similarity, confidence, or model output are not symbol
anchors. A file rename is preserved only for one exact Git `renamed` fact with
equal complete content hash, kind, language and extractor provenance. `copied`
is added, never rename preservation.

Candidate generation is closed by object kind:

| Kind | Candidate key and endpoint order |
| --- | --- |
| repository | equal canonical repository identity; fixed one-to-one |
| snapshot | the fixed S0→S1 pair; always modified |
| file artifact | normalized same path, or the exact same-content Git rename pair |
| Rust symbol artifact | same path/kind/label first; otherwise equal complete anchor tuple |
| every other artifact kind | exact artifact kind, logical label, language option, and normalized location; moves have no candidate |
| relation | relation kind plus ordered mapped source endpoint IDs, ordered mapped target endpoint IDs, and direction; hyperedge order is preserved and never set-sorted |
| context | context kind plus sorted mapped member IDs |
| invariant | property/version plus sorted mapped scope IDs |
| limitation | kind/severity/description plus sorted mapped source IDs |

Relations are considered only after endpoint components are fixed. A relation
with any unresolved endpoint belongs to the same unresolved component; reversing
source/target direction never matches. Context/invariant/limitation candidates
are likewise delayed until all member/source components are fixed.

Candidate stages are exclusive and consume vertices: repository/snapshot;
same-path files; unmatched exact Git-renamed files; same-path/kind/label symbols;
unmatched equal-anchor symbols; exact-key other artifacts; relations; contexts;
invariants; limitations.
A vertex matched by an earlier stage cannot gain a later edge. Within a stage
all equal-key edges are retained before components are formed. Thus a same-path
modified symbol cannot also jump to an equal anchor elsewhere, while duplicate
unmatched anchors remain an explicit ambiguity component.

For each kind, core builds a deterministic bipartite candidate graph, sorts
vertices by `(side,id)`, and finds connected components. Components are
classified exclusively:

| Source count | Target count | Status |
| ---: | ---: | --- |
| 0 | 1 | `added` |
| 1 | 0 | `removed` |
| 1 | 1 | `preserved` iff normalized complete bodies are equal after ID replacement; otherwise `modified` |
| 1 | >1 | `split` |
| >1 | 1 | `merged` |
| >1 | >1 | `unresolved` |

Zero/zero is impossible. A target/source with no key edge becomes its own
added/removed component. Multiple candidate edges are never broken by canonical
first choice. Split, merged and unresolved preserve all candidate IDs.

For the one-to-one equality test, repository uses canonical identity; snapshot
is always modified; same-path files compare kind/language/full content hash,
semantic attributes and extractor provenance after snapshot-ID replacement;
Git-renamed files use the same tuple but permit only the exact old/new path and
path-derived label difference; Rust symbols compare the complete anchor tuple,
kind/language and non-location semantic attributes (a rename/move may change
only declared name/location); other artifacts compare the complete body after
snapshot-ID replacement; relations compare kind, direction, attributes and
ordered mapped endpoints; contexts/invariants/limitations compare their complete
bodies after mapped member/scope/source replacement. No other field is ignored.

```rust
ProgramMappingV5 {
  schema: "reviewgraphen.program_mapping.v5",
  id, source_closure_id, source_snapshot_id, target_snapshot_id,
  object_kind, from_ids, to_ids, status, candidate_key_kind,
  source_body_hashes, target_body_hashes, change_fact_ids,
  predecessor_mapping_ids, successor_ids, source_ids
}
ChangeMorphismV5 {
  schema: "reviewgraphen.change_morphism.v5",
  id, source_closure_id, repository_id, source_snapshot_id,
  target_snapshot_id,
  mapping_policy_descriptor_id: "reviewgraphen.program_mapping@1",
  semantic_anchor_descriptor_id: "reviewgraphen.rust_symbol_anchor@1",
  mapping_count, mapping_set_digest, source_domain_count,
  source_domain_digest, target_domain_count, target_domain_digest,
  status_counts, source_ids
}
```

`object_kind` is exactly `repository|snapshot|artifact|relation|context|
invariant|limitation`; artifact subkind remains in the candidate key/body and is
never erased. Status is the seven-value component status table above.
Mapping identity uses every field except schema/id/source_ids/successor_ids;
those two trace sets remain body-hash protected and exactly recomputed.
`successor_ids` is the sorted target IDs for source-bearing components and empty
for added components. `source_ids` is exactly closure ID ∪ from/to/change facts
∪ predecessor mappings. `status_counts` is the closed object with all seven
keys `preserved,modified,added,removed,split,merged,unresolved`, including zero.
Each digest hashes the canonical ordered array of `{id,body_hash}`; domain
digests hash sorted IDs. The seal contains counts/digests, not the full member
arrays, so it remains under the event limit. Replay recomputes exact domain
coverage, exclusive component ownership, candidates, statuses, predecessor
edges, successor IDs, sources and digests before sealing.

Morphism `source_ids` is exactly `{source_closure_id}`; mapping members are
committed by `mapping_count/mapping_set_digest` and recovered from the preceding
sealed phase, not duplicated into the seal source set.

### 6. Obligation correspondence

Obligations use the same exclusive component algorithm over complete source and
target universes. The candidate key is exactly rule ID, property ID/version,
target kind, semantic key, and the mapped Program target/context/generator sets.
Dependencies are resolved in canonical topological order; a cycle or external
dependency is `InvalidObligationUniverse`.

```rust
ObligationCorrespondenceEntryV5 {
  schema: "reviewgraphen.obligation_correspondence_entry.v5",
  id, morphism_id, from_obligation_ids, to_obligation_ids, status,
  source_mapping_ids, predecessor_entry_ids, successor_obligation_ids,
  source_body_hashes, target_body_hashes, source_ids
}
ObligationCorrespondenceV5 {
  schema: "reviewgraphen.obligation_correspondence.v5",
  id, morphism_id, source_universe_id, target_universe_id,
  policy_descriptor_id: "reviewgraphen.obligation_correspondence@1",
  entry_count, entry_set_digest, source_domain_count,
  source_domain_digest, target_domain_count, target_domain_digest,
  status_counts, source_ids
}
```

Statuses are `preserved|modified|added|removed|split|merged|unresolved`; the
counts object includes all seven zero-valued keys. One-to-one is preserved only
when profile/rule/property/extractor tuple, applicability, evidence requirement,
accepted modes, risk, target/source/context/generator sets and dependency shape
are equal after preserved mappings/correspondences and S0 snapshot replacement.
Entry identity excludes only schema/id/source_ids/successor_obligation_ids.
Source IDs are exactly morphism ID ∪ source mappings ∪ predecessor entries ∪
from/to obligations. Successor IDs are exact target members or empty for added.
The seal digest/domain rules are identical to §5 and full coverage is mandatory.

Correspondence-seal `source_ids` is exactly
`{morphism_id,source_universe_id,target_universe_id}`; entry members are bound by
count/digest. Its entry-set digest hashes ordered `{id,body_hash}`, while both
domain digests hash sorted obligation IDs.

### 7. Closed impact, reason, and action reduction

The sole policy is `reviewgraphen.mvp_property_impact@1`. Traversal uses accepted
ProgramSpace edges only, a sorted frontier, and each relation ID once. `out`
means from ordered source endpoints to ordered target endpoints; `in` is the
reverse. Depth counts traversed relation edges.

| Rule/property | Direct dependency classes | Indirect traversal `(kind,direction,depth)` |
| --- | --- | --- |
| `node.changed_public_symbol@1` / `async.concurrent_reentry` | target artifact, source/context/generator IDs | `handled_by,out,2`; `awaits,out,2`; `writes,out,2` |
| `relation.concurrent_reentry@1` / `async.concurrent_reentry` | relation and ordered endpoints | `handled_by,both,2`; `awaits,out,2`; `writes,out,2` |
| `relation.changed_call_contract@1` / `payment.idempotency_contract` | calls relation, caller, callee, attributes | `calls,both,2`; `covers,in,2`; `reads,out,2` |
| `path.external_side_effect@1` / `payment.at_most_once` | every ordered path relation/endpoint/source/context | `handled_by,both,4`; `calls,both,4`; `covers,in,4` |
| `invariant.payment_at_most_once@1` / `payment.at_most_once` | invariant/scope/source/context/generating obligations | `handled_by,both,4`; `calls,both,4`; `covers,in,4`; `constrains,both,4` |
| `capability_gap.origin_rule@1` / `reviewgraphen.capability_gap` | capability declarations, qualifications, profile/rule/extractor tuple | none |

An API/config/test artifact is indirect only when reached by a listed edge and
direction. Unknown relation/rule/property/policy versions do not widen the walk;
they produce `unsupported_impact_policy` and conservative rerun.

The mapping-status reduction is complete. `position` is direct when the mapped
object occurs in the row's direct classes, otherwise indirect when reached by
the row traversal:

| Mapping/correspondence status | Direct position | Indirect position |
| --- | --- | --- |
| preserved | no reason/action | no reason/action |
| modified | class-specific reason, direct | `dependency_changed`, indirect |
| added | `target_changed`, direct | `dependency_changed`, indirect |
| removed | `target_changed`, direct | `dependency_changed`, indirect |
| split/merged/unresolved | `mapping_unresolved`, direct | `mapping_unresolved`, indirect |

When both positions fire, directness is `direct_and_indirect`. Stale reasons are
the closed enum:

```text
target_changed, dependency_changed, context_changed, evidence_changed,
test_changed, policy_changed, rule_changed, extractor_changed,
model_policy_changed, human_authority_not_carried, mapping_unresolved,
obligation_changed, gluing_input_changed, unsupported_impact_policy
```

The direct class-specific reason is: context→`context_changed`; rule ID/version→
`rule_changed`; profile or impact policy→`policy_changed`; extractor→`extractor_changed`; test artifact→
`test_changed`; evidence/registration/binding→`evidence_changed`; execution
model/prompt/tool tuple→`model_policy_changed`; obligation body→
`obligation_changed`; M5 descriptor/member/overlap/assignment→
`gluing_input_changed`; otherwise `target_changed`.

The record-to-action table is also complete; prerequisite order is left-to-right:

| Source record/dependency result | Target actions |
| --- | --- |
| every admitted preservation (the MVP `issue_present` fixture only) | emit structural preservation records; for context/reviewer/verifier/human, reuse only each exact current-target S1 closure admitted by §11 and otherwise schedule that stage |
| added/modified/split/merged/unresolved obligation or Program/context impact | `reproject_context → rerun_reviewer → rerun_verifier` |
| execution/claim/claim-assessment/model tuple stale | `reproject_context → rerun_reviewer → rerun_verifier` |
| evidence/registration/binding/verification stale | `rerun_verifier`; add reviewer first if no exact current target claim |
| any source active human decision or current finding on the successor | for context/reviewer/verifier/human, reuse only each exact current-target S1 closure admitted by §11 and otherwise schedule that stage; source authority itself satisfies none |
| source M5 descriptor/Section/restriction/attempt/candidate/obstruction, whether preserved or stale, when target gluing is required | seal only the gluing requirement in the first plan; after durable D2 claims the second plan derives `register_gluing_input → rebuild_section → reglue`, with rebuild completion after native verifier |
| removed source with no successor | no target action; historical status `superseded` |
| unsupported policy or no unique successor | conservative reviewer/verifier actions for every candidate target, with context new/reuse per §11; no preservation |

These three tables form the exhaustive Cartesian reduction: select the one
rule/property row, enumerate exactly its direct objects and directed indirect
cone, apply the one mapping-status row to each object, replace a generic direct
reason with the one record-class reason, union/sort reasons, take the maximum
directness, then emit exactly the row's native partial-plan DAG or its declared
post-D2 gluing requirement. Concrete gluing claim/Section actions are selected
only by §11.1 and never occur in the first plan. There is no default
record kind, implicit edge direction, severity heuristic, or caller-selected
action. A combination not represented by the closed rule or record enums is
`unsupported_impact_policy` and uses the final conservative row.

“Active” uses the source decision/finding state recorded at the pinned source
tail. Human authority is never carried, even if structure is preserved. Runtime
evidence TTL is not modeled in the MVP, so there is no runtime-expiry reason or
clock read. `assessment_time` is retained only as a caller-supplied canonical
audit timestamp and does not change this reduction.

#### 7.1 Reduction and predecessor-resolution decisions

The following decisions close ambiguities that otherwise permit equivalent
source histories to produce different M6 members. They are normative for the
MVP implementation.

1. `record_set_digest` uses the §8 closed 21-kind enum/order:
   `obligation,review_plan,context_envelope,execution,claim,claim_assessment,
   artifact_registration_v3,artifact_registration_v4,evidence,
   evidence_binding,verification,decision,finding,gluing_input_descriptor,
   context_cover,section,restriction,gluing_attempt,global_candidate,
   gluing_obstruction,coverage`; within each kind it uses ascending StableId
   order. This is an assessment-member ordering;
   append order remains the separately derived ascending event-ID order in
   §12.
2. A source record that is relevant to more than one obligation produces one
   independently reduced row per obligation. Its reasons are the sorted union
   of those rows and directness is their maximum (`direct_and_indirect` is
   greater than either singleton value); no first matching obligation wins.
   The final `successor_record_ids`, `mapping_ids`,
   `correspondence_entry_ids`, `reasons`, and `dependency_source_ids` are each
   the complete sorted union of their independently reduced rows. Each union
   is capped at 512 IDs/items; exceeding a cap is a typed `Incomplete` refusal,
   never truncation. The
   final record is `structurally_preserved` only when every row is preserved;
   it is `superseded` only when every row meets the semantic-removal predicate
   in item 4. An unsupported, unresolved, unexecuted, or otherwise nonremoved
   row makes the final record `stale`, even when another row has no successor.
3. The historical coverage row is always `superseded` and carries mandatory
   direct `target_changed`. Required stale predecessors additionally union
   their indirect `dependency_changed` reason, yielding
   `direct_and_indirect`; it is never structurally preserved.
4. A semantic removal is `superseded` only for `MappingStatus::Removed` or a
   correspondence component with no target successor. Any unexecuted target
   stage, including a mapped relevant object with no actual target successor
   at the pinned target-predecessor tail, is `stale` and carries mandatory
   direct `target_changed`. Required stale predecessors additionally union
   their indirect reason and may yield `direct_and_indirect`.
   `GluingFreshnessV5` uses the same split.
5. `dependency_source_ids` is the complete sorted union of every required
   transitive closure for the selected source-record rows, excluding the
   row's own record ID. Direct dependencies are included because they are
   depth-one members of that closure. The union is capped at 512 IDs; exceeding
   it is a typed `Incomplete` refusal, never truncation.
6. “Visit each relation once” is scoped to one deterministic policy walk for
   one source-record row. A separate row starts a new sorted frontier and its
   own visited-relation set; sharing a process-global set is forbidden.
7. `assessment_time` is exactly the canonical 20-byte UTC-second string
   `YYYY-MM-DDTHH:MM:SSZ` and uses canonical JSON formatting. Reusing the same
   assessment ID/time with a different body is an ID/body collision refusal;
   time is not rounded, generated, or used as a reduction input.
8. The target predecessor ends immediately before the `source_bound` event at
   its recorded `target_predecessor_offset`, `target_predecessor_event_count`,
   and `target_predecessor_tail_hash`. A roots-bound authority replay must also
   match run ID, genesis hash, and the sealed authority replay-basis digest
   before it determines which target M4 history and optional frozen V4
   descriptor/registration pair(s) or M5 bundle are accepted at that tail.
   Later target events are outside the predecessor and cannot suppress an M6
   action; structural replay supplies coordinates only and suppresses nothing.
   Before `source_bound`, its closed-order recognizer admits the initial
   registration/source/plan prefix, then only inherited D2 records and complete
   M4 `evidence -> [binding ->] verification` bundles, zero to two frozen V4
   registrations, and at most one frozen M5 bundle after exactly two V4
   registrations. It rejects duplicate source/plan records, partial or
   reordered M4 bundles, duplicate/third V4 registrations, inherited D2/M4
   after a V4 registration, and every payload after the M5 bundle. This check
   decodes and drops bodies: it performs no CAS read, authority validation,
   lifecycle/claim reduction, or accepted-state update. M6 payloads cannot
   enter this pre-`source_bound` vocabulary.

The first and fifth action rows put their successor obligation in
`mandatory_native_rerun`. This means source preservation or source human state
never suppresses target work. It does not discard already-current target S1
work: §11 may independently revalidate and reuse an exact target envelope,
Completed reviewer closure, native verification, or V5-human decision/finding.
The first unequal or absent target stage and every downstream stage without its
own exact target predicate are scheduled. A changed context forces
reprojection; a changed reviewer claim/raw/body at an already-Completed target
is a typed refusal because lifecycle cannot be reopened.

### 8. Immutable historical assessment

M6 never edits source events, lifecycle, M4 assessments, decisions, findings,
M5 records, or coverage. It appends target-side assessments:

```rust
HistoricalRecordAssessmentV5 {
  schema: "reviewgraphen.historical_record_assessment.v5",
  id, assessment_id, source_record_kind, source_record_id,
  source_record_body_hash, successor_record_ids, status, directness,
  reasons, dependency_source_ids, mapping_ids,
  correspondence_entry_ids, source_ids
}
StalenessAssessmentV5 {
  schema: "reviewgraphen.staleness_assessment.v5",
  id, source_closure_id, morphism_id, correspondence_id,
  impact_policy_descriptor_id: "reviewgraphen.mvp_property_impact@1",
  assessment_time, record_count, record_set_digest,
  gluing_freshness_count, gluing_freshness_set_digest,
  stale_source_count, stale_source_digest,
  superseded_source_count, superseded_source_digest,
  preservation_candidate_count, preservation_candidate_digest,
  m5_dependent_successor_count, m5_dependent_successor_digest,
  source_ids
}
```

Record kind is exactly `obligation|review_plan|context_envelope|
artifact_registration_v3|artifact_registration_v4|execution|claim|
claim_assessment|evidence|evidence_binding|verification|decision|finding|
gluing_input_descriptor|context_cover|section|restriction|gluing_attempt|
global_candidate|gluing_obstruction|coverage`. Status is
`structurally_preserved|stale|superseded`. Directness is `not_applicable` only
for preserved, otherwise `direct|indirect|direct_and_indirect`. Preserved has an
empty reason set; stale/superseded has a nonempty set. `successor_record_ids` is
the exact sorted current target semantic successors and is never a guessed ID.

Let `M(X)` be exactly the mapping IDs whose `from_ids` or `to_ids` intersect the
ProgramSpace IDs in dependency set `X`, and `C(O)` exactly the correspondence
entry IDs whose source or target obligation sets intersect `O`. “Substituted
equal” means complete body equality after only preserved M/C successor
replacement and S0→S1 snapshot/run/envelope-position replacement. Every source
record kind uses this closed reduction:

| Source record kind | Direct dependencies | Transitive dependency closure | Successor predicate | Exact M/C sets |
| --- | --- | --- | --- | --- |
| obligation | complete obligation body; target/source/context/generator Program IDs; predecessor obligations | predecessor obligations and their Program dependencies | target obligation in its one correspondence component; preserved only for one substituted-equal target | `M(all Program deps)`, its entry plus `C(predecessors)` |
| review_plan | universe/profile/rule/model policy; selected obligations; wave/dependency order; budgets | every selected obligation closure | actual target plan with substituted-equal selected set/order/policy | `M(selected closures)`, `C(selected)` |
| context_envelope | plan/wave/obligations; projection ID/hash/template/loss/source IDs | selected obligation and complete projection Program closure | actual target envelope for exact successor obligations and substituted-equal template/loss/body | `M(projection sources)`, `C(obligations)` |
| artifact_registration_v3 | complete CAS tuple and closed V3 source object | referenced execution/claim/snapshot/Program subjects | actual target V3 registration with substituted-equal source and equal CAS bytes; never inferred from a V4/V5 registration | `M(source subjects)`, `C(source obligations)` |
| artifact_registration_v4 | complete CAS tuple and closed V4 source object | descriptor/claim/snapshot/Program qualification closure | actual fresh target V4 registration with substituted-equal source and equal CAS bytes | `M(source/qualification subjects)`, `C(source obligations)` |
| execution | plan/wave/obligations/envelope; reviewer/provider/model/revision/prompt/inference/tool policy/calls; raw registration | plan, envelope, obligations, registration and Program projection closure | actual target execution at v5 position with exact policy tuple and successor subjects; model prose is never synthesized as successor | `M(envelope/program sources)`, `C(obligations)` |
| claim | execution; obligations/property/target refs/source IDs; polarity/disposition/summary/assumptions/evidence request/author/review status | execution, obligations and all Program target/source refs | actual target structured claim emitted by the successor execution and substituted-equal in every non-run field | `M(target/source refs)`, `C(obligations)` |
| claim_assessment | claim; evidence/binding/verification/decision/finding IDs and derived disposition/review state | complete listed M4 record closures | actual target assessment derived from actual successor records; never copied | union of the listed records' M/C sets |
| evidence | claim/obligations/subjects; evidence kind/descriptor/observation; registration/CAS | claim, registration and subject Program closure | actual target evidence with target snapshot subjects and equal admitted bytes; preservation evidence is not its successor | `M(subjects)`, `C(claim obligations)` |
| evidence_binding | claim/evidence; mode; property; subject/source IDs | evidence, claim and subject Program closure | actual target binding between actual successor claim/evidence with substituted-equal mode/subjects | `M(subjects)`, `C(claim obligations)` |
| verification | claim/evidence/binding; descriptor/procedure/outcome; artifact registrations | complete claim/evidence/binding/registration closure | actual native target M4 verification only; PreservationVerificationV5 is not this successor | union of predecessor M/C sets |
| decision | claim/property/status/authority/grant validity/actor/time/source IDs | claim assessment and authority-bearing verification closure | actual target V5-human decision after native verifier; source active decision is always stale regardless of equality | union of claim/assessment M/C sets |
| finding | claim/decision/property/status/severity/blocks/source IDs | decision and complete claim assessment closure | actual target finding derived after target decision; source current finding is always stale regardless of equality | union of decision/assessment M/C sets |
| gluing_input_descriptor | target snapshot/plan/context/profile; assignment; qualifications; V4 registration | registration, selected Section claim assessment and qualification Program/M4 closure | actual fresh target descriptor plus target V4 registration for the same mapped context; source descriptor is never authority | `M(context/qualifications)`, `C(selected obligation)` |
| context_cover | target plan/universe; selected obligations; contexts; exact cover/covered/uncovered domain | all selected obligation and Program domain closures | cover inside the sole actual target bundle with substituted-equal domain | `M(domain/contexts)`, `C(selected)` |
| section | cover/context/obligation/claim/assessment/descriptor/registration/assignment/trace sets | every named cover/M4/descriptor predecessor | Section inside target bundle with native current verification and exact mapped trace | union of predecessor M/C sets |
| restriction | Section/context pair/overlap/assignment/trace sets | Section plus mapped overlap Program members and M4 trace | restriction inside target bundle with substituted-equal mapped overlap/assignment | `M(overlap)`, union of Section M/C |
| global_candidate | cover/invariant/Sections/restrictions/qualifications/M4 trace | all named M5 and M4 predecessor closures | candidate inside target bundle only when complete substituted result body matches | union of predecessor M/C sets |
| gluing_attempt | cover/profile/invariant/descriptors/Sections/restrictions/result/option IDs/M4 trace | all bundle inputs and option closure | the sole target attempt for mapped profile/invariant; actual result/option body must match | union of bundle M/C sets |
| gluing_obstruction | attempt/kind/contexts/Sections/overlap/assignment/resolution/blocks/M4 trace | all attempt inputs except the ownership back-reference cycle excluded by M5 | obstruction inside target bundle with substituted-equal kind/body; never a finding | union of attempt input M/C sets |
| coverage | universe denominator and every numerator ID/count | all obligations and current M4/M6 records contributing to numerators | no structural successor: target coverage is always freshly recomputed and source coverage is superseded | `M(all denominator deps)`, `C(denominator)` |

The source `coverage` row is the internal immutable derived projection
`HistoricalCoverageSnapshotV4`. It is not an event, table row, authority, or
appendable DTO. Its schema is
`reviewgraphen.historical_coverage_snapshot.v4`, its StableId kind is
`historical-coverage-snapshot-v4`, and its ID preimage is its complete body
excluding only `schema` and `id`. The body is exactly source universe ID,
snapshot ID, profile ID, policy version, rule-set hash, extractor-set hash,
rule-pack version, sorted denominator obligation IDs and count, and the sorted
completed, evidence-supported, verified, fresh, and human-accepted numerator
obligation IDs with each exact count. Numerators remain separate and no state
axis implies another. It is derived from the one pinned replayed V4 aggregate,
is included exactly once as the final historical source record, and is always
`superseded`; it has no structural successor even when its complete body is
equal to newly recomputed target coverage.

For each row, an unequal direct field is direct stale; a stale required
predecessor is indirect stale; both yield `direct_and_indirect`. No row may use a
target record merely because its label, prose, confidence, or suffix matches.
Independently of this table, every source decision active at the pinned tail and
every source current finding is always `stale`, `direct`, with sole mandatory
reason `human_authority_not_carried` plus any other mechanically derived reason,
and requires the current target reviewer/verifier/human closure with every
stage either exactly revalidated or newly scheduled under §11.

The assessment ID preimage is closure/morphism/correspondence/policy/time and is
known before member events. Member `source_ids` is exactly assessment ID,
source record, successors, dependency sources, mappings and correspondence
entries. Record-set digests hash ordered `{id,body_hash}`; result digests hash
sorted source or target IDs. The seal uses counts/digests instead of member
arrays. It validates every pinned source record exactly once in this order:

```text
obligation/plan -> context -> execution/claim/assessment
-> registration/evidence/binding/verification -> decision/finding
-> M5 input/cover/Section/restriction/attempt/candidate/obstruction -> coverage
```

Staleness-seal `source_ids` is exactly
`{source_closure_id,morphism_id,correspondence_id}`; its member/result sets are
bound only by the displayed count/digest pairs. `stale_source_digest` and
`superseded_source_digest` hash sorted source record IDs;
`preservation_candidate_digest` hashes sorted target obligation IDs.
`m5_dependent_successor_digest` hashes the exact sorted set defined in §10.
`gluing_freshness_set_digest` hashes the ordered complete
`{id,body_hash}` array of every GluingFreshnessV5 member. The staleness seal
requires exactly one such member for the source's sole M5 attempt.
Reusing that assessment ID with different member/result digests is an ID/body
collision; the smaller preimage avoids an assessment/member identity cycle but
does not permit a second assessment at the same canonical audit time.

A dependent cannot be more reusable than a required predecessor. Coverage is
always superseded when universe IDs differ. Unsealed members are an incomplete
auditable prefix and cannot feed preservation, plan, report, or gate.

### 9. Target preservation evidence

Old `VerificationV3` never enters a target numerator. The sole preservation
procedure is deterministic, process/network/workspace-write free:

```text
descriptor: reviewgraphen.structural_preservation@1
procedure:  reviewgraphen.structural_preservation.payment_v1
```

It supports only a source fixture-test `Passed VerificationV3` for
`payment.at_most_once`, with one-to-one preserved obligation, every §7 direct
and indirect dependency preserved, exact source claim/evidence/binding/
registration/verification closure, equal fixture descriptor/procedure/witness/
harness/repository/test anchor/rule/profile/extractor/CAS tuple, and equal
deterministic fake reviewer model/prompt/tool/inference tuple. Real-provider
output, any active source human decision/finding, split/merge/unresolved
mapping, or any stale reason in the candidate obligation's §7 Program and
predecessor-obligation dependency cone is
`PreservationUnsupported` and schedules the native table actions.

The source claim/evidence/binding/registration/verification closure above is
validated directly as the preservation witness; those downstream records need
not have structurally-preserved successors at the S1 predecessor. In
particular, S1 may retain the exact substituted D2 Completed reviewer/claim
closure while native M4 evidence, binding, verification, decision, and finding
records are still absent and scheduled under §11.

The reachable positive fixture is the existing `payment.at_most_once`
`issue_present` claim. A passed preservation verification means only that its
structural correspondence and witness mapping were reproduced at S1; it is not
a semantic claim carry-forward, safety pass, acceptance, verification, or
resolution. Its obligation is always included in the current target
reviewer/verifier/human closure with §11 exact reuse/rerun handling and in the
plan's required-human-resolution set.

```rust
PreservationInputV1 {
  schema: "reviewgraphen.preservation_input.v1",
  source_closure_id, morphism_id, correspondence_entry_id,
  source_claim_id, source_evidence_ids, source_verification_id,
  target_snapshot_id, target_obligation_id,
  dependency_mapping_ids, policy_revision_hash
}
PreservationResultV1 {
  schema: "reviewgraphen.preservation_result.v1",
  descriptor_id, procedure_version, input_hash,
  target_snapshot_id, target_obligation_id, outcome: "passed",
  source_verification_id, dependency_mapping_ids
}
PreservationEvidenceV5 {
  schema: "reviewgraphen.preservation_evidence.v5",
  id, target_snapshot_id, target_obligation_id, source_closure_id,
  morphism_id, correspondence_entry_id, source_claim_id,
  source_evidence_ids, source_verification_id, dependency_mapping_ids,
  input_registration_id, output_registration_id,
  descriptor_id, procedure_version, observation: "structure_preserved",
  source_ids
}
PreservationVerificationV5 {
  schema: "reviewgraphen.preservation_verification.v5",
  id, target_snapshot_id, target_obligation_id, evidence_id,
  source_verification_id, descriptor_id, procedure_version,
  outcome: "passed", source_ids
}
```

Input/output media types are exactly
`application/vnd.reviewgraphen.preservation-input+json;version=1` and
`application/vnd.reviewgraphen.preservation-result+json;version=1`. For each
target obligation, CAS put and `artifact_registered_v5` occur input then output,
then one atomic `preservation_verified_v5` event. The v5 session admits both
registrations only from one exact `PreservationTrustBindingV5`; CAS/registration
or policy mismatch yields no verification.

Evidence `source_ids` is exactly closure, morphism, correspondence entry, source
claim/evidence/verification, dependency mappings and both v5 registrations.
Verification `source_ids` is exactly evidence ID and source verification ID.

Report `structurally_preserved` is the target obligations with passed
`PreservationVerificationV5`. It is audit-only. `verified` and
`fresh_verified` are exactly current target native M4 Passed obligations; no
preservation record contributes. Source verification IDs occur in none.
Preservation evidence does
not satisfy M4 `evidence_supported`, create a claim/decision/finding, or make
`accepted`. Active source human state is always native-rerun-only.

### 10. M5 is always fresh target work

`GluingFreshnessV5` is historical audit only:

```rust
GluingFreshnessV5 {
  schema: "reviewgraphen.gluing_freshness.v5",
  id, assessment_id, source_attempt_id, status, reasons,
  dependency_mapping_ids, successor_target_attempt_ids, source_ids
}
```

Its `successor_target_attempt_ids` is empty or the singleton target-predecessor
attempt present when assessment is sealed and found by exact mapped
profile/property/invariant closure. A later post-plan bundle does not rewrite
this historical record. Its
`source_ids` is exactly assessment ID, source attempt, dependency mappings and
that successor set.
Its status uses the §8 three-value enum; structurally preserved requires empty
reasons, while stale/superseded requires a nonempty sorted §7 reason set.

It never carries authority, current gate status, a blocker, or verified/glued
credit. A source `assignment_conflict` remains source history and does not block
the target. Every selected target gluing evaluation requires two target-snapshot
`GluingInputDescriptorV4` CAS objects and two exact `artifact_registered_v4`
events at v5 positions, either as revalidated predecessor
descriptor/registration pairs or as unsuppressed scheduled actions after the
second seal, plus native current-target M4 Sections and one target atomic
`gluing_bundle_recorded_v4`. Preservation
verification cannot satisfy frozen `SectionV4.passed_current_verification`.

`target_gluing_required` is true exactly when the target plan selects at least
one `payment.at_most_once` obligation and target ProgramSpace contains
`invariant:payment-at-most-once`; otherwise it is false and no target descriptor,
Section, bundle, gluing action, or gluing gate requirement is allowed.

Let `Q_target` be exactly the sorted target-plan selected obligation IDs whose
property is `payment.at_most_once`. The complete
`m5_dependent_successor_obligation_ids` set is `Q_target` when
`target_gluing_required`, otherwise empty. There is no graph expansion or
confidence filter. Every ID is included in the staleness-seal count/digest and
may receive preservation coverage, but that coverage can never satisfy or
suppress frozen M5's native M4 verification requirement.

The complete target v5 stream permits zero or one M5 bundle total, including its
predecessor. If one already exists, it is the only target result and must match
the post-D2 gluing rerun plan exactly; another `register_gluing_input`, `rebuild_section`, or
`reglue` action is invalid. Otherwise both target descriptors/registrations must
be present, but only unsuppressed registration actions occur after the
gluing-plan seal; the target bundle itself occurs after the seal. Thus zero,
one, or two post-seal registrations are legal according to the exact
suppression predicates. A second target bundle is always
`AlreadyComplete`.

### 11. Subject-bound partial rerun

```rust
ExistingTargetRecordV5 { record_id, body_hash, event_id }
ActionPrerequisiteV5 =
  ScheduledAction { kind: "scheduled_action", action_id }
| ExistingTargetRecord { kind: "existing_target_record",
    record_id, body_hash, event_id };

PartialRerunActionV5 {
  schema: "reviewgraphen.partial_rerun_action.v5",
  id, staleness_assessment_id, subject_kind, subject_ids, action,
  prerequisites: Vec<ActionPrerequisiteV5>,
  stale_source_record_ids, reasons,
  source_ids
}
PartialRerunPlanV5 {
  schema: "reviewgraphen.partial_rerun_plan.v5",
  id, source_closure_id, morphism_id, correspondence_id,
  staleness_assessment_id,
  planner_descriptor_id: "reviewgraphen.partial_rerun@1",
  target_plan_id, selected_target_count, selected_target_digest,
  action_count, action_set_digest,
  preservation_verification_count, preservation_verification_digest,
  required_human_resolution_count, required_human_resolution_digest,
  source_ids
}
```

`subject_kind` is exactly `obligation`; IDs are
nonempty sorted target IDs. Action is
`reproject_context|rerun_reviewer|rerun_verifier|rerun_human_decision`.
Every derived action is scheduled; the MVP has no deferral or budget omission.
Prerequisites are sorted by `(kind,action_id|record_id,event_id)`, contain no
duplicate, share the same subject closure and form the §7 DAG. Identity includes every field except schema/id/
source_ids. `source_ids` is exactly the assessment ID, subject IDs, stale-source
record IDs, every `ScheduledAction.action_id`, and every
`ExistingTargetRecord.record_id/event_id`; reason witnesses add exactly their
record IDs. Counts include zero and digests hash sorted IDs or
`{id,body_hash}`.

Plan `source_ids` is exactly source closure, morphism, correspondence,
staleness assessment and target plan IDs. `selected_target_digest` hashes sorted
target obligation IDs; `action_set_digest` and
`preservation_verification_digest` hash ordered `{id,body_hash}` records.
`required_human_resolution_digest` hashes sorted target obligation IDs. That set
is exactly (a) obligations with a preservation verification whose source claim
polarity is `issue_present`, union (b) successors of source active decisions or
current findings.

Action/subject combinations are closed:

| Action | Subject kind / exact IDs |
| --- | --- |
| reproject_context, rerun_reviewer, rerun_verifier, rerun_human_decision | `obligation` / one successor target obligation ID |

No other action/subject pair validates. “Same subject closure” includes the
successor obligation and every target record selected by a prerequisite.

Action state is a report-derived enum `pending|complete`; no completion event or
caller Boolean exists. Completion uses the exact subject closure:

| Action | Exact completion witness |
| --- | --- |
| reproject_context | post-seal `Planned` and `InProgress` transition event IDs plus the post-seal target `ReviewContextEnvelope` for the subject/current projection |
| rerun_reviewer | the Planned/InProgress transition event IDs, satisfying existing-or-new envelope ID, target reviewer raw-response registration, atomic execution and its exactly one structured claim ID, followed by the `Completed` transition event ID |
| rerun_verifier | native target M4 terminal verification for the exact current claim |
| rerun_human_decision | target decision and resulting finding under a V5 human grant after verifier |

The prerequisite alternatives and suppression predicates are exact and apply
to every subject, including `mandatory_native_rerun`, but only current target S1
records can satisfy them. A source preservation/decision/finding or source-side
body equality is never a prerequisite witness. Each suppressed stage is
replaced in its emitted immediate dependent by the displayed typed target
witness:

| Action | Required prerequisites when emitted | Suppressed iff predecessor contains |
| --- | --- | --- |
| reproject_context | empty | for any subject, one current-S1 envelope for the subject obligation, target plan/wave/template, recomputed exact Program source set/hash and loss declaration; when reviewer remains scheduled, its frozen lifecycle history is Generated→Planned→InProgress before the envelope and current state is still InProgress |
| rerun_reviewer | exactly `ScheduledAction(reproject_context_id)`; if reproject is suppressed, exactly `ExistingTargetRecord(envelope.id,envelope.body_hash,envelope.event_id)` instead | the satisfying target envelope plus one target S1 atomic execution/claim closure equal in every plan, wave, envelope, snapshot, obligation, reviewer descriptor, reviewer/model/revision/prompt/inference/tool, raw registration/CAS, exactly-one complete claim, structured outcome, body hash and event field, followed by its valid Completed transition |
| rerun_verifier | exactly `ScheduledAction(rerun_reviewer_id)`; if reviewer is suppressed, exactly two `ExistingTargetRecord` values for its execution and structured claim | the satisfying actual target claim plus one target S1 native M4 terminal verification whose complete evidence/binding/registration/harness/descriptor/procedure/authority closure validates at the predecessor tail; preservation or source verification never satisfies |
| rerun_human_decision | exactly `ScheduledAction(rerun_verifier_id)`; if verifier is suppressed, exactly `ExistingTargetRecord(native_verification.id,body_hash,event_id)` | the satisfying target S1 native verifier plus one target S1 V5-grant decision and derived current finding for the exact claim/property/status/actor/grant/body/event closure, both valid at the predecessor tail; source human state never satisfies |

An existing predecessor record suppresses an action only under the exact §11
suppression predicates above; every recorded action requires a post-seal witness.
Whenever a predecessor action is suppressed, every emitted immediate dependent
must contain the displayed `ExistingTargetRecord` replacement; it cannot omit
the prerequisite or retain a dangling `ScheduledAction`. Existing record event
IDs must be at/before the target predecessor tail and body hashes must equal the
replayed record. The partial-plan seal itself records the exact revalidation of
every suppressed target stage through the source closure's pinned target-
predecessor tail/hash and the deterministic action-set digest; no duplicate
envelope, lifecycle transition or separate witness event is appended. Emitted
immediate dependents additionally carry the displayed `ExistingTargetRecord`
values. Every recorded action otherwise requires its displayed post-seal
witnesses.
All selected target IDs remain in the selected-target digest.

For the verifier's two-record replacement, validating the execution record also
walks its exact registered raw-response ID/body/CAS closure; the registration is
therefore a required reviewer completion witness even though it is not a third
`ExistingTargetRecord` prerequisite value.

`ExactTargetReviewerClosureV5(o)` is a closed predicate. It requires the exact
target S1 envelope; one atomic structured execution whose complete fields equal
the recomputed plan, wave, envelope, snapshot, obligation set, reviewer ID and
descriptor, model/revision, prompt/inference/tool tuple, raw registration ID and
complete registration/CAS bytes, outcome and execution body hash; the complete
singleton claim set with its complete claim DTO/body hash; one shared exact
execution event tuple (`event_id,sequence,actor,logical_time,payload_hash,
event_hash`); and a later exact `obligation_transition(Completed)` event for the
subject with current lifecycle still Completed. Plan sealing recomputes this
predicate from journal/CAS/index bytes. If true, `rerun_reviewer` is omitted and
its verifier dependent contains exactly the execution and selected-claim
`ExistingTargetRecord` prerequisites. No lifecycle or witness event is appended.
If any claim, raw byte/registration, outcome, body hash, event field or Completed
closure differs, suppression is forbidden; when lifecycle is already Completed
the plan refuses `CompletedReviewerClosureMismatch` rather than reopening it.

M6 narrows only its incremental profile to exactly one parsed claim per
obligation-bound reviewer execution. General D2 remains frozen at 1..16 claims
outside an M6 session. For both a scheduled fresh execution and a candidate
reused `ExactTargetReviewerClosureV5`, core counts the complete parsed claim set
before any Completed transition, verifier preparation, human preparation, or
post-review selection:

```rust
M6ClaimCardinalityUnsupported { execution_id, observed }
```

Exactly one binds that sole claim ID/body/event tuple to the reviewer completion
witness. Zero or greater than one returns the displayed typed obstruction,
never accepts a caller-selected index/ID, never appends Completed for a fresh
attempt, and permits no `PreparedVerificationBundleV5`, verifier input,
`PreparedDecisionV5`, or human input to be generated. A reused Completed
execution with zero/multiple claims is handled by this cardinality obstruction
before `CompletedReviewerClosureMismatch`; lifecycle is not reopened and no
reviewer/verifier/human executable operation is generated. Presealed
`PartialRerunActionV5` values are obligation-bound requirements, not executable
claim-selection authority: their mint paths remain unavailable until this
exactly-one reduction succeeds.

When the reviewer prerequisite was `ScheduledAction`, successful reduction
resolves it internally to exactly two immutable inputs
`ExistingTargetRecord(execution.id,body_hash,event_id)` and
`ExistingTargetRecord(claim.id,body_hash,event_id)`. A reused reviewer already
contains those two values. `mint_verification_bundle_v5` derives the sole claim
from them and rejects any unequal caller request; verifier evidence/binding and
the later human request carry that same claim ID/body transitively. No ordering,
confidence, polarity or caller choice selects a claim.

For each scheduled reviewer action, ascending action-ID order is noninterleaved
and exact:

```text
Generated --obligation_transition(Planned)--> Planned
Planned   --obligation_transition(InProgress)--> InProgress
InProgress -- context envelope: append scheduled envelope, or consume the
              plan-sealed ExistingTargetRecord without another event
InProgress -- artifact_registered(ReviewerExecution V3)
InProgress -- one atomic review_execution_recorded {execution, claims}
InProgress -- iff claim_count==1: obligation_transition(Completed) --> Completed
```

The existing-envelope branch is legal only when its replayed Planned and
InProgress transitions precede that envelope, the partial-plan seal revalidated
all §11 fields/body/event position, and lifecycle remains InProgress with no
intervening structured execution or Completed transition. Earlier bounded
abstained/malformed/provider-failure attempts may remain in the exact attempt
set and do not block the next scheduled attempt. It appends neither transition
nor envelope again. The
new-envelope branch requires Generated at the plan seal and appends both
transitions before the envelope. A mismatched envelope, Planned-only
predecessor, Stale/Superseded/Cancelled state, or duplicate envelope is a typed
refusal, not a fallback append. Completed state reaches this scheduled branch
only after `ExactTargetReviewerClosureV5` failed and therefore refuses as above.
The final Completed transition is
legal only after the atomic structured execution is durable and every addressed
subject matches; abstained/malformed/provider-failure cannot complete the
action or advance to gluing selection.

#### 11.1 Post-review gluing plan

The first plan seals no context-specific claim or Section. When
`target_gluing_required=false`, no second plan or gluing action is allowed and
the report's required `gluing_rerun_plan` value is `null`. When it is true,
after all scheduled D2 reviewer actions have durable structured claims and
their following Completed transitions, core derives exactly one second plan:

```rust
GluingClaimBindingV5 {
  context_id, status: Selected|Missing,
  claim_id: Option<StableId>, claim_body_hash: Option<Hash>,
  obligation_id: Option<StableId>
}
GluingRerunActionV5 {
  schema: "reviewgraphen.gluing_rerun_action.v5",
  id, planning_scope_id, subject_kind, subject_ids, action,
  prerequisites: Vec<ActionPrerequisiteV5>, reasons, source_ids
}
GluingRerunPlanSealV5 {
  schema: "reviewgraphen.gluing_rerun_plan.v5",
  id, planning_scope_id, source_closure_id, partial_rerun_plan_id,
  target_plan_id,
  selection_descriptor_id: "reviewgraphen.m5_claim_selection@1",
  claim_bindings: [GluingClaimBindingV5; 2],
  action_count, action_set_digest,
  existing_target_bundle_witness: Option<ExistingTargetRecordV5>,
  source_ids
}
```

For each required context `c`, core evaluates ADR 0022's actual claim rule over
durable target D2 claims, including revalidated predecessor claims that exactly
suppress a first-plan reviewer action: property `payment.at_most_once`, exactly one addressed
`Q_target` obligation, and at least one claim source in the exact context member
set `M(c)`. One candidate yields `Selected` with its actual claim/body/
obligation; zero yields `Missing` and all option fields absent; more than one is
`AmbiguousGluingClaimSelection` and no plan. Bindings are payment then ui-event.
“D2 selection ready” means every scheduled reviewer action satisfies the full
§11 completion witness through its Completed transition and every suppressed
reviewer closure has been revalidated at the plan predecessor.
The `planning_scope_id` hashes source closure, first plan, target plan and both
complete bindings using StableId kind `gluing-rerun-scope-v5`. No concrete M5
claim/obligation/Section binding occurs in the first plan. A gluing action's
identity includes every field except schema/id/source_ids. Its `source_ids` is
exactly the planning scope ID, subject IDs, every
`ScheduledAction.action_id`, and every
`ExistingTargetRecord.record_id/event_id`. The seal's `source_ids` is exactly the
source closure ID, partial plan ID, target plan ID, planning scope ID, both
selected claim IDs (omitting absent options), every action ID, and the existing
target attempt record/event IDs when present. Its identity includes every
displayed field except schema/id/source_ids. `action_count` counts the complete
derived gluing-action set including zero, and `action_set_digest` hashes its
ascending `{id,body_hash}` records; callers supply neither value. The two claim
bindings are fixed-order complete values, not an unsealed caller array.

Every recorded gluing action has exactly the singleton reason
`fresh_target_gluing_required`; this is the sole closed
`GluingRerunReasonV5` value. Source M5 stale/preserved reasons remain only in
their immutable assessment records and never vary the second-plan action body.
Consequently no reason contributes an implicit record ID: a gluing action's
exact `source_ids` formula above ends after its explicit planning scope,
subjects, prerequisite IDs/event IDs, and no additional “reason witnesses” are
permitted.

Gluing action is `register_gluing_input|rebuild_section|reglue`.
`register_gluing_input` has one `gluing_context` subject and empty prerequisites.
A `Selected` context has one `rebuild_section` action with:

- its registration as `ScheduledAction`, or existing descriptor and V4
  registration as exactly two `ExistingTargetRecord` prerequisites; and
- its obligation's first-plan `rerun_verifier` as `ScheduledAction`, or the
  exact native verification as one `ExistingTargetRecord` prerequisite.

A `Missing` context has no rebuild action. The singleton `reglue` subject is
`gluing_attempt/{invariant:payment-at-most-once}`. Its prerequisites are, for
both contexts, the scheduled registration or exact existing descriptor/
registration pair, plus each Selected context's scheduled rebuild. A Section
cannot be an independent predecessor witness because frozen M5 persists all
Sections only inside the atomic bundle. These are the only allowed prerequisite
variants. Every independently suppressed registration is therefore replaced by
its typed existing witnesses.

Action/subject combinations are closed:

| Action | Subject kind / exact IDs |
| --- | --- |
| register_gluing_input | `gluing_context` / the singleton `{context_id}` for payment or ui-event |
| rebuild_section | `gluing_context` / the singleton `{context_id}` of its Selected binding |
| reglue | `gluing_attempt` / the singleton `{invariant:payment-at-most-once}` |

No other kind, empty/multipleton subject set, or cross-context ID validates.

The gluing suppression/completion predicates are exact:

| Action | Suppressed iff predecessor contains | Post-second-seal completion witness |
| --- | --- | --- |
| register_gluing_input | the context's frozen V4 descriptor and registration with exact target run/snapshot/universe/plan/profile/context, CAS tuple, body hashes and valid V5 envelope position | that same descriptor/registration pair newly appended after the seal |
| rebuild_section | never independently; a predecessor Section implies a predecessor atomic bundle, handled by the all-actions-zero rule below | one frozen V4 Section inside the newly appended atomic bundle, with the actual selected claim/body, exact context membership, and native current-target Passed verification trace |
| reglue | one complete target M5 bundle satisfying the equality predicate below | the complete target M5 attempt/bundle event appended after both registrations and all native verifications; its Sections and attempt are atomic members of that same event |

Suppression is evaluated only against records at or before the target
predecessor tail used by the first plan. A suppressed registration contributes
its descriptor and registration as two `ExistingTargetRecord` prerequisites to
each immediate dependent. Without an existing bundle, every Selected binding
therefore emits `rebuild_section`, and `reglue` refers to it only as
`ScheduledAction`. No other record shape suppresses a gluing action.

If a target M5 bundle already exists, all gluing actions are suppressed only
when every frozen field equals the newly derived target plan: run, snapshot,
universe, plan, profile, selected obligations, contexts, cover partition, both
descriptor/registration bodies, assignments, qualifications, actual claim and
assessment IDs, native verification traces, Sections, overlap, restrictions,
attempt result and candidate/obstruction option. The seal then has zero actions
and its existing witness is exactly the attempt ID/body/event. Any inequality is
`ExistingTargetM5Mismatch`. Without a bundle, the witness is absent and every
action not suppressed by the exact predicates above is sealed. Gluing action
completion uses only the displayed post-second-seal witnesses.

### 12. Event order, crash recovery, and limits

V5 inherits all v4 payload kinds unchanged and adds exactly:

```text
incremental_source_bound_v5(IncrementalSourceClosureV5)
program_mapping_recorded_v5(ProgramMappingV5) repeated
change_morphism_sealed_v5(ChangeMorphismV5)
obligation_correspondence_entry_recorded_v5(ObligationCorrespondenceEntryV5) repeated
obligation_correspondence_sealed_v5(ObligationCorrespondenceV5)
historical_record_assessed_v5(HistoricalRecordAssessmentV5) repeated
gluing_freshness_recorded_v5(GluingFreshnessV5) repeated
staleness_assessment_sealed_v5(StalenessAssessmentV5)
artifact_registered_v5(ArtifactRegistrationV5) repeated
preservation_verified_v5 { evidence: PreservationEvidenceV5,
                            verification: PreservationVerificationV5 } repeated
partial_rerun_action_recorded_v5(PartialRerunActionV5) repeated
partial_rerun_plan_sealed_v5(PartialRerunPlanV5)
gluing_rerun_action_recorded_v5(GluingRerunActionV5) repeated, post-D2 only
gluing_rerun_plan_sealed_v5(GluingRerunPlanSealV5), post-D2 only
```

The pre-review M6 phases through the partial-plan seal occur in listed order;
the final two kinds occur only at their post-D2 position described below.
Repeated records use ascending derived ID. Each
preservation target has input registration, output registration, then atomic
verification, and targets are ID ordered. No inherited payload may interleave
from source binding through the partial-plan seal. After that seal, only the
per-action §11 sequence may append: new-context actions use inherited
`obligation_transition(Planned)`, then `obligation_transition(InProgress)`, then
`context_envelope_projected`; existing-context actions append none of those
three. Both branches then append the reviewer raw-response
`artifact_registered` V3 event, one atomic `review_execution_recorded` event
containing the execution and all structured claims, and the inherited
`obligation_transition(Completed)`. The registration must be the one minted by
`mint_reviewer_registration_v3_at_v5`, must immediately precede the execution
that references it, and is part of the same action's completion witness and
recovery seam. The Completed transition must immediately follow that atomic
event only when its parsed claim cardinality is exactly one. At zero or greater
than one it is forbidden, the typed obstruction is report-derived from the
durable prefix, and the reviewer action remains incomplete. Only after the
valid transition is durable is the reviewer action complete. If
`target_gluing_required`, the next phase is ascending derived gluing-action
events followed by exactly one gluing-plan seal; no M4, human, gluing-input or
M5 event may precede that second seal. If gluing is not required, both gluing
rerun event kinds are forbidden. After the applicable second-plan condition is
satisfied, only exact scheduled target native M4 bundles (including their
required V3 registrations/evidence/binding/verification events), human events,
zero to two unsuppressed target `artifact_registered_v4` gluing inputs, and at
most one target M5 bundle may
append. Target ProgramSpace/universe never changes. A class seal forbids later
members of that class; every downstream phase requires all upstream seals.

Prepared values and session state follow §4 uncertainty rules. Recovery
recomputes an unsealed deterministic phase and may append only its first missing
ID/body at the current exact tail. For a reviewer pipeline it recognizes every
exact prefix at Planned, InProgress, envelope, raw registration, atomic
execution/claims, or Completed and resumes only with the next §11 event; each
transition's uncertainty uses normal V5 session recovery and never duplicates a
lifecycle or envelope ID. An atomic execution prefix with cardinality zero or
greater than one is a terminal incomplete M6 prefix, not a resumable Completed
seam. The same rule applies independently to
gluing actions and their seal after durable D2 claims. A single-event
uncertainty at either is resolved by `recover_replayed_v5_session`: recovery
observes the exact landed event or appends the same first missing deterministic
event, never retries a consumed prepared value. A late member, changed body,
duplicate ID, caller seal/digest, changed source prefix, or changed target
predecessor refuses. Reports/gates require the partial-plan seal and, when
gluing is required but D2 selection is not ready, they project only the pending
partial-action IDs; no planning-scope ID exists yet. Once every D2 reviewer
action is complete, the two bindings and planning-scope ID are deterministic,
and an absent gluing-plan seal projects `gluing_plan_missing` plus that scope ID
as incomplete. M4,
human and M5 append still refuse while that required second plan is unsealed;
sealed post-plan actions may remain pending. Any missing earlier M6 class seal
still refuses report/gate generation.

All limits are inclusive and independent; every exact/+1 and checked-`u64`
overflow is tested before reserve or encoding:

| Exact collection | Limit |
| --- | ---: |
| source/target ProgramSpace domain IDs | 4,096 each |
| mapping records | 8,192 |
| from/to IDs or ambiguity candidates per mapping | 64 each |
| change facts/predecessor mappings per mapping | 64 each |
| source/target obligations | 2,048 each |
| correspondence entries | 4,096 |
| obligations/predecessors per correspondence entry | 64 each |
| traversal relation visits | 65,536 |
| historical assessments | 8,192 |
| dependency/mapping/correspondence/source IDs per historical record | 512 each |
| preservation evidence / verifications | 2,048 each |
| v5 preservation registrations | 4,096 |
| preservation dependency mappings | 512 |
| gluing freshness records | 1 |
| partial rerun actions | 4,096 |
| gluing rerun actions | 5 |
| gluing claim bindings | 2 |
| V5-position lifecycle transition events | `3*N_new_context_reviewer + N_reused_context_reviewer`, maximum 4,095 |
| subject IDs/prerequisites/stale records/source IDs per action | 512 each (gluing prerequisites max 8) |
| canonical v5 DTO or event line including LF | 1,048,576 bytes |
| preservation input or result CAS object | 1,048,576 bytes |
| inherited CAS object | 67,108,864 bytes |
| each confirmed journal prefix | 67,108,864 bytes |
| index rows | 1,000,000 |
| each complete index snapshot | 67,108,864 bytes |
| index query bytes | 16,777,216 bytes |
| incremental/report working bytes | 536,870,912 bytes |

Seal DTOs contain counts and digests, never unbounded member arrays. The
one-MiB event bound therefore remains real at collection limits; individual
member source sets remain separately bounded. Each scheduled reviewer also
requires a verifier action, and a new-context reviewer additionally requires a
reprojection action, so `3*N_new + 2*N_reused <= 4,096` actions implies the
displayed exact 4,095 transition maximum. Core checked-multiplies/adds the two
counts before reserving journal/index bytes; the exact maximum is admitted and
+1/arithmetic overflow refuses before allocation.

For index v6, `Rows6` is every inherited v5 and M6 row, `Icells6` every non-null
INTEGER, `Tbytes6` every non-null TEXT UTF-8 byte, `SQL6=Tbytes6 + 8*Icells6 +
Rows6`, `Qbytes6` canonical `IndexSnapshotV6`, and `Owned6` recursive decoded
ownership. Recursive ownership charges UTF-8 strings/keys, 8/list slot,
16/object entry, 8/integer or float, 1/Boolean, and 0/null. Exact peak is:

```text
Working6 = SQL6 + Qbytes6 + Owned6
         + max_live_CAS_object_bytes + max_live_event_line_bytes
```

For a dual session, `Js/Jt` are retained source/target journal bytes including
LF, `I5s/I6t` canonical index bytes, `O5s/O6t` decoded ownership, `Cs/Ct` live
CAS buffers, `E4/E5` live source/target event-line buffers, and `Mr` reserved M6
construction ownership:

```text
dual_session_peak = Js + Jt + I5s + I6t + O5s + O6t
                  + Cs + Ct + E4 + E5 + Mr
```

Zero is charged only when that buffer is not live; maxima never substitute for
actual simultaneous ownership. The peak must be ≤536,870,912 bytes.

### 13. Index v6

`SCHEMA_V6` is one literal, never `SCHEMA_V5 + patch`. It copies every v5 table
and constraint byte-for-byte, including separate inherited
`artifact_registrations` and `artifact_registrations_v4`, replaces only event/
metadata contract literals, and adds the tables below.

```sql
CREATE TABLE index_meta (
  singleton INTEGER PRIMARY KEY CHECK(singleton=1),
  index_schema_version INTEGER NOT NULL CHECK(index_schema_version=6),
  projection_contract_version TEXT NOT NULL CHECK(projection_contract_version='reviewgraphen.index_projection.v6'),
  event_contract_version TEXT NOT NULL CHECK(event_contract_version='reviewgraphen.review_event.v5'),
  projection_mode TEXT NOT NULL CHECK(projection_mode='v5_incremental'),
  run_id TEXT NOT NULL, genesis_hash TEXT NOT NULL,
  confirmed_offset INTEGER NOT NULL CHECK(confirmed_offset>=0),
  tail_hash TEXT NOT NULL, event_count INTEGER NOT NULL CHECK(event_count>=0),
  policy_revision_hash TEXT NOT NULL, authority_replay_basis_digest TEXT NOT NULL,
  incremental_source_closure_id TEXT, source_prefix_tail_hash TEXT,
  CHECK((incremental_source_closure_id IS NULL)=(source_prefix_tail_hash IS NULL))
) STRICT;

CREATE TABLE events (
  sequence INTEGER PRIMARY KEY CHECK(sequence>0), event_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.review_event.v5'),
  event_hash TEXT NOT NULL, payload_hash TEXT NOT NULL,
  payload_kind TEXT NOT NULL CHECK(payload_kind IN (
    'obligation_transition','run_genesis_manifest','artifact_registered',
    'snapshot_sources_recorded','review_plan_recorded','context_envelope_projected',
    'review_execution_recorded','evidence_recorded_v3','evidence_bound_v3',
    'verification_recorded_v3','decision_recorded_v3','finding_recorded_v3',
    'artifact_registered_v4','gluing_bundle_recorded_v4',
    'incremental_source_bound_v5','program_mapping_recorded_v5',
    'change_morphism_sealed_v5','obligation_correspondence_entry_recorded_v5',
    'obligation_correspondence_sealed_v5','historical_record_assessed_v5',
    'gluing_freshness_recorded_v5','staleness_assessment_sealed_v5',
    'artifact_registered_v5','preservation_verified_v5',
    'partial_rerun_action_recorded_v5','partial_rerun_plan_sealed_v5',
    'gluing_rerun_action_recorded_v5','gluing_rerun_plan_sealed_v5'
  )),
  actor TEXT NOT NULL, logical_time INTEGER NOT NULL CHECK(logical_time>=0),
  UNIQUE(sequence,event_id)
) STRICT;

CREATE TABLE artifact_registrations_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  registration_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.artifact_registration.v5'),
  run_id TEXT NOT NULL, cas_hash TEXT NOT NULL, media_type TEXT NOT NULL,
  size INTEGER NOT NULL CHECK(size>=0),
  sensitivity TEXT NOT NULL CHECK(sensitivity='canonical_state'),
  source_kind TEXT NOT NULL CHECK(source_kind='preservation_artifact'),
  source_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,registration_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE incremental_source_closures_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  closure_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.incremental_source_closure.v5'),
  repository_id TEXT NOT NULL, repository_identity_hash TEXT NOT NULL,
  source_run_id TEXT NOT NULL, source_genesis_hash TEXT NOT NULL,
  source_confirmed_offset INTEGER NOT NULL CHECK(source_confirmed_offset>=0),
  source_tail_hash TEXT NOT NULL, source_event_count INTEGER NOT NULL CHECK(source_event_count>0),
  source_snapshot_id TEXT NOT NULL, source_universe_id TEXT NOT NULL,
  source_index_snapshot_hash TEXT NOT NULL,
  source_authority_policy_revision_hash TEXT NOT NULL,
  source_authority_replay_basis_digest TEXT NOT NULL,
  source_resolved_target_commit_oid TEXT NOT NULL, source_target_tree_hash TEXT NOT NULL,
  source_gluing_bundle_id TEXT NOT NULL,
  target_run_id TEXT NOT NULL, target_genesis_hash TEXT NOT NULL,
  target_predecessor_offset INTEGER NOT NULL CHECK(target_predecessor_offset>=0),
  target_predecessor_tail_hash TEXT NOT NULL,
  target_predecessor_event_count INTEGER NOT NULL CHECK(target_predecessor_event_count>0),
  target_snapshot_id TEXT NOT NULL, target_universe_id TEXT NOT NULL,
  target_predecessor_index_snapshot_hash TEXT NOT NULL,
  target_authority_policy_revision_hash TEXT NOT NULL,
  target_pre_incremental_authority_replay_basis_digest TEXT NOT NULL,
  target_resolved_base_commit_oid TEXT NOT NULL, target_base_tree_hash TEXT NOT NULL,
  target_resolved_target_commit_oid TEXT NOT NULL, target_target_tree_hash TEXT NOT NULL,
  body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,closure_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE program_mappings_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  mapping_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.program_mapping.v5'),
  source_closure_id TEXT NOT NULL REFERENCES incremental_source_closures_v5(closure_id),
  source_snapshot_id TEXT NOT NULL, target_snapshot_id TEXT NOT NULL,
  object_kind TEXT NOT NULL CHECK(object_kind IN ('repository','snapshot','artifact','relation','context','invariant','limitation')),
  from_ids_canonical_json TEXT NOT NULL, to_ids_canonical_json TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('preserved','modified','added','removed','split','merged','unresolved')),
  candidate_key_kind TEXT NOT NULL CHECK(candidate_key_kind IN ('repository_identity','snapshot_pair','same_path','git_rename_same_content','rust_symbol_anchor_v1','same_kind_label_language_location','mapped_directed_endpoints','mapped_members','mapped_scope','mapped_limitation_sources','no_candidate')),
  source_body_hashes_canonical_json TEXT NOT NULL,
  target_body_hashes_canonical_json TEXT NOT NULL,
  change_fact_ids_canonical_json TEXT NOT NULL,
  predecessor_mapping_ids_canonical_json TEXT NOT NULL,
  successor_ids_canonical_json TEXT NOT NULL, source_ids_canonical_json TEXT NOT NULL,
  body_hash TEXT NOT NULL, PRIMARY KEY(event_sequence,mapping_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE change_morphisms_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  morphism_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.change_morphism.v5'),
  source_closure_id TEXT NOT NULL REFERENCES incremental_source_closures_v5(closure_id),
  repository_id TEXT NOT NULL, source_snapshot_id TEXT NOT NULL, target_snapshot_id TEXT NOT NULL,
  mapping_policy_descriptor_id TEXT NOT NULL CHECK(mapping_policy_descriptor_id='reviewgraphen.program_mapping@1'),
  semantic_anchor_descriptor_id TEXT NOT NULL CHECK(semantic_anchor_descriptor_id='reviewgraphen.rust_symbol_anchor@1'),
  mapping_count INTEGER NOT NULL CHECK(mapping_count>=0), mapping_set_digest TEXT NOT NULL,
  source_domain_count INTEGER NOT NULL CHECK(source_domain_count>=0), source_domain_digest TEXT NOT NULL,
  target_domain_count INTEGER NOT NULL CHECK(target_domain_count>=0), target_domain_digest TEXT NOT NULL,
  status_counts_canonical_json TEXT NOT NULL, source_ids_canonical_json TEXT NOT NULL,
  body_hash TEXT NOT NULL, PRIMARY KEY(event_sequence,morphism_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE obligation_correspondence_entries_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  entry_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.obligation_correspondence_entry.v5'),
  morphism_id TEXT NOT NULL REFERENCES change_morphisms_v5(morphism_id),
  from_obligation_ids_canonical_json TEXT NOT NULL, to_obligation_ids_canonical_json TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('preserved','modified','added','removed','split','merged','unresolved')),
  source_mapping_ids_canonical_json TEXT NOT NULL,
  predecessor_entry_ids_canonical_json TEXT NOT NULL,
  successor_obligation_ids_canonical_json TEXT NOT NULL,
  source_body_hashes_canonical_json TEXT NOT NULL,
  target_body_hashes_canonical_json TEXT NOT NULL,
  source_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,entry_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE obligation_correspondences_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  correspondence_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.obligation_correspondence.v5'),
  morphism_id TEXT NOT NULL REFERENCES change_morphisms_v5(morphism_id),
  source_universe_id TEXT NOT NULL, target_universe_id TEXT NOT NULL,
  policy_descriptor_id TEXT NOT NULL CHECK(policy_descriptor_id='reviewgraphen.obligation_correspondence@1'),
  entry_count INTEGER NOT NULL CHECK(entry_count>=0), entry_set_digest TEXT NOT NULL,
  source_domain_count INTEGER NOT NULL CHECK(source_domain_count>=0), source_domain_digest TEXT NOT NULL,
  target_domain_count INTEGER NOT NULL CHECK(target_domain_count>=0), target_domain_digest TEXT NOT NULL,
  status_counts_canonical_json TEXT NOT NULL, source_ids_canonical_json TEXT NOT NULL,
  body_hash TEXT NOT NULL, PRIMARY KEY(event_sequence,correspondence_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE historical_record_assessments_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  record_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.historical_record_assessment.v5'),
  assessment_id TEXT NOT NULL,
  source_record_kind TEXT NOT NULL CHECK(source_record_kind IN ('obligation','review_plan','context_envelope','artifact_registration_v3','artifact_registration_v4','execution','claim','claim_assessment','evidence','evidence_binding','verification','decision','finding','gluing_input_descriptor','context_cover','section','restriction','gluing_attempt','global_candidate','gluing_obstruction','coverage')),
  source_record_id TEXT NOT NULL, source_record_body_hash TEXT NOT NULL,
  successor_record_ids_canonical_json TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('structurally_preserved','stale','superseded')),
  directness TEXT NOT NULL CHECK(directness IN ('not_applicable','direct','indirect','direct_and_indirect')),
  reasons_canonical_json TEXT NOT NULL, dependency_source_ids_canonical_json TEXT NOT NULL,
  mapping_ids_canonical_json TEXT NOT NULL,
  correspondence_entry_ids_canonical_json TEXT NOT NULL,
  source_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,record_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE gluing_freshness_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  freshness_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.gluing_freshness.v5'),
  assessment_id TEXT NOT NULL, source_attempt_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('structurally_preserved','stale','superseded')),
  reasons_canonical_json TEXT NOT NULL,
  dependency_mapping_ids_canonical_json TEXT NOT NULL,
  successor_target_attempt_ids_canonical_json TEXT NOT NULL,
  source_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,freshness_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE staleness_assessments_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  assessment_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.staleness_assessment.v5'),
  source_closure_id TEXT NOT NULL REFERENCES incremental_source_closures_v5(closure_id),
  morphism_id TEXT NOT NULL REFERENCES change_morphisms_v5(morphism_id),
  correspondence_id TEXT NOT NULL REFERENCES obligation_correspondences_v5(correspondence_id),
  impact_policy_descriptor_id TEXT NOT NULL CHECK(impact_policy_descriptor_id='reviewgraphen.mvp_property_impact@1'),
  assessment_time TEXT NOT NULL,
  record_count INTEGER NOT NULL CHECK(record_count>=0), record_set_digest TEXT NOT NULL,
  gluing_freshness_count INTEGER NOT NULL CHECK(gluing_freshness_count>=0),
  gluing_freshness_set_digest TEXT NOT NULL,
  stale_source_count INTEGER NOT NULL CHECK(stale_source_count>=0), stale_source_digest TEXT NOT NULL,
  superseded_source_count INTEGER NOT NULL CHECK(superseded_source_count>=0), superseded_source_digest TEXT NOT NULL,
  preservation_candidate_count INTEGER NOT NULL CHECK(preservation_candidate_count>=0),
  preservation_candidate_digest TEXT NOT NULL,
  m5_dependent_successor_count INTEGER NOT NULL CHECK(m5_dependent_successor_count>=0),
  m5_dependent_successor_digest TEXT NOT NULL,
  source_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,assessment_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE preservation_evidence_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  evidence_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.preservation_evidence.v5'),
  target_snapshot_id TEXT NOT NULL, target_obligation_id TEXT NOT NULL,
  source_closure_id TEXT NOT NULL REFERENCES incremental_source_closures_v5(closure_id),
  morphism_id TEXT NOT NULL REFERENCES change_morphisms_v5(morphism_id),
  correspondence_entry_id TEXT NOT NULL REFERENCES obligation_correspondence_entries_v5(entry_id),
  source_claim_id TEXT NOT NULL, source_evidence_ids_canonical_json TEXT NOT NULL,
  source_verification_id TEXT NOT NULL,
  dependency_mapping_ids_canonical_json TEXT NOT NULL,
  input_registration_id TEXT NOT NULL REFERENCES artifact_registrations_v5(registration_id),
  output_registration_id TEXT NOT NULL REFERENCES artifact_registrations_v5(registration_id),
  descriptor_id TEXT NOT NULL CHECK(descriptor_id='reviewgraphen.structural_preservation@1'),
  procedure_version TEXT NOT NULL CHECK(procedure_version='reviewgraphen.structural_preservation.payment_v1'),
  observation TEXT NOT NULL CHECK(observation='structure_preserved'),
  source_ids_canonical_json TEXT NOT NULL,
  body_hash TEXT NOT NULL, PRIMARY KEY(event_sequence,evidence_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE preservation_verifications_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  verification_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.preservation_verification.v5'),
  target_snapshot_id TEXT NOT NULL, target_obligation_id TEXT NOT NULL,
  evidence_id TEXT NOT NULL REFERENCES preservation_evidence_v5(evidence_id),
  source_verification_id TEXT NOT NULL,
  descriptor_id TEXT NOT NULL CHECK(descriptor_id='reviewgraphen.structural_preservation@1'),
  procedure_version TEXT NOT NULL CHECK(procedure_version='reviewgraphen.structural_preservation.payment_v1'),
  outcome TEXT NOT NULL CHECK(outcome='passed'),
  source_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,verification_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE partial_rerun_actions_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  action_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.partial_rerun_action.v5'),
  staleness_assessment_id TEXT NOT NULL REFERENCES staleness_assessments_v5(assessment_id),
  subject_kind TEXT NOT NULL CHECK(subject_kind='obligation'),
  subject_ids_canonical_json TEXT NOT NULL,
  action TEXT NOT NULL CHECK(action IN ('reproject_context','rerun_reviewer','rerun_verifier','rerun_human_decision')),
  prerequisites_canonical_json TEXT NOT NULL,
  stale_source_record_ids_canonical_json TEXT NOT NULL,
  reasons_canonical_json TEXT NOT NULL,
  source_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,action_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE partial_rerun_plans_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  plan_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.partial_rerun_plan.v5'),
  source_closure_id TEXT NOT NULL REFERENCES incremental_source_closures_v5(closure_id),
  morphism_id TEXT NOT NULL REFERENCES change_morphisms_v5(morphism_id),
  correspondence_id TEXT NOT NULL REFERENCES obligation_correspondences_v5(correspondence_id),
  staleness_assessment_id TEXT NOT NULL REFERENCES staleness_assessments_v5(assessment_id),
  planner_descriptor_id TEXT NOT NULL CHECK(planner_descriptor_id='reviewgraphen.partial_rerun@1'),
  target_plan_id TEXT NOT NULL,
  selected_target_count INTEGER NOT NULL CHECK(selected_target_count>=0), selected_target_digest TEXT NOT NULL,
  action_count INTEGER NOT NULL CHECK(action_count>=0), action_set_digest TEXT NOT NULL,
  preservation_verification_count INTEGER NOT NULL CHECK(preservation_verification_count>=0),
  preservation_verification_digest TEXT NOT NULL,
  required_human_resolution_count INTEGER NOT NULL CHECK(required_human_resolution_count>=0),
  required_human_resolution_digest TEXT NOT NULL,
  source_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,plan_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE gluing_rerun_actions_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  action_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.gluing_rerun_action.v5'),
  planning_scope_id TEXT NOT NULL,
  subject_kind TEXT NOT NULL CHECK(subject_kind IN ('gluing_context','gluing_attempt')),
  subject_ids_canonical_json TEXT NOT NULL,
  action TEXT NOT NULL CHECK(action IN ('register_gluing_input','rebuild_section','reglue')),
  prerequisites_canonical_json TEXT NOT NULL,
  reasons_canonical_json TEXT NOT NULL,
  source_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,action_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE gluing_rerun_plans_v5 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0), event_id TEXT NOT NULL,
  plan_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.gluing_rerun_plan.v5'),
  planning_scope_id TEXT NOT NULL,
  source_closure_id TEXT NOT NULL REFERENCES incremental_source_closures_v5(closure_id),
  partial_rerun_plan_id TEXT NOT NULL REFERENCES partial_rerun_plans_v5(plan_id),
  target_plan_id TEXT NOT NULL,
  selection_descriptor_id TEXT NOT NULL CHECK(selection_descriptor_id='reviewgraphen.m5_claim_selection@1'),
  claim_bindings_canonical_json TEXT NOT NULL,
  action_count INTEGER NOT NULL CHECK(action_count>=0 AND action_count<=5),
  action_set_digest TEXT NOT NULL,
  existing_target_bundle_witness_canonical_json TEXT,
  source_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,plan_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;
```

Core bounded-decodes and canonical-reserializes every JSON cell before insert
and query; verifies all IDs, exact source sets, successor sets, status-count
zero keys, digests, component exclusivity, event tuples, actors, ordering,
authority and phase seals. It also validates each prerequisite JSON element as
exactly one closed `scheduled_action` or `existing_target_record` shape, checks
referenced action/record IDs, exact body hashes and event positions, and enforces
the per-action alternatives in §11/§11.1. For inherited
`obligation_transition` rows it additionally checks the prepared action
binding, exact prior/next state, V5 event position, §11 adjacency and structured
execution prerequisite for Completed. SQL constraints are backstops.
`IndexSnapshotV6` returns inherited arrays plus complete durable M6 rows,
including a valid prefix of gluing actions and the optional gluing-plan seal, in
event order. It refuses an incomplete pre-review M6 phase or a non-prefix,
mismatched gluing-action phase; an exact post-D2 gluing-action prefix is exposed
so the gate can report `gluing_plan_missing`.

Lifecycle transitions add no new DTO, SQL table or report-result array: the V5
`events` CHECK already admits inherited `obligation_transition`, the inherited
obligation index row projects the replayed current lifecycle, and report status,
visited/completed coverage and rerun `completion_witness_ids` are recomputed
from those exact event positions. A report/index that ignores, invents or
reorders any applicable §11 transition event refuses.
The D2 execution/claim tables and schemas retain their frozen 1..16 general
bound. M6 index/report construction adds the profile check over each
obligation-bound execution: exactly one permits the Completed/downstream
closure, while zero/multiple preserves the durable general-D2 rows but exposes
the typed M6 obstruction and no downstream mint. No DDL widening, caller claim
column or selection table is introduced.

### 14. Closed report v5 and gate

`schemas/reviewgraphen.report.v5.schema.json` is Draft 2020-12 and has
`additionalProperties:false` at every object. Its exact top-level required keys
are `schema,report_type,report_version,metadata,scenario,result,coverage,
projection,gate`; constants are `reviewgraphen.review.report.v5`, `review`, `5`.

Required object keys are closed as follows:

- metadata: `report_id,run_id,profile_id,rule_set_hash,extractor_set_hash,
  policy_version,event_contract_version,index_projection_version,genesis_hash,
  confirmed_offset,confirmed_tail_hash,confirmed_event_count,tool_versions,
  authority_policy_revision_hash,authority_replay_basis_digest,
  gluing_profile_descriptor_id,incremental_policy_descriptor_id,
  source_run_id,source_genesis_hash,source_confirmed_offset,source_tail_hash,
  source_event_count,source_index_snapshot_hash,
  source_authority_policy_revision_hash,source_authority_replay_basis_digest,
  target_predecessor_offset,target_predecessor_tail_hash,
  target_predecessor_event_count,target_predecessor_index_snapshot_hash,
  target_pre_incremental_authority_replay_basis_digest,
  target_index_snapshot_hash`;
- scenario: `repository_id,snapshot_id,program_space_ref,universe_id,plan_id,
  selected_obligation_ids,artifact_registration_ids,
  preservation_registration_ids,context_cover_ids,
  incremental_source_closure,change_morphism,obligation_correspondence`;
- result: `status,artifact_registrations,artifact_registrations_v5,
  executions,claims,evidence,evidence_bindings,
  verifications,decisions,findings,claim_assessments,obstructions,
  gluing_input_descriptors,context_covers,sections,gluing_attempts,restrictions,
  global_candidates,gluing_obstructions,program_mappings,
  obligation_correspondence_entries,historical_record_assessments,
  gluing_freshness,staleness_assessment,preservation_evidence,
  preservation_verifications,partial_rerun_actions,partial_rerun_plan,
  gluing_rerun_actions,gluing_rerun_plan`;
- coverage: `universe_id,denominator_obligation_ids,visited_obligation_ids,
  completed_obligation_ids,evidence_supported_obligation_ids,
  m5_dependent_successor_obligation_ids,
  required_human_resolution_obligation_ids,
  structurally_preserved_obligation_ids,native_verified_obligation_ids,
  verified_obligation_ids,fresh_verified_obligation_ids,
  accepted_obligation_ids,selected,visited,completed,evidence_supported,
  m5_dependent_successors,required_human_resolutions,structurally_preserved,native_verified,verified,
  fresh_verified,accepted`;
- projection: exactly `views` with the inherited three closed v4 views; and
- gate: `schema,id,policy_descriptor_id,status,required_fresh_obligation_ids,
  blocking_ids,incomplete_ids,reasons,source_ids,body_hash`.

Every event-backed item is its complete strict DTO plus event tuple and computed
body hash. Inherited/report-derived obstruction items retain their frozen
eventless four-field shape; the M6 cardinality specialization below closes
their values and source trace.
`artifact_registrations_v5` is separate and contains its complete decoded closed
source. Scenario seal DTOs retain count/digest fields; result member arrays are
complete and independently digest-checked.

Each `partial_rerun_actions` and `gluing_rerun_actions` report item additionally requires derived
`state: pending|complete` and sorted `completion_witness_ids`. Pending requires
an empty witness set; complete requires exactly the §11 or §11.1 witness
event/record IDs and all scheduled prerequisite actions complete; existing-record
prerequisites are revalidated but have no action state. These two report-only
fields are excluded from the event DTO body hash but included in the complete
report item and report serialization checks. `gluing_rerun_plan` is the complete
seal item when present and exactly `null` when gluing is not required or its
post-D2 plan is not yet sealed. In the latter case all durable gluing actions
are reported `pending`. Before D2 selection readiness, pending partial actions
are the only corresponding incomplete IDs. After readiness, the gate
additionally distinguishes the missing seal with `gluing_plan_missing` and the
now-derived planning scope.

Each partial `rerun_verifier` item additionally requires
`resolved_reviewer_output_records`. It is exactly the two sorted
`ExistingTargetRecordV5` values for the execution and sole claim after an
exactly-one fresh/reused reviewer reduction, and empty while that prerequisite
is pending or cardinality-obstructed. Every other partial action item requires
this array empty. These values are report-derived, excluded from the action DTO
body hash, and must equal the verifier mint inputs byte-for-byte.

For each zero/multiple-claim M6 execution, `result.obstructions` contains exactly
one frozen obstruction-shaped item:

```text
kind = "m6_claim_cardinality_unsupported"
message = "M6 requires exactly one parsed claim; observed <canonical u64>"
blocks = {subject obligation ID}
source_ids = {source closure ID, partial plan ID, reviewer action ID iff emitted,
              envelope ID, raw registration ID, execution ID, execution event ID,
              every observed claim ID, Completed event ID iff reused}
```

The two source closures are exact, before canonical sorting:

```text
fresh = {closure, partial plan, emitted reviewer action, envelope,
         raw registration, execution, execution event} ∪ all observed claims
reused = {closure, partial plan, envelope, raw registration, execution,
          execution event, Completed event} ∪ all observed claims
```

Both contain exactly `7 + observed` distinct IDs. Each set is sorted by canonical
identifier bytes and deduplicated before serialization; any duplicate category,
missing/extra ID, fresh Completed event, reused reviewer-action ID, or swapped
execution/Completed event refuses. The execution event must be the atomic event
owning that execution/claim set. The optional Completed event must be the later
transition for that same obligation and is present only on reused lifecycle-
Completed input.

Frozen D2 makes a reused zero-claim Completed execution impossible and replay-
invalid before M6. A recovered durable zero-claim M6 attempt is still the
`fresh` form because its reviewer action exists and no Completed event does;
legitimate `reused` cardinality obstruction therefore covers general-D2
Completed executions with 2..16 claims. This classification is mechanical, not
caller-selected.

It is a deterministic report projection, not a new event/accepted fact. At most
one is emitted per obligation/action; it counts inside the inherited 8,192
obstruction/report-row bound. Reordering claims, choosing one, omitting an
observed claim ID, or changing `observed`, message, blocks or source IDs refuses
the report.
The `result.obstructions` array retains the frozen inherited obstruction order,
then appends M6 cardinality rows sorted by `(blocks[0], execution_id)` where
`execution_id` is uniquely decoded from `source_ids`; no caller ordering is
accepted.
Frozen D2 `result.status` remains lifecycle-derived: a fresh obstructed attempt
without Completed is `partial`, while a reused previously-Completed multi-claim
execution can retain D2 `completed`; in both cases the M6 gate is incomplete.
Because the obstruction uses the inherited `result.obstructions` shape, its
view representation/loss pointer remains the frozen inherited obstruction
contract; M6 adds no duplicate omission-loss location.

For one atomic M5 bundle event, completion is reduced in fixed logical order:
first its context-ordered Section records complete scheduled `rebuild_section`
actions, then its attempt/bundle record completes `reglue`. Thus the latter's
scheduled rebuild prerequisites are complete within the same event without
pretending that a Section was independently durable or earlier in the journal.

Each view computes an exact omission loss for each M6 array or singleton below.
`R_a` is the complete record IDs and
`V_a` those completely represented in the view;
nonempty `R_a\V_a` creates exactly one meaningful/recoverable loss with that
set as `source_ids`, affected properties from those records, and this closed
pointer:

| Array | Exact `recovery_ref` |
| --- | --- |
| artifact registrations v5 | `reviewgraphen.review.report.v5#/result/artifact_registrations_v5` |
| program mappings | `reviewgraphen.review.report.v5#/result/program_mappings` |
| correspondence entries | `reviewgraphen.review.report.v5#/result/obligation_correspondence_entries` |
| historical assessments | `reviewgraphen.review.report.v5#/result/historical_record_assessments` |
| gluing freshness | `reviewgraphen.review.report.v5#/result/gluing_freshness` |
| preservation evidence | `reviewgraphen.review.report.v5#/result/preservation_evidence` |
| preservation verifications | `reviewgraphen.review.report.v5#/result/preservation_verifications` |
| partial rerun actions | `reviewgraphen.review.report.v5#/result/partial_rerun_actions` |
| gluing rerun actions | `reviewgraphen.review.report.v5#/result/gluing_rerun_actions` |
| source closure | `reviewgraphen.review.report.v5#/scenario/incremental_source_closure` |
| change morphism | `reviewgraphen.review.report.v5#/scenario/change_morphism` |
| obligation correspondence | `reviewgraphen.review.report.v5#/scenario/obligation_correspondence` |
| staleness assessment | `reviewgraphen.review.report.v5#/result/staleness_assessment` |
| partial rerun plan | `reviewgraphen.review.report.v5#/result/partial_rerun_plan` |
| gluing rerun plan | `reviewgraphen.review.report.v5#/result/gluing_rerun_plan` |
| gate | `reviewgraphen.review.report.v5#/gate` |

Affected properties are not caller labels. Let `P(O)` be the sorted property IDs
of obligation set `O`, `O(r)` the exact source/target obligation IDs in record
`r`'s §8 direct/transitive closure, and `nonempty(P)` return `P` unless empty,
otherwise the singleton `reviewgraphen.capability_gap`. Each loss location uses:

| Record location | Exact `affected_properties` |
| --- | --- |
| artifact registrations v5 | `P({registration.source.target_obligation_id})` |
| program mappings | `nonempty(P(obligations whose §7 direct/indirect Program closure intersects mapping.from_ids∪to_ids))` |
| correspondence entries | `nonempty(P(entry.from_obligation_ids∪to_obligation_ids))` |
| historical assessments | `nonempty(P(O(source_record)∪successor_record obligations))`; every M5 record additionally contributes `payment.at_most_once` |
| gluing freshness | `{payment.at_most_once}` |
| preservation evidence/verifications | `P({target_obligation_id})` |
| partial rerun actions | `P(subject obligation IDs)` |
| gluing rerun actions | `{payment.at_most_once}` |
| source closure | `nonempty(P(target denominator))` |
| change morphism | sorted union of its mapping-row formulas |
| obligation correspondence | sorted union of its entry-row formulas |
| staleness assessment | sorted union of historical and gluing-freshness formulas |
| partial rerun plan | sorted union of partial-action and preservation-verification formulas |
| gluing rerun plan | `{payment.at_most_once}` |
| gate | `nonempty(P(required-fresh IDs ∪ obligation IDs resolved from blocking/incomplete IDs))`; M5 gate IDs contribute `payment.at_most_once` |

The reason is exactly `view omits complete records from <record location>` and kind is
`omitted_m6_incremental_records`; no nonexistent path or combined hidden array
is allowed. At most sixteen M6 omission losses exist per view. Inherited v4
loss pointers remain frozen.

The gate policy is `reviewgraphen.incremental_gate@1`:

```rust
IncrementalGateV5 {
  schema: "reviewgraphen.incremental_gate.v5",
  id, policy_descriptor_id: "reviewgraphen.incremental_gate@1",
  status, required_fresh_obligation_ids, blocking_ids,
  incomplete_ids, reasons, source_ids, body_hash
}
```

It is report-only and never appears in the event journal or index tables. Its ID
preimage is every displayed field except schema/id/body_hash; the body hash
covers the complete object except itself. The gate loss row uses this gate ID.

```text
blocked, with priority, when:
  a current target accepted issue finding exists; or
  the sole current target M5 bundle has assignment_conflict

incomplete when not blocked and:
  a selected critical target obligation is not fresh_verified; or
  a required-human-resolution obligation lacks a current target `rejected` finding; or
  an M6 reviewer execution has parsed claim cardinality other than one; or
  any partial or gluing rerun action is pending; or
  selected closure has unresolved/unsupported impact; or
  target gluing is required, D2 selection is ready, but its second plan is unsealed; or
  a required target M5 bundle/fresh descriptor pair is absent; or
  the target M5 result is unknown or candidate; or
  target extraction is partial/missing/unknown

pass otherwise
```

Source decisions/findings/conflicts never directly block; they only create
native target actions. Gate status is `blocked|incomplete|pass`. Reasons are the
closed enum `current_accepted_issue|target_gluing_assignment_conflict|
fresh_verification_missing|rerun_pending|mapping_unresolved|
unsupported_impact_policy|target_gluing_missing|target_gluing_incomplete|
gluing_plan_missing|human_resolution_missing|m6_claim_cardinality_unsupported|
extraction_incomplete`.
Blocked requires nonempty blocking IDs; incomplete requires nonempty incomplete
IDs; pass requires empty blocking/incomplete/reason sets. Required-fresh IDs are
exact critical selected obligations from fixed rule metadata, never centrality
or confidence. Gate is a deterministic report projection, not authority.

`blocking_ids` is exactly the current target finding or target M5 obstruction
IDs satisfying the blocked predicates. `incomplete_ids` is exactly the union of
missing required-fresh obligation IDs, pending action IDs, unresolved
mapping/correspondence IDs, a selection-ready required missing gluing
planning-scope ID, missing
target gluing subject IDs, every cardinality-obstructed execution and subject
obligation ID, and extraction
limitation IDs satisfying the incomplete predicates; an existing
unknown/candidate gluing result contributes its attempt and obstruction (when
present) IDs, and unresolved human review contributes its target obligation ID.
A structurally preserved `issue_present` obligation is not `fresh_verified` until
native M4 passes and remains gate-incomplete until human resolution; a current
target accepted finding blocks, and only a current target rejected finding satisfies resolution.
Gate `source_ids` is the following closed union, sorted and deduplicated:

```text
W_base = {closure, morphism, correspondence, staleness, partial-plan IDs}
       ∪ {gluing-plan ID iff sealed}
W_native = for each current native Passed verification used by fresh_verified:
           {context-envelope, execution, selected claim, evidence,
            evidence binding, verification IDs}
           ∪ every exact ArtifactRegistrationV3 ID in that verification closure:
             {reviewer raw, verifier input, verifier output,
              external-harness witness iff the fixture variant uses it}
W_finding = for each current accepted/rejected finding used by the gate:
            {decision, finding IDs} ∪ that finding's W_native claim closure
W_m5 = iff target gluing is required, every durable current target ID among:
       {both descriptors, both registrations, context cover, Sections,
        Restrictions, attempt, candidate, obstruction}
W_extraction = {target snapshot, ProgramSpace, universe, plan IDs}
             ∪ exact extraction-limitation IDs
W_cardinality = union of every m6_claim_cardinality_unsupported obstruction's
                exact source_ids
E_cardinality = from each W_cardinality member set, exactly its classified
                execution event ID plus its Completed event ID iff reused
gate.source_ids = W_base ∪ blocking_ids ∪ incomplete_ids
                ∪ W_native ∪ W_finding ∪ W_m5 ∪ W_extraction
                ∪ W_cardinality
```

“Current” is the unique record selected by the frozen M4/M5 state reductions;
missing optional records contribute no ID. `E_cardinality` is the sole event-ID
exception in `gate.source_ids`: those explicitly classified execution/Completed
event IDs are included through `W_cardinality`. No other event ID, source-
history record, preservation record, report row, or additional transitive record
may be added. The complete union, including these exceptions, is sorted once by
canonical identifier bytes and deduplicated once; component concatenation order
never affects the gate ID/body.
Its body hash covers every gate field except itself.

Report limits are 262,144 rows, 8,192 mappings, 4,096 correspondence entries,
8,192 historical records, 4,096 v5 registrations, 2,048 each preservation
evidence/verifications, 4,096 partial rerun actions, 5 gluing rerun actions,
2,048 M6 cardinality obstructions within the inherited total obstruction cap
of 8,192, 1 gluing rerun plan, 1 gluing freshness row, 4,096 losses,
3 views, 134,217,728 output bytes, and 536,870,912 working bytes. Exact rows:

Cardinality source accounting checked-adds `7 + observed` per obstruction before
allocation. With observed bounded by frozen D2 at 16, the pre-dedup
`W_cardinality` ceiling is 47,104 IDs and `E_cardinality` is at most 4,096 event
IDs. Canonical dedup may lower realized ownership but never lowers reservation;
both sets are charged to report/gate working and output bytes.

```text
report_rows_v5 = complete_inherited_target_v4_shape_rows
               + artifact_registrations_v5 + program_mappings
               + correspondence_entries + historical_assessments
               + gluing_freshness + preservation_evidence
               + preservation_verifications + partial_rerun_actions
               + m6_claim_cardinality_obstructions
               + gluing_rerun_actions + gluing_rerun_plan_present
               + 6 singleton closure/seal/assessment/partial-plan/gate records
```

`gluing_rerun_plan_present` is one exactly when the gluing-plan seal is durable
and zero when gluing is not required or the required seal is still missing; the
six fixed singleton rows are source closure, change
morphism, obligation correspondence, staleness assessment, partial rerun plan,
and gate.
Here `complete_inherited_target_v4_shape_rows` excludes the new report-derived
M6 cardinality obstruction rows even though they reuse the inherited
obstruction object shape.

Let `Rr5/R5` be reserved/realized report ownership, `S5` largest complete
record and `Out5` output bytes. Report generation retains the same dual-session
buffers as §12, so it enforces:

```text
projection_peak_v5 = Js + Jt + I5s + I6t + O5s + O6t
                   + Cs + Ct + E4 + E5 + Mr + Rr5
serialization_peak_v5 = Js + Jt + I5s + I6t + O5s + O6t
                      + Cs + Ct + E4 + E5 + Mr + R5 + S5 + Out5
```

The generator holds both locks in canonical order, validates both prefixes,
indexes, CAS, authority bases, all seals, current post-plan action witnesses and
byte equality, then recomputes coverage/loss/gate. “All seals” here means every
pre-review M6 seal plus the gluing seal when present. There are exactly two
permitted unsealed post-plan projections: before D2 selection readiness,
pending partial-action IDs are reported without a planning scope; after
readiness, an exact deterministic gluing-action prefix is reported with the
derived scope and `gluing_plan_missing`. Any mismatch emits no report.

### 15. Required tests

Implementation requires:

1. Independently constructed equivalent S0/S1 pairs produce identical closure,
   mappings, correspondence, assessment, preservation, plan, index and report
   bytes despite shuffled inputs.
2. Source must have the complete two-registration/one-bundle M5 baseline;
   missing/partial/duplicate M5 and every cross-root/repository/run/snapshot/
   dirty/version mismatch refuse.
3. Resolved source-target OID equals target-base OID and both tree hashes equal;
   branch names, abbreviations, equal tree-only, OID-only, base/target swaps and
   Program/CAS mutations refuse.
4. Every object-kind key, directed relation endpoint order, zero/one/many
   component, split/merge/many-many ambiguity, no-candidate addition/removal,
   status-count zero key, digest, predecessor/successor/source set and domain
   coverage mutation refuses.
5. Unique same-content Git rename and unique symbol anchor preserve; copy,
   changed anchor, duplicate candidate, label/path/prose/confidence guesses do not.
6. Every property-table row, traversal direction/depth, direct/indirect/both,
   mapping status, every §8 source-record dependency/successor/M/C formula,
   record-class reason and action-DAG combination is exercised.
   A changed callee invalidates unchanged caller relation/path/invariant closure.
7. Old VerificationV3 and structural preservation never give verified/gate credit. Positive preservation and
   every CAS/registration/harness/test/policy/model/dependency mutation prove the
   separate V5 registration and target verification boundary. Preserved
   issue-present proves correspondence only and still requires a complete
   current-target reviewer/verifier/human closure, each stage exactly reused or
   rerun, an exact new-or-revalidated context, and rejected finding.
8. Active source human decision/finding requires current-target context,
   reviewer, verifier and human/finding closures; source state suppresses none,
   while exact target S1 predecessor closures may independently suppress their
   matching actions. Preservation-only is refused and old issue/nonissue never
   becomes accepted or a direct gate result. Every execution/claim/raw/event/
   Completed field mutation defeats reviewer reuse; a mismatch on an already-
   Completed obligation refuses rather than reopening lifecycle.
9. Source M5 success/conflict/unknown is history only. Target requires fresh V4
   registrations/descriptors/native Sections and exactly one total M5 bundle;
   old conflict does not block and a second target bundle refuses.
10. Every action subject kind/ID/prerequisite closed-enum variant, suppression
    predicate, mandatory existing-record replacement, exact body/event witness,
    post-seal completion witness, pending gate state, and
    blocked/incomplete/pass precedence. Scheduled and suppressed predecessor
    combinations cover all four partial actions and every reachable gluing
    variant. Mandatory-native tests prove independent exact target reuse for
    context, Completed reviewer closure, native verifier and V5 human/finding;
    source-only equality never suppresses, and changed context/claim/raw/body/
    event/lifecycle cases rerun when legal or refuse Completed mismatch without
    a duplicate envelope or lifecycle reopen.
11. Fresh/pre-incremental replay/closure append/normal replay/recover, canonical
    dual-lock order, noncyclic pre-basis→closure→normal-basis digests, every
    listed prepared mint/append including every
    `prepare/append_obligation_transition_at_v5`, successful tail/sequence/basis
    transition, replay-entry
    change, pre-durable failure, uncertainty, one-shot V5 resume, source
    append-after-pin and deadlock resistance.
12. V3/V4/V5 registration kind/table/actor/identity/source confusion and nested/
    outer CAS tuple mutation refuse in append/replay/index/report.
13. Crash at every member/seal/CAS/registration/preservation/first-plan/D2
    Planned/InProgress/context/reviewer-registration/atomic-execution-with-claims/
    Completed/
    gluing-action/gluing-plan/M4/M5 seam; exact first-missing recovery, no
    duplicate/late member and no source mutation. Missing pre-review M6 seals
    refuse report/gate; a valid unsealed gluing-plan prefix instead yields the
    exact incomplete gate and cannot admit M4/human/M5. Before D2 readiness no
    planning scope is emitted; after readiness the same missing seal emits the
    derived scope plus `gluing_plan_missing`.
    The first plan contains no concrete claim/Section binding; post-D2 selection
    covers zero/one/many actual candidates, existing-target bundle exact equality
    and mismatch, gluing-not-required null plan, and M4/M5 rejection before the
    required second seal.
14. `SCHEMA_V6` creates with all inherited/new tables; displayed SQL parses;
    rebuild twice equally; every schema/ID/hash/source/status/order/FK mutation
    and exact/+1/overflow row/event/journal/index/CAS/output/working limit
    refuses, including the exact lifecycle transition count formula and 4,095
    bound.
15. Closed report keys, report-only gate ID/body, every exact recovery pointer,
    omitted set and record-location affected-property formula, numerator
    separation and source-bound double-submit fixture are generated from actual
    journals/CAS/indexes/trust roots, never detached report or SQLite assertions.
16. General non-M6 D2 still admits 1..16 claims. M6 scheduled-fresh and reused
    reviewer fixtures with exactly one claim pass and bind its exact ID/body/
    event to verifier and human inputs. Zero and two claims both produce
    `M6ClaimCardinalityUnsupported` with exact observed count, obstruction,
    source trace and incomplete gate; no Completed transition for fresh output
    and no verifier/human prepared operation for either path. Reordering the two
    claims, choosing either claim, changing polarity/confidence, or mutating the
    obstruction/resolved prerequisite records never creates a choice and is
    rejected byte-for-byte. Fresh/reused tests assert the exact `7+observed`
    source set, canonical order/dedup and gate union. Fresh zero/two use the
    emitted-action/no-Completed form; reused two uses no-action/Completed, while
    reused zero-Completed is frozen-D2 replay-invalid. Only the classified
    execution event and reused-only Completed event enter `E_cardinality`, while
    adding either to the wrong variant or adding any other event ID refuses.

## Consequences

M6 makes correspondence, impact scope and native rerun selection deterministic.
Its structural preservation record is audit evidence only and by itself does
not skip reviewer/verifier work or carry human/gluing authority; only an exact
current-target S1 closure can satisfy a stage. Unknown correspondence
remains visible and conservative, historical state remains immutable, and the
gate is derived from current source-bound state.

The cost is a two-journal authority session, semantic anchors, component-based
mapping, sealed digest phases, a separate V5 registration, fresh target gluing,
and larger exact resource accounting.

## Decision acceptance

This ADR is Accepted because ADR 0022 is Accepted unchanged at §1 and an
independent review accepted its registration, authority, mapping, impact,
human, gluing, event-size, DDL, report, gate and resource contracts. There is no open
implementation choice; changing an enumerated version, status, key, direction,
reason, action, identity, digest, limit or gate rule requires ADR revision.
