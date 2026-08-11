# ADR 0022: M5 Context Cover and Source-Bound Gluing

- Status: Accepted
- Date: 2026-08-10
- Amended: 2026-08-11 — closed the Core/Store bootstrap and recovery boundary, orphan adoption,
  v5 reciprocal-FK/index-item/accounting details, and the legal pre-bundle prefix.
- Amended: 2026-08-11 — strengthened strict-interior M4 recovery so neither an editable log nor
  a replay basis escapes before the sealed suffix is durable and fully replay-confirmed. This
  prevents a resume-only holder from leaking basis-shaped state that callers could mistake for
  ordinary append authority.
- Amended: 2026-08-11 — added the opaque read-only runtime profile-basis projection used to
  derive the fixed descriptor CAS objects and augment existing V4 host roots without rebuilding
  fixed IDs, qualification IDs, or private inherited-root tuples outside Core.
- Scope: Defines the M5 vertical slice over the Accepted M4 contract: a fresh homogeneous event-v4 run, context cover, profile-owned local Sections, deterministic pairwise restrictions and gluing, source-bound gluing obstructions, index v5, and report v4. It does not implement a verifier, decision, finding, policy gate, provider adapter, generic command runner, generic context taxonomy, change morphism, or staleness.

## Context

M5 does not make review comments agree. It preserves an auditable answer to whether local review results can be used together. The conceptual rule is:

```text
section_i restricted to (context_i ∩ context_j)
  is compatible with
section_j restricted to (context_i ∩ context_j)
```

A disagreement is a ReviewSpace obstruction, never a ProgramSpace fact, reviewer claim, evidence, finding, or acceptance.

M3's `ReviewContextEnvelope` is a reviewer-input projection and M4's claim assessment is an evidence/authority projection. Neither names a cover denominator, local assignment vocabulary, restriction, or gluing result. It is unsafe to infer those objects from an envelope, claim summary, or claim `assumptions` strings. A model sentence such as “the caller prevents retries” is not a machine-readable responsibility assignment.

The initial profile is intentionally narrower than the general design prose: only the fixed `double-submit-payment@1` gluing profile is admitted. It has two required contexts and one closed assignment key. This proves the M5 control loop without claiming a generic semantic-equivalence engine.

## Decision

### 1. Version, migration, replay authority, and frozen boundaries

`reviewgraphen.review_event.v1`, `.v2`, and `.v3` remain readable only under their respective contracts. Index projections v3/v4 and reports v1/v2/v3 are frozen. No existing journal, report, or index is patched, `ALTER TABLE`d, copied, upcast, or used as M5 evidence.

M5 introduces one homogeneous family:

```text
event:  reviewgraphen.review_event.v4
index:  reviewgraphen.index_projection.v5 (PRAGMA user_version = 5)
report: reviewgraphen.review.report.v4
```

A v4 run freshly performs D2 and M4 work. The v4 envelope/genesis use the v3 field layout and hashing algorithm with the v4 event discriminator. Inherited D2/M4 payload bytes, record IDs, DTO schema strings, and canonical encodings are unchanged, including the inherited `artifact_registered(ArtifactRegistrationV3)` payload needed by that prefix. M5 never widens that v3 registration. It introduces the distinct payload `artifact_registered_v4(ArtifactRegistrationV4)` below. Every inherited authority-bearing event is revalidated at its actual v4 `(run_id, genesis_hash, predecessor_hash, sequence, event_id)` position. A v3 replay basis, admission, registration admission, or event position is never reusable authority in v4.

`ArtifactRegistrationV4` is a strict, versioned record and not an alias/upcast of `ArtifactRegistrationV3`:

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

The first five source objects copy the v3 field sets and closed enum values exactly but are members of the new v4 enum; their presence does not convert a durable v3 registration into v4. `GluingInput` is the only source kind admitted by the M5 registration API. Its descriptor tuple must equal the outer `cas_hash`, `size`, `media_type`, and `sensitivity` field-for-field; media type is `application/vnd.reviewgraphen.gluing-input.v4+json` and sensitivity is `CanonicalState`. Unknown source kinds/fields are refused.

The exact registration identity is `StableId::derived("registration-v4", body)` where `body` has lexicographically ordered keys `cas_hash,media_type,run_id,sensitivity,size,source` and `source` is the complete closed object. The event actor is exactly `engine:reviewgraphen.m5_gluing_input@1`; a different actor is replay-invalid. `ArtifactRegistrationV4` contains no `body_hash`; index/report derive it as for the other v4 DTOs.

The exact v4 replay types are private, non-`Serialize`, non-`Deserialize`, non-`Clone`, and have no public constructor:

```rust
ReplayedV4RunSession {
  session_identity: OpaqueSessionIdentity,
  journal_lock: ExclusiveJournalLock,
  run_id, genesis_hash, snapshot_id, universe_id,
  confirmed_tail_hash, confirmed_event_count, next_sequence,
  aggregate: ReplayedV4Aggregate,
  state: Editable | NonEditable,
}
AuthorityReplayBasisV4 {
  schema: "reviewgraphen.authority_replay_basis.v4",
  run_id, genesis_hash, confirmed_tail_hash, confirmed_event_count,
  next_sequence, policy_revision_hash,
  inherited_m4_entries: Vec<AuthorityReplayEntryV3AtV4>,
  gluing_input_entries: Vec<GluingInputReplayEntryV4>,
  basis_digest,
}
AuthorityReplayEntryV3AtV4 {
  event_sequence, event_id, payload_kind, record_id, record_body_hash,
  predecessor_event_hash, v3_trust_binding_digest, v4_position_digest,
}
GluingInputReplayEntryV4 {
  event_sequence, event_id, registration_id, registration_body_hash,
  descriptor_id, descriptor_hash, descriptor_size,
  descriptor_media_type, descriptor_sensitivity,
  source: GluingInput,
  predecessor_event_hash, trust_binding_digest,
}
AuthorityTrustRootsV4 {
  policy_revision_hash, repository_id, repository_source_hash,
  allowed_harness_bindings: BTreeSet<AuthorityHarnessBindingV3Tuple>,
  human_grants: BTreeSet<AuthorityHumanGrantV3Tuple>,
  allowed_gluing_input_bindings: BTreeSet<GluingInputTrustBindingV4>,
}
AuthorityHarnessBindingV3Tuple {
  policy_revision_hash, repository_id, repository_source_hash,
  harness_id, harness_revision, harness_source_hash, test_artifact_id,
  descriptor_id, procedure_version, result_hash, result_size,
  result_media_type, result_sensitivity, run_id, genesis_hash,
  snapshot_id, universe_id, property_id, claim_id, claim_body_hash,
}
AuthorityHumanGrantV3Tuple {
  policy_revision_hash, actor, authority_id, capabilities,
  run_id, snapshot_id, universe_id, property_ids, claim_ids,
  valid_from, valid_until,
}
GluingInputTrustBindingV4 {
  policy_revision_hash, repository_id, repository_source_hash,
  run_id, genesis_hash, snapshot_id, universe_id, plan_id,
  profile_descriptor_id, context_id, descriptor_id,
  descriptor_hash, descriptor_size, descriptor_media_type,
  descriptor_sensitivity, registration_id, source: GluingInput,
}
```

The two `*V3Tuple` field sets and validation semantics are exactly ADR 0021's trust-root tuples; v4 copies them into its host capability rather than embedding, accepting, or translating an `AuthorityTrustRootsV3` value. `OpaqueSessionIdentity`, the lock guard, mutable aggregate, and health state never cross the API. `NonEditable` exposes no append/snapshot operation and can only be dropped.

`basis_digest` is the deterministic SHA-256 projection of the canonical basis body containing
`schema`, run/genesis, repository ID/source hash, snapshot/universe, policy revision, confirmed
tail/count, next sequence, and every complete ordered field of every inherited-M4 and gluing-input
replay entry. It excludes only `basis_digest` itself and the process-local `session_identity`.
Thus two fresh sessions that fully replay the same confirmed prefix under the same roots have the
same digest for index metadata, while a changed root, tail, coordinate, or entry field changes it.
`session_identity` remains a separate exact field in the in-memory basis and in every prepared
value, admission, confirmation seal/lease, and receipt; all use it independently of the digest to
reject cross-session token reuse and replay forks. It is never persisted in index metadata.
`v4_position_digest` is SHA-256 over the inherited ADR 0021 trust preimage plus the actual v4
run/genesis/predecessor/sequence/event tuple. `trust_binding_digest` is SHA-256 over the complete
gluing-input trust binding. The basis contains no secret and grants no fresh append authority.

Recovery is inspected read-only, then keyed and attributed explicitly. The inspection descriptor is closed, and the key is an opaque, private-field, non-`Serialize`, non-`Deserialize`, non-`Clone` one-shot value with no public constructor. Receipts are descriptive and never replay or append authority:

```rust
RecoveryInspectionV4 {
  run_id, genesis_hash,
  event_contract_version: "reviewgraphen.review_event.v4",
  expected_kind: RecoveryKindV4,
}
RecoveryKeyV4 {
  run_id, genesis_hash, event_contract_version: "reviewgraphen.review_event.v4",
  expected_kind: RecoveryKindV4,
  pre_recovery_offset, pre_recovery_tail_hash,
  pre_recovery_file_hash: Option<ContentHash>,
  pending_digest: Option<ContentHash>,
}
RecoveryKindV4 = GenesisBootstrap | CanonicalTail | M4BundleResume;
M4BundlePrefixStageV4 {
  classification: Stage0 | StrictInterior | AlreadyComplete,
  confirmed_events, expected_events,
}
M4BundleMarkerActionV4 = ClearedAndSynced | RetainedForResume;
M4BundleMarkerRecoveryReceiptV4 {
  prefix_stage: M4BundlePrefixStageV4,
  action: M4BundleMarkerActionV4,
  pre_marker_hash: ContentHash,
  post_marker_hash: Option<ContentHash>,
}
RecoveryOutcomeV4 =
  GenesisNotCommitted
| GenesisCommitted { event_id, event_hash, confirmed_offset }
| TailRecovered { good_offset, discarded_hash }
| M4BundleCleanupOrdinary { prefix_stage: M4BundlePrefixStageV4 }
| M4BundleResumeRequired { prefix_stage: M4BundlePrefixStageV4 };
RecoveryProvenanceV4 { actor, tool_version }
RecoveryReceiptV4 {
  schema: "reviewgraphen.recovery_receipt.v4",
  key: RecoveryKeyV4, kind: RecoveryKindV4, outcome: RecoveryOutcomeV4,
  provenance: RecoveryProvenanceV4, timestamp_unix_seconds,
  pre_file_hash: Option<ContentHash>, post_file_hash: Option<ContentHash>,
  marker_recovery: Option<M4BundleMarkerRecoveryReceiptV4>,
}

EventJournal::inspect_recovery_v4(
    &StoreRoot, RecoveryInspectionV4,
) -> Result<RecoveryKeyV4>;
EventJournal::replayed_v4_session(
    &AuthorityTrustRootsV4,
) -> Result<(ReplayedV4RunSession, AuthorityReplayBasisV4)>;
RecoveredV4Session =
  Editable { session: ReplayedV4RunSession, basis: AuthorityReplayBasisV4 }
| M4BundleResumeRequired {
    session: RecoveredM4BundleV4Session,
    resume_authority: VerificationBundleResumeAuthorityV4,
  };
EventJournal::recover_replayed_v4_session(
    &AuthorityTrustRootsV4, RecoveryKeyV4, RecoveryProvenanceV4,
) -> Result<(RecoveryReceiptV4, RecoveredV4Session)>;
```

`inspect_recovery_v4` acquires the same exclusive root/journal lock used by recovery, validates the requested run/genesis/version/kind against the observed filesystem and pending-marker state, performs no mutation, and returns only the opaque key. The lock guard never escapes. Recovery reacquires the lock and, immediately before mutation, rechecks every sealed key field against the current file identity, complete file hash, offset, tail, and pending marker; this is the mandatory TOCTOU check. The receipt's `kind` equals `key.expected_kind`, and its `pre_file_hash` equals `key.pre_recovery_file_hash`. Both receipt file hashes are exact optionals: `None` means that the log file did not exist at that boundary, not an empty-file hash. Wrong, stale, cross-run, cross-genesis, changed-after-inspection, or wrong-kind keys refuse without mutation. Actor and tool version are validated nonempty provenance; neither grants authority.

For a nonexistent genesis log, the inspected key has offset zero, the run/genesis chain-start hash as `pre_recovery_tail_hash`, `pre_recovery_file_hash=None`, and `pending_digest=None`. An empty existing file instead has `pre_recovery_file_hash=Some(sha256(empty bytes))`. `GenesisNotCommitted` removes any empty/partial unpublished log and has `post_file_hash=None`; `GenesisCommitted` has `Some` hashes for the same confirmed file at both boundaries. `CanonicalTail` always has an existing post-recovery file. Inspection never mutates a marker. `M4BundleResume` recovery either clears and fsyncs it for a cleanup-only prefix or retains it for a strict-interior resume, as classified below; because marker storage is separate from the journal, its journal `pre_file_hash` and `post_file_hash` remain equal and `Some`.

Kind and outcome are exhaustive and may not be mixed:

| `RecoveryKindV4` | Only allowed `RecoveryOutcomeV4` | Returned state |
| --- | --- | --- |
| `GenesisBootstrap` | `GenesisNotCommitted` or `GenesisCommitted` | closed `GenesisRecoveryV4` branch; never a session/basis |
| `CanonicalTail` | `TailRecovered` | ordinary `Editable` replay session and basis |
| `M4BundleResume` | `M4BundleCleanupOrdinary` for `Stage0` or `AlreadyComplete`; `M4BundleResumeRequired` only for `StrictInterior` | ordinary `Editable` session/basis after cleanup, or opaque resume-only recovered M4 bundle holder plus one-shot authority; no basis is exposed in the strict branch |

The M4 marker classification is exhaustive relative to the exact deterministic plan sealed by that marker. `expected_events>0`. `Stage0` means `confirmed_events=0` and no planned bundle event is durable. `StrictInterior` means `0<confirmed_events<expected_events` and the durable events are exactly the first `confirmed_events` entries of the ADR 0021 plan, including its static no-evidence stage omissions. `AlreadyComplete` means `confirmed_events=expected_events` and the entire exact plan is durable. Stage0 and already-complete recovery revalidate the complete marker/plan/trust closure, remove the marker, fsync the marker directory, then return an ordinary editable session and rebuilt basis; they never mint resume authority. Strict-interior recovery retains the marker and returns only an opaque resume-only holder and exact remaining-suffix authority. The rebuilt log and basis remain private inside that holder; neither is returned or inspectable before the sealed suffix is durable and the complete prefix has been replay-confirmed. A duplicate, gap, reorder, overlong prefix, unexpected planned-kind omission, foreign event, wrong body/ID/position, malformed marker, or suffix beyond the sealed plan is corrupt and refuses without marker cleanup, receipt, session, basis, or authority.

`marker_recovery` is `None` for the other two kinds and required for every successful `M4BundleResume` recovery. Its `prefix_stage` equals the outcome stage. Cleanup outcomes record `ClearedAndSynced` with `post_marker_hash=None`; strict-interior outcomes record `RetainedForResume` with `post_marker_hash=Some(pre_marker_hash)`. Any other classification/action/hash/outcome combination is invalid.

Both session APIs hold the exclusive journal lock. `open` scans the confirmed canonical v4 prefix, revalidates every inherited M4 authority event from original registered CAS bytes using the exact ADR 0021 rules plus its v4 position, validates every gluing-input registration against the exact v4 trust binding, and reconstructs the aggregate and basis. `recover` accepts only a `CanonicalTail` or `M4BundleResume` key, performs its keyed recovery, and then performs the identical scan. `CanonicalTail` and cleanup-only `M4BundleResume` return `Editable`; only a strict-interior M4 prefix returns the opaque resume-only branch, with its reconstructed state held privately until durable completion. Missing/mismatched trust roots, CAS bytes, policy revision, inherited authority, recovery key, or uncertain prefix return a typed refusal and no session.

Every v4 registration or bundle append takes `&mut AuthorityReplayBasisV4`, verifies its exact tail and next sequence, and consumes a sealed tail-bound prepared value. Only `Ok(receipt)` replaces `confirmed_tail_hash`, increments `confirmed_event_count` and `next_sequence`, and recomputes `basis_digest`; a successful trusted descriptor registration also appends its one `GluingInputReplayEntryV4`, while the authority-free M5 bundle appends no replay entry. A confirmed pre-durability failure leaves the basis unchanged. `SessionUncertain` and any interrupted inherited M4 bundle leave it byte-for-byte unchanged, consume the prepared value, make the session non-editable, and require `recover_replayed_v4_session`; a partial M4 bundle additionally requires the v4 one-shot resume authority defined below. No retry is inferred from a basis.

An inherited `ArtifactRegistrationV3` body may be freshly written inside a v4 envelope only through this distinct sealed bridge:

```rust
ArtifactRegistrationV3AtV4SourceRole =
  SnapshotIngest {
    adapter_id, snapshot_id, source_bundle_hash, source_bundle_total_bytes,
    source_bundle_entry_count, artifact_id, normalized_path, content_hash,
    line_count,
  }
| ReviewerRaw {
    execution_id, reviewer_id, raw_output_hash, raw_output_size,
    raw_output_media_type, raw_output_sensitivity,
  }
| VerifierInput {
    claim_id, claim_body_hash, descriptor_id, procedure_version,
  }
| VerifierOutput {
    claim_id, claim_body_hash, descriptor_id, procedure_version,
    expected_result_hash,
  }
| ExternalHarnessWitness {
    harness_binding: AuthorityHarnessBindingV3Tuple,
  };

ArtifactRegistrationV3AtV4TrustBinding {
  policy_revision_hash, repository_id, repository_source_hash,
  session_identity, run_id, genesis_hash, snapshot_id, universe_id,
  confirmed_tail_hash, expected_next_sequence,
  registration_id, registration_body_hash, cas_hash, size, media_type,
  sensitivity, complete_v3_source, source_role,
  role_closure_digest, trust_binding_digest,
}
ArtifactRegistrationV3AtV4Admission {
  binding: ArtifactRegistrationV3AtV4TrustBinding,
  registration: ArtifactRegistrationV3,
  actor, predecessor_event_hash, event_sequence,
}
ArtifactRegistrationV3AtV4Receipt {
  session_identity, run_id, registration_id, source_role,
  event_sequence, event_id, confirmed_tail_hash, expected_next_sequence,
  trust_binding_digest,
}
```

`complete_v3_source` is respectively the exact closed v3 `SnapshotIngest`, `ReviewerExecution`, `VerifierArtifact(role=input)`, `VerifierArtifact(role=output)`, or `ExternalHarnessWitness` object from ADR 0021. The outer CAS tuple equals the registration and role tuple. For `SnapshotIngest`, the source is exactly `{adapter_id,kind:"snapshot_ingest",run_id,snapshot_id}`, actor is exactly `reviewgraphen-core@1`, sensitivity is `WorkspaceSource`, and the selected bundle entry's artifact/path/content hash/CAS hash/byte size/line count must match both the current ProgramSpace file fact and registration. `source_bundle_hash=sha256(canonical SnapshotSourceBundle)`; its snapshot ID, path-ordered complete entries, and checked total bytes/count are sealed, so no missing/extra/reordered/sibling entry can share the closure.

`role_closure_digest` is SHA-256 over: the complete ProgramSpace plus canonical source-bundle closure and selected entry for snapshot ingest; the complete structured execution/raw-output registration closure for reviewer raw; the compiled descriptor, exact claim closure, and canonical input/result DTO for verifier input/output; or the complete harness trust-root tuple and witness DTO for external witness. `trust_binding_digest` covers every preceding field except itself. No strings, reviewer prose, report/index row, or partial tuple can select a role.

Every bridged v3 `artifact_registered` payload has event actor exactly `reviewgraphen-core@1`; actor is sealed alongside its role and v4 position. Role-specific authority comes from the validated closure/trust binding, never from changing that system actor.

```rust
ReplayedV4RunSession::prepare_artifact_registration_v3_at_v4(
  source: ValidatedArtifactRegistrationV3AtV4Source,
  registration: ArtifactRegistrationV3,
  basis: &AuthorityReplayBasisV4,
) -> Result<ArtifactRegistrationV3AtV4Admission>;
ReplayedV4RunSession::append_artifact_registration_v3_at_v4(
  admission: ArtifactRegistrationV3AtV4Admission,
  basis: &mut AuthorityReplayBasisV4,
) -> Result<ArtifactRegistrationV3AtV4Receipt>;
```

`ValidatedArtifactRegistrationV3AtV4Source` is a private nonserializable enum whose variants are constructed only by the lock-held replay session from, respectively, the current ProgramSpace plus a fully validated `SnapshotSourceBundle` and selected entry, a validated D2 execution output, a just-produced compiled verifier input, a just-produced compiled verifier output, or an exclusive borrow of `TrustedFixtureHarnessV4`. The harness capability itself is consumed only by the later witness-admission call. The registration admission is one-shot/non-`Clone`, seals exact session/run/genesis/tail/sequence/CAS/source role/trust binding/actor, and accepts no v3 admission token. Snapshot prepare additionally requires its run/snapshot/adapter, bundle hash/count/bytes, selected CAS tuple, source object, actor, and current aggregate closure field-for-field. Successful append advances basis tail/count/next/digest but adds no authority entry merely for registration. Pre-durable failure leaves the basis unchanged; interruption or `SessionUncertain` consumes the admission, leaves the caller basis unchanged, makes the session non-editable, and requires v4 recovery.

External witness ordering is strict: prepare and durably append its v3 registration at a v4 position; receive the updated basis; then call `admit_external_witness` with that exact receipt/registration and post-registration basis. The returned `ExternalWitnessAdmissionV4` seals that new tail and next sequence. Admission before durable success, after uncertainty, from a verifier/reviewer registration, or after any intervening append is `WitnessAdmissionMismatch`. A v3 external-witness admission is never accepted.

`RunGenesis` registration is the sole exception to the replay-session bridge because no session, tail, or basis exists before event 1. It is admitted only by bootstrap:

```rust
RunGenesisManifestV4 {
  run_id,
  event_contract_version: "reviewgraphen.review_event.v4",
  genesis_artifact: ArtifactRegistrationV3,
  repository_identity, snapshot_id, profile_id, profile_version,
}
RunGenesisBootstrapRequestV4 {
  run_id,
  canonical_genesis_bytes: Vec<u8>,
  repository_identity, snapshot_id, profile_id, profile_version,
}
EventLogV4::from_bootstrap_request(
  RunGenesisBootstrapRequestV4,
) -> Result<EventLogV4>;
EventLogV4::replay_confirmed_v4_prefix(
  run_id, canonical_genesis_bytes, envelopes,
  resolver: &impl AuthorityArtifactResolverV4,
  roots: &AuthorityTrustRootsV4,
  limits: EventReplayLimits,
) -> Result<(EventLogV4, AuthorityReplayBasisV4)>;
EventJournal::publish_new_v4(
  &StoreRoot, EventLogV4,
) -> Result<(EventJournal, GenesisCommitReceiptV4)>;
GenesisRecoveryV4 =
  NotCommitted { receipt: RecoveryReceiptV4 }
| Committed { receipt: RecoveryReceiptV4, journal: EventJournal };
EventJournal::recover_new_v4(
  &StoreRoot, RecoveryKeyV4, RecoveryProvenanceV4,
) -> Result<GenesisRecoveryV4>;
```

The bootstrap request's source closure is exactly its six fields. `canonical_genesis_bytes` must strict-canonical-decode as one `RunGenesisSnapshot`; its embedded run, repository identity, snapshot, profile ID, and profile version equal the five scalar fields, and its aggregate is pristine with no event-derived state. The request contains no path, `StoreRoot`, CAS handle, lock, journal, caller registration, manifest, event envelope, hash, ID, actor, or admission token.

Core's two `EventLogV4` operations are pure over supplied bytes/envelopes and have no `StoreRoot`, filesystem, lock, CAS-put, fsync, truncation, or recovery capability. `from_bootstrap_request` strictly validates the complete source closure, computes `genesis_hash=sha256(canonical_genesis_bytes)` and exact byte size, internally constructs and validates a private bootstrap admission containing the nested v3 registration, manifest, and canonical sequence-1 v4 envelope, applies that envelope to a pristine in-memory aggregate, and returns an opaque validated `EventLogV4`. The nested registration has `run_id`, `registration_id=StableId::derived("registration", exact v3 preimage)`, `cas_hash=genesis_hash`, `media_type="application/json"`, exact byte size, `CanonicalState`, and closed `RunGenesis {run_id}` source. The manifest contains that complete registration and is the canonical payload of event 1; applying the one event atomically inserts the nested registration and manifest, with no standalone registration event. No externally produced bootstrap admission, registration, manifest, envelope, hash, or ID is accepted. `replay_confirmed_v4_prefix` validates an already confirmed prefix through a read-only resolver.

Store is the only durable publisher. `publish_new_v4` consumes the opaque validated one-event `EventLogV4`, CAS-puts and fsyncs its exact sealed genesis bytes, then writes and syncs its exact sealed sequence-1 envelope. Store performs only storage-bound limit, collision, identity, and byte-integrity checks; it cannot construct, replace, or edit the request closure, nested registration, manifest, actor, envelope, or hashes. The pure in-memory constructors neither publish nor recover durable state.

`previous_event_hash=event_chain_genesis_hash(run_id,genesis_hash)`. `payload_hash` hashes the canonical `run_genesis_manifest` payload containing `RunGenesisManifestV4`. `event_id` and `event_hash` use the normal event identity/envelope-hash preimages with the exact v4 schema discriminator, run/genesis, sequence/logical time 1, actor, payload hash, and previous hash. Substituting a v3 event discriminator, v3 manifest contract string, actor, nested registration, or any hash changes both validation and hashes and is refused.

Bootstrap durability has one boundary: the CAS object is durable before the canonical sequence-1 JSONL line is attempted, and the journal becomes visible only after that complete line is synced. A failure before line durability returns `GenesisNotCommitted` and may leave only an authority-free orphan CAS object. Post-sync uncertainty returns `GenesisSessionUncertain`, returns no journal/session/basis, consumes the validated log, and forbids retry until `inspect_recovery_v4(... expected_kind=GenesisBootstrap)` produces a key for `EventJournal::recover_new_v4`. Genesis recovery accepts only that kind. Exactly one complete canonical sequence-1 event yields `GenesisRecoveryV4::Committed { receipt, journal }`; an absent log or an empty/recoverably partial unconfirmed line is removed and yields `GenesisRecoveryV4::NotCommitted { receipt }`, with no journal/session/basis. Any complete malformed, duplicated, wrong-actor, wrong-discriminator, or hash-mismatched line is a hard refusal. Only a committed journal may call `replayed_v4_session` to construct the initial basis. No other source role bypasses session/tail/sequence binding.

Fresh M4 work inside a v4 run uses only these v4 session capabilities, even though the durable M4 record bodies retain their v3 schemas:

```rust
TrustedFixtureHarnessV4; TrustedHumanAdmissionV4;
ExternalWitnessAdmissionV4; EvidenceAdmissionV4;
EvidenceBindingAdmissionV4; VerificationAdmissionV4; DecisionAdmissionV4;
ValidatedVerificationBundleV4; VerificationBundleResumeAuthorityV4;

ReplayedV4RunSession::admit_external_witness(
  TrustedFixtureHarnessV4, claim_id,
  ArtifactRegistrationV3AtV4Receipt,
  &AuthorityReplayBasisV4,
) -> Result<ExternalWitnessAdmissionV4>;
ReplayedV4RunSession::mint_verification_bundle(
  VerificationBundleRequestV4, Option<&ExternalWitnessAdmissionV4>,
) -> Result<ValidatedVerificationBundleV4>;
ReplayedV4RunSession::append_verification_bundle(
  ValidatedVerificationBundleV4, &mut AuthorityReplayBasisV4,
) -> Result<VerificationBundleReceiptV4>;
RecoveredM4BundleV4Session::prepare_resume(
  self, VerificationBundleResumeAuthorityV4, &OpaqueSessionIdentityV4,
) -> Result<PreparedVerificationBundleResumeV4>;
PreparedVerificationBundleResumeV4::envelopes(
  &self,
) -> &[EventEnvelope];
PreparedVerificationBundleResumeV4::confirm_replayed(
  self, confirmed_envelopes, resolver, roots, limits, session_identity,
) -> Result<(EventLogV4, AuthorityReplayBasisV4, VerificationBundleReceiptV4)>;
RecoveredM4BundleV4Session::resume_verification_bundle(
  self, VerificationBundleResumeAuthorityV4,
) -> Result<(ReplayedV4RunSession, AuthorityReplayBasisV4,
             V4VerificationBundleAppendReceipt)>;
ReplayedV4RunSession::mint_decision(
  TrustedHumanAdmissionV4, HumanDecisionRequestV4,
) -> Result<ValidatedDecisionV4>;
ReplayedV4RunSession::append_decision(
  ValidatedDecisionV4, &mut AuthorityReplayBasisV4,
) -> Result<DecisionReceiptV4>;
ReplayedV4RunSession::append_finding(
  ValidatedFindingV4, &mut AuthorityReplayBasisV4,
) -> Result<FindingReceiptV4>;
```

The sealed internal layouts are exact:

```rust
AuthorityAppendSealV4 {
  session_identity, policy_revision_hash, authority_scope_digest,
  run_id, genesis_hash, predecessor_event_hash, event_sequence,
  payload_kind, record_id, record_body_hash, actor,
}
ExternalWitnessAdmissionV4 {
  session_identity, run_id, genesis_hash, confirmed_tail_hash,
  expected_next_sequence, registration_id,
  harness_binding: HarnessBindingRoleV4,
}
EvidenceAdmissionV4(AuthorityAppendSealV4);
EvidenceBindingAdmissionV4(AuthorityAppendSealV4);
VerificationAdmissionV4(AuthorityAppendSealV4);
DecisionAdmissionV4(AuthorityAppendSealV4);
ValidatedVerificationBundleV4 {
  session_identity, start_predecessor_hash, start_sequence,
  basis_digest, start_authority_entry_count, authority_scope_digest,
  expected_events: Vec<ExpectedAuthorityEventV4>, sealed_admissions,
  envelopes: Vec<EventEnvelope>, verification_id,
}
VerificationBundleResumeAuthorityV4 {
  session_identity, policy_revision_hash, repository_id,
  repository_source_hash, run_id, genesis_hash, snapshot_id, universe_id,
  property_id, claim_id, claim_body_hash, scope: VerificationResumeScopeV4,
  prefix_stage, confirmed_tail_hash, expected_next_sequence,
  plan_digest, planned_event_ids,
  expected_events: Vec<ExpectedAuthorityEventV4>,
  remaining_envelopes: Vec<EventEnvelope>,
  start_authority_entry_count, verification_id,
}
VerificationResumeScopeV4 =
  Static { descriptor_id, procedure_version }
| Fixture(HarnessBindingRoleV4);
// HarnessBindingRoleV4 has exactly the AuthorityHarnessBindingV3Tuple field set.
RecoveredM4BundleV4Session {
  log: EventLogV4,
  basis: AuthorityReplayBasisV4,
}
PreparedVerificationBundleResumeV4 {
  pre_log: EventLogV4,
  pre_basis: AuthorityReplayBasisV4,
  authority: VerificationBundleResumeAuthorityV4,
}
ExpectedAuthorityEventV4 {
  event_schema: "reviewgraphen.review_event.v4",
  sequence, predecessor_event_hash, payload_kind, record_id, record_body_hash,
  event_id, event_hash,
}
```

For static verification `VerificationResumeScopeV4::Static` is exactly the compiled-static descriptor/procedure tuple defined by ADR 0021; fixture recovery instead carries the complete copied harness binding in the distinct closed `Fixture` variant. `planned_event_ids` covers the complete deterministic plan, while `expected_events` and `remaining_envelopes` are the same exact strict suffix of evidence/binding/verification stages. None may be supplied by a caller. The recovered holder and prepared stage layouts are implementation-private despite being named here normatively; their fields have no public accessor except the prepared stage's immutable exact-suffix borrow.

The authority-bearing and resume-state values listed above are one-shot, private-field, nonserializable, non-`Clone`, and collectively bind `session_identity`, v4 run/genesis/predecessor/tail/next sequence, policy revision, snapshot/universe/property/claim body, and the exact ADR 0021 authority scope. `ExpectedAuthorityEventV4` is the sole descriptive exception: its public read-only accessors describe an exact planned position/body/event tuple but grant no append authority. `TrustedFixtureHarnessV4` may be constructed only by the trusted host that actually ran the compiled harness and contains the complete `AuthorityHarnessBindingV3Tuple` plus the v4 envelope position. `TrustedHumanAdmissionV4` may be constructed only by the trusted host from one exact `AuthorityHumanGrantV3Tuple`, requested decision body, and v4 envelope position. Session minting checks those values against `AuthorityTrustRootsV4`; store/session never constructs trust.

The mint path internally creates the v4 evidence/binding/verification/decision admissions for exact next positions; no admission accessor is public. `M4BundleResume` recovery scans the confirmed v4 prefix and pending marker. For a strict interior only, it reconstructs `VerificationBundleResumeAuthorityV4` from the matching v4 trust root, CAS registrations, claim closure, and exact remaining v3-schema M4 bodies at v4 positions. Stage0 and already-complete recovery construct no resume authority and return the ordinary editable branch after marker cleanup. `RecoveredM4BundleV4Session` is public only as an opaque, private-field, nonserializable, non-`Clone` resume-only holder: it exposes no log, basis, snapshot, mint, ordinary append, or raw-event operation. Consuming the holder and one-shot authority creates a non-`Clone`, nonserializable `PreparedVerificationBundleResumeV4` whose sole borrow exposes the exact immutable suffix to Store's durability stage. Only after that stage is durably complete may consuming confirmation replay the full prefix under the same session identity and roots and release the editable log, rebuilt basis, and receipt. Store clears and fsyncs the marker only after the exact suffix is complete. Any interruption or uncertainty consumes the holder/stage, exposes no basis, and requires another inspected `M4BundleResume` key. Resume follows ADR 0021's exhaustive prefix table but replaces every session/admission/resume position binding with v4. No `*V3`, `TrustedFixtureHarnessV1`, `AuthorityTrustRootsV3`, or `VerificationBundleResumeAuthorityV3` value is accepted by any v4 API.

Every successful v4 append, including authority-free registration, finding, and M5 bundle events, advances basis tail/count/next sequence and recomputes its digest. A successful evidence, binding, verification, or decision append additionally adds exactly one `AuthorityReplayEntryV3AtV4` per durable authority-bearing event; a successful gluing-input registration adds exactly one gluing-input entry. Interrupted/uncertain multi-event M4 append updates neither caller basis nor exposed session state and recovery rebuilds both entry vectors from the confirmed prefix.

There is no v3-to-v4 migration. A v3 journal/index request against v5 returns `RebuildRequired { found: 4, required: 5 }`; existing images remain immutable. M5 is single-snapshot. Cross-snapshot input is `SnapshotMismatch`, not stale evidence; morphisms and invalidation remain M6.

### 2. Exact profile and source-bound assignment input

The only code-owned profile is:

```text
reviewgraphen.double_submit_gluing@1
profile_id: double-submit-payment@1
property_id: payment.at_most_once
invariant_id: invariant:payment-at-most-once
required_context_ids: [context:payment, context:ui-event]
assignment_key: caller_duplicate_protection
```

Its only values are `satisfied`, `required`, and `unknown`; there are no aliases or truthy conversions. The compatibility table is exhaustive:

| left | right | compatibility |
| --- | --- | --- |
| `satisfied` | `satisfied` | `compatible` |
| `required` | `required` | `compatible` |
| `satisfied` | `required` | `conflict` |
| `required` | `satisfied` | `conflict` |
| `unknown` | any | `unknown` |
| any | `unknown` | `unknown` |

Assignments are not hard-coded per context and are never parsed from repository text, claim prose, model output, labels, or attributes. Before the M5 bundle, a trusted operator/profile host creates exactly one canonical CAS document per required context:

```rust
GluingInputDescriptorV4 {
  schema: "reviewgraphen.gluing_input_descriptor.v4",
  id, run_id, snapshot_id, universe_id, plan_id,
  profile_descriptor_id, context_id,
  assignment_key, assignment_value,
  qualification_source_ids,
}
// identity keys: assignment_key, assignment_value, context_id, plan_id,
// profile_descriptor_id, qualification_source_ids, run_id, snapshot_id, universe_id
```

The descriptor has closed fields and sorted/unique qualifications. Its exact canonical bytes are stored under `descriptor_hash=sha256(bytes)` and registered only through `artifact_registered_v4(ArtifactRegistrationV4)` with:

```text
media_type  = application/vnd.reviewgraphen.gluing-input.v4+json
sensitivity = canonical_state
source      = exact closed GluingInput object from §1
actor       = engine:reviewgraphen.m5_gluing_input@1
```

`TrustedGluingInputAdmissionV4` is a one-shot nonserializable capability minted only while the v4 session lock is held after exact CAS decode, DTO/ID/hash/size/media/sensitivity/source checks, and equality with one `allowed_gluing_input_binding`. It seals the complete `ArtifactRegistrationV4`, actor, tail, and next sequence and authorizes only that v4 registration. The bundle refers to the descriptor and v4 registration IDs and rechecks their bytes. A v3 registration, registration without a matching descriptor, descriptor without its exact registration, duplicate contexts, or untrusted binding is a typed refusal, not an `unknown` result.

```rust
ReplayedV4RunSession::admit_gluing_input_registration(
  TrustedGluingInputSourceV4, GluingInputDescriptorV4,
  &AuthorityReplayBasisV4,
) -> Result<TrustedGluingInputAdmissionV4>;
ReplayedV4RunSession::append_artifact_registration_v4(
  TrustedGluingInputAdmissionV4, &mut AuthorityReplayBasisV4,
) -> Result<ArtifactRegistrationReceiptV4>;
```

`TrustedGluingInputSourceV4` is host-constructed, nonserializable, and contains exactly one `GluingInputTrustBindingV4`; session admission requires equality with the trust-root member and CAS object. No raw `ArtifactRegistrationV4` append is public.

Runtime construction uses a narrower Core-owned projection. From an exact confirmed legal
zero/one/two-descriptor pre-bundle V4 prefix and its matching `AuthorityReplayBasisV4`, Core may mint one opaque
`M5GluingProfileBasisV4`. Minting requires the fixed profile, the unique current plan, the exact
ProgramSpace/universe/property/context/overlap closure, and the current M4 assessment closure.
The basis is non-`Clone`, non-`Serialize`, non-`Deserialize`, has no public constructor, and is
bound to the observed run, genesis, tail, event count, policy, repository, snapshot, universe,
and plan. It cannot be minted from an index, report, descriptor, prose, or caller-provided IDs.

The only semantic caller input is a closed pair
`M5DoubleSubmitAssignmentsV4::new(payment, ui_event)` using `AssignmentValueV4`; contexts,
order, assignment key, profile descriptor, plan, property, and qualification IDs are not caller
arguments. Consuming the basis derives exactly two canonical descriptor byte strings in
payment-then-ui order and two matching trust-root bindings. It verifies every already registered
descriptor against that exact derivation and emits one-shot source objects only for the remaining
legal suffix. The payment qualification set is the required overlap plus
the selected current payment assessment's evidence IDs when that assessment exists; with no
eligible payment assessment, or an eligible assessment whose evidence set is empty, it is the
required overlap alone. The UI qualification set is empty for this profile. A missing assessment
does not block descriptor generation: it remains absent from `S` and produces the normative
`required_section_missing` result in §4–§5. Ambiguous eligible claim/assessment closure and
qualification overflow refuse rather than silently selecting or dropping a source.

The exact profile projection source set is:

```text
RP = cover.source_ids ∪ O
     ∪ ⋃{ {c,o(c),q(c)} ∪ M(c) ∪ source_ids(q(c))
           ∪ B(c) ∪ E(c) ∪ V(c) ∪ J(c) ∪ F(c) | eligible c }
     ∪ {descriptor and registration IDs already present in the legal prefix}
```

Because `O`, `c`, and `M(c)` are already in `cover.source_ids`, its dedicated conservative count
bound is `16,387 + 2*(2 + 128 + 5*64) + 4 = 17,291` IDs. Its retained-byte bound is
`17,291 * (size_of::<StableId>() + 256)`. Core performs a borrowed union count, checked retained-
byte sum, and per-ID 256-byte check before cloning/extending the target set, then rechecks the
final 0/1/2-prefix union after adding existing descriptor/registration IDs. The attempt-source
bound of 4,944 is unrelated and is never reused for this projection.

The projection exposes its complete source-ID set and the fixed meaningful-information-loss
declaration `non_qualification_assessment_detail_omitted`. It does not expose a serializable
canonical authority record. Its host material consumes an existing `AuthorityTrustRootsV4`,
requires the exact policy/repository tuple and no pre-existing gluing roots, and returns a new
root set containing the same private inherited harness/human roots plus exactly the derived pair.
Public augmentation refuses even one preseeded exact gluing binding; only Core's private second
pass may replace the exact pair it minted during the same inspection operation.
This root augmentation grants no append by itself: Runtime must drop the inspection session,
freshly replay the identical prefix under the augmented roots, CAS-put the returned exact bytes,
and pass each one-shot source through ordinary locked admission. Bindings and descriptor bytes
remain stable across legal 0/1/2 restarts; append position/session binding is performed only by
ordinary admission against the fresh replay. A cross-run, stale-prefix, reordered, substituted,
or post-bundle mint refuses; after the bundle, only the existing read-only bundle projection is
available.

Before inspection scans or decodes a payload discriminator or calls the CAS resolver, it applies
the same checked V4 replay preflight to the complete input: `event_count <= max_events` and the
checked sum of canonical envelope bytes is `<= max_canonical_bytes`. The subsequent M5-start scan
reads only the top-level closed `type` discriminator without allocation; complete payload decode
and trust validation occur only in the two authority replay passes.

An unregistered descriptor CAS object left by a crash is adoptable only as those exact immutable bytes; adoption never deletes, rewrites, or substitutes the object. Under the session lock, Store must strictly canonical-decode it as one `GluingInputDescriptorV4`, verify its hash/size/media type/sensitivity and complete run/genesis/snapshot/universe/plan/profile/context closure, require equality with exactly one `allowed_gluing_input_binding`, require that no v4 registration exists for that descriptor or context, and require that its context is the next legal context in payment-then-ui order. Successful adoption uses the same derived descriptor/registration IDs and emits the same registration event as a fresh CAS put. An ambiguous binding, extra or out-of-order object, byte/closure mismatch, or object not eligible for the next registration is `OrphanCanonicalInput`; an already registered descriptor is reconstructed from the journal and never appended again. A `CasStore::put` result such as `existed=true` is storage information only and grants no admission or replay authority.

Every `qualification_source_id` must resolve at the current tail either to a ProgramSpace ID in `D` or to a binding/evidence/verification/decision/finding ID in the chosen claim's exact M4 assessment; otherwise admission refuses. The qualification array is part of the descriptor bytes, ID, hash, registration, trust binding, and replay entry, so it cannot be added by the bundle or model prose.

The two descriptor values are independent. Therefore every result is reachable without prose: a compatible pair with an unpassed Section yields `candidate`; a compatible passed pair with any nonempty qualification union yields `glued_with_qualification`; a compatible passed pair with an empty union yields `glued`; a conflict pair yields `failed`; and either `unknown` yields `unknown`. The reference fixture explicitly registers payment=`required` and ui-event=`satisfied`; that fixture data is not a code conclusion.

### 3. Strict DTOs, identity, and atomic ownership

Every event DTO below has `#[serde(deny_unknown_fields)]`, an exact schema discriminator, closed enums, and sorted/unique set arrays. No event DTO contains `body_hash`. IDs are `StableId::derived(kind, canonical_identity_body)` with exactly the comment-listed lexicographic keys. A caller ID mismatch is `Validation`; any repeated ID is `IdCollision`, including identical bytes. Index/report compute `body_hash = sha256(canonical complete DTO including schema and id)` after validation and store/project that derived value; it is never caller or event input and therefore cannot hash itself.

All trace arrays below are present even when empty:

```rust
ContextCoverV4 {
  schema: "reviewgraphen.context_cover.v4",
  id, run_id, snapshot_id, universe_id, plan_id, profile_descriptor_id,
  selected_obligation_ids, required_context_ids, cover_domain_ids,
  covered_domain_ids, uncovered_domain_ids, source_ids,
}
// identity: cover_domain_ids, plan_id, profile_descriptor_id,
// selected_obligation_ids, required_context_ids, run_id, snapshot_id, universe_id

SectionV4 {
  schema: "reviewgraphen.section.v4",
  id, cover_id, context_id, snapshot_id, property_id, invariant_id,
  obligation_id, claim_id, claim_assessment_id,
  input_descriptor_id, input_registration_id,
  assignment_key, assignment_value, passed_current_verification,
  source_ids, qualification_source_ids, binding_ids, evidence_ids,
  verification_ids, decision_ids, finding_ids,
}
// identity: assignment_key, assignment_value, claim_id, context_id, cover_id,
// input_descriptor_id, input_registration_id, invariant_id, obligation_id,
// property_id, qualification_source_ids, snapshot_id

RestrictionV4 {
  schema: "reviewgraphen.restriction.v4",
  id, section_id, context_pair, overlap_member_ids, assignment_key,
  assignment_value, source_ids, qualification_source_ids, claim_ids,
  evidence_ids, verification_ids, decision_ids, finding_ids,
}
// identity: assignment_key, assignment_value, context_pair,
// overlap_member_ids, qualification_source_ids, section_id

GlobalCandidateV4 {
  schema: "reviewgraphen.global_candidate.v4",
  id, cover_id, invariant_id, property_id, required_section_ids,
  restriction_ids, qualification_source_ids, source_ids, claim_ids,
  evidence_ids, verification_ids, decision_ids, finding_ids,
}
// identity: cover_id, invariant_id, property_id, qualification_source_ids,
// required_section_ids, restriction_ids

GluingAttemptV4 {
  schema: "reviewgraphen.gluing_attempt.v4",
  id, cover_id, snapshot_id, property_id, invariant_id,
  input_descriptor_ids, section_ids, restriction_ids, result,
  global_candidate_id: Option<StableId>, obstruction_id: Option<StableId>,
  source_ids, claim_ids, evidence_ids, verification_ids, decision_ids, finding_ids,
}
// identity: cover_id, input_descriptor_ids, invariant_id, property_id,
// restriction_ids, result, section_ids, snapshot_id

GluingObstructionV4 {
  schema: "reviewgraphen.gluing_obstruction.v4",
  id, attempt_id, kind, conflicting_context_ids, section_ids,
  overlap_member_ids, assignment_key,
  left_assignment_value: Option<AssignmentValue>,
  right_assignment_value: Option<AssignmentValue>,
  source_ids, claim_ids, evidence_ids, verification_ids, decision_ids,
  finding_ids, affected_invariant_id, severity, required_resolution,
  human_decision_required, blocks,
}
// identity: affected_invariant_id, assignment_key, attempt_id, blocks,
// conflicting_context_ids, kind, left_assignment_value, overlap_member_ids,
// required_resolution, right_assignment_value, section_ids, severity

GluingBundleV4 {
  schema: "reviewgraphen.gluing_bundle.v4",
  cover: ContextCoverV4,
  input_descriptor_ids: [StableId; 2],
  sections: Vec<SectionV4>,
  restrictions: Vec<RestrictionV4>,
  attempt: GluingAttemptV4,
  global_candidate: Option<GlobalCandidateV4>,
  obstruction: Option<GluingObstructionV4>,
}
```

`GluingBundleV4` is the sole M5 payload and owns every M5 DTO exactly once. `GluingAttemptV4` is an IDs-only summary and nests no record. Exactly one bundle, one cover, and one attempt may exist per run. Input descriptors remain canonical registered CAS documents and are referenced, not duplicated, by the bundle.

The `StableId::derived` kind argument is not inferred from schema or Rust type. The complete literal mapping is:

| Record | Literal kind |
| --- | --- |
| `ArtifactRegistrationV4` | `registration-v4` |
| `GluingInputDescriptorV4` | `gluing-input-descriptor-v4` |
| `ContextCoverV4` | `context-cover-v4` |
| `SectionV4` | `section-v4` |
| `RestrictionV4` | `restriction-v4` |
| `GlobalCandidateV4` | `global-candidate-v4` |
| `GluingAttemptV4` | `gluing-attempt-v4` |
| `GluingObstructionV4` | `gluing-obstruction-v4` |

`GluingBundleV4` has no independent ID. No other literal, unversioned alias, enum debug string, or schema string is accepted as a kind.

The following arrays are positional wire arrays, not generic ID sets, and are always ordered by ascending owning `context_id`, exactly payment then ui-event: the two registered descriptors and `GluingBundleV4.input_descriptor_ids`; `sections`; `restrictions`; `attempt.input_descriptor_ids`, `attempt.section_ids`, and `attempt.restriction_ids`; `candidate.required_section_ids` and `candidate.restriction_ids`; and any obstruction's `section_ids`. `required_context_ids`, `context_pair`, and `conflicting_context_ids` are also context-ID ordered. Missing Sections/Restrictions preserve the relative order without placeholders. Every other ID collection is a sorted/unique set in raw StableId byte order. Append, replay, index snapshot, and report require this exact distinction and order.

### 4. Exact cover, Section, restriction, and trace sets

Let `C={context:payment, context:ui-event}`, `M(c)=member_ids(c)`, `O=M(payment)∩M(ui)`, `Q` be selected `payment.at_most_once` obligations, and `A(c)` the assessment for the selected structured D2 claim `q(c)`. The one cover denominator is:

```text
D = invariant.scope_ids ∪ M(payment) ∪ M(ui-event)
    ∪ ⋃{target_refs(o) ∪ source_ids(o) | o∈Q}
K = M(payment) ∪ M(ui-event) ∪ C ∪ {invariant:payment-at-most-once}
covered_domain_ids   = D ∩ K
uncovered_domain_ids = D \ K
cover.source_ids     = D ∪ C ∪ {invariant:payment-at-most-once}
```

All are sorted sets; the partition is disjoint and unions to `D`. Every member resolves in the same snapshot. `Q` is nonempty and is exactly the cover's selected obligations from the recorded plan/universe.

For context `c`, a Section is eligible iff exactly one selected structured D2 claim addresses exactly one `o(c)∈Q`, has a source in `M(c)`, has current assessment `A(c)`, and has exactly one registered descriptor for `c`. Eligibility does **not** depend on `A(c).disposition`, `A(c).review_status`, active decision, finding status, confidence, or acceptance. Missing claim/assessment closure produces no Section and later `required_section_missing`; a descriptor value `unknown` produces a Section and later `section_unknown`.

Define the exact trace sets:

```text
B(c)=A(c).binding_ids       E(c)=A(c).evidence_ids
V(c)=A(c).verification_ids  J(c)=A(c).decision_ids
F(c)=A(c).finding_ids
X(c)=source_ids(q(c)) ∩ M(c)
P(c)={ v∈V(c) | v.outcome=Passed and v cites a nonempty evidence set whose
                   every evidence has a Reproduces binding in B(c) to q(c)
                   and o(c), all at the current snapshot }
passed_current_verification(c) = (P(c) != ∅)
```

The Section arrays equal `B/E/V/J/F` exactly, not a filtered sign-off subset. Its sets are:

```text
section.qualification_source_ids = descriptor(c).qualification_source_ids
section.source_ids = {cover_id,c,invariant_id,o(c),q(c),descriptor(c).id,
                      descriptor_registration(c).id}
                     ∪ X(c) ∪ B(c) ∪ E(c) ∪ V(c) ∪ J(c) ∪ F(c)
```

The only pair is `[context:payment,context:ui-event]`. No label/prose/alias/traversal expands `O`. Restrictions exist exactly as `R={r(c) | s(c)∈S and function:checkout-submit∈O}`: zero if the required overlap is invalid, otherwise one per existing Section. Each `r(c)` is:

```text
restriction.claim_ids        = {q(c)}
restriction.evidence_ids     = E(c)
restriction.verification_ids = V(c)
restriction.decision_ids     = J(c)
restriction.finding_ids      = F(c)
restriction.qualification_source_ids = descriptor(c).qualification_source_ids
restriction.source_ids = {s(c).id,c,cover_id,invariant_id,o(c),q(c),
                          descriptor(c).id,descriptor_registration(c).id}
                         ∪ O ∪ B(c) ∪ E(c) ∪ V(c) ∪ J(c) ∪ F(c)
```

For any set of existing sections `S` and restrictions `R`, define `claims(S)`, `evidence(S)`, `verifications(S)`, `decisions(S)`, `findings(S)`, and `qualifications(S)` as exact sorted unions of the correspondingly named arrays. Define these exact bases:

```text
trace(S) = claims(S) ∪ evidence(S) ∪ verifications(S)
           ∪ decisions(S) ∪ findings(S)
sources(S,R) = IDs(S) ∪ IDs(R)
               ∪ ⋃{s.source_ids | s∈S} ∪ ⋃{r.source_ids | r∈R}
descriptor_refs = {both descriptor IDs, both registration IDs}
attempt_base(S,R) = {cover_id,invariant_id} ∪ descriptor_refs
                    ∪ sources(S,R) ∪ trace(S) ∪ qualifications(S)
```

### 5. Exact result, candidate, and obstruction matrix

The result is selected by the first matching row:

| Predicate | Sections / Restrictions | Result | Candidate | Obstruction |
| --- | --- | --- | --- | --- |
| one or both eligible Sections absent | `0..1 / (0 if anchor absent, otherwise Section count)` | `unknown` | absent | one `required_section_missing` |
| `O` empty or lacks `function:checkout-submit` | `2 / 0` | `unknown` | absent | one `required_overlap_missing` |
| either descriptor value is `unknown` | `2 / 2` | `unknown` | absent | one `section_unknown` |
| compatibility is `conflict` | `2 / 2` | `failed` | absent | one `assignment_conflict` |
| compatible and either `passed_current_verification=false` | `2 / 2` | `candidate` | present | absent |
| compatible, both passed, qualifications union nonempty | `2 / 2` | `glued_with_qualification` | present | absent |
| compatible, both passed, qualifications union empty | `2 / 2` | `glued` | present | absent |

Thus failed and unknown never have a `GlobalCandidateV4`; candidate and both glued results always have exactly one. The candidate is not a claim, evidence, verification, finding, fact, acceptance, coverage numerator, global safety result, or sign-off.

For a present candidate `g`:

```text
g.required_section_ids       = IDs(S)               // cardinality 2
g.restriction_ids            = IDs(R)               // cardinality 2
g.qualification_source_ids   = qualifications(S)
g.claim_ids                  = claims(S)
g.evidence_ids               = evidence(S)
g.verification_ids           = verifications(S)
g.decision_ids               = decisions(S)
g.finding_ids                = findings(S)
g.source_ids = attempt_base(S,R)
```

For every result, attempt arrays equal `IDs(S)`, `IDs(R)`, and the exact named trace unions above. `attempt.input_descriptor_ids` is always both descriptor IDs. Its `source_ids` equals `attempt_base(S,R)` for every result and never contains its candidate or obstruction ID. Option/cardinality is exactly the result table. Replay separately requires exact option fields for the result.

The obstruction fields are exhaustive:

| Kind | contexts / sections / overlap | assignments | trace/source sets | resolution / human / blocks |
| --- | --- | --- | --- | --- |
| `required_section_missing` | `conflicting_context_ids=C\contexts(S)`; `section_ids=IDs(S)`; `overlap_member_ids=O` | both null | named trace arrays equal the five unions over `S`; `source_ids=attempt_base(S,R)∪conflicting_context_ids∪O` | `record_required_section` / false / `{invariant_id}` |
| `required_overlap_missing` | `conflicting_context_ids=C`; `section_ids=IDs(S)` (2); `overlap_member_ids=O` | both null | named trace arrays equal the five unions over `S`; `source_ids=attempt_base(S,R)∪C∪O` | `resolve_context_overlap` / false / `{invariant_id}` |
| `section_unknown` | IDs of contexts whose value is `unknown`; `section_ids=IDs(S)` (2); `overlap_member_ids=O` | left/right are the payment/ui values in context order, including `unknown` | named trace arrays equal the five unions over `S`; `source_ids=attempt_base(S,R)∪conflicting_context_ids∪O` | `resolve_unknown_duplicate_protection_assignment` / false / `{invariant_id}` |
| `assignment_conflict` | `conflicting_context_ids=C`; `section_ids=IDs(S)` (2); `overlap_member_ids=O` | non-null payment/ui values in context order | named trace arrays equal the five unions over `S`; `source_ids=attempt_base(S,R)∪C∪O` | `resolve_duplicate_protection_responsibility` / true / `{invariant_id}` |

For every obstruction, `attempt_id` is a non-source ownership/back-reference required to equal the bundle's attempt. It is excluded from `source_ids` and from the obstruction identity's source closure. Conversely, attempt source IDs exclude all downstream candidate/obstruction IDs. The resulting source graph is strictly descriptor/ProgramSpace/M4 trace → Section → Restriction → Attempt/Candidate/Obstruction, with no cycle. `severity=critical` only for conflict; all others are `high`. Null assignments are allowed only in the first two rows. Exactly one obstruction exists only for `failed`/`unknown`; no obstruction ever blocks a nonexistent candidate ID.

The four table values are the complete required-resolution enum. In particular, `section_unknown` uses only `resolve_unknown_duplicate_protection_assignment`; `record_required_section` is valid only when an eligible Section is absent and never means that an existing `unknown` assignment should be fabricated or accepted.

M5 never creates or mutates ProgramSpace facts, D2 claims, M4 assessments, evidence, bindings, verifications, decisions, findings, lifecycle, coverage sets, report status, or CI gate. Disposition/review status are trace only and cannot change eligibility or result.

### 6. Bounds and accounting

All limits are inclusive and checked before allocation/event encoding:

| Exact collection | Limit |
| --- | ---: |
| gluing-input descriptors / run; descriptor IDs / bundle or attempt | 2 |
| descriptor `qualification_source_ids` | 32 |
| required contexts / cover; context pair; conflicting contexts | 2 |
| selected obligations / cover | 2,048 |
| members / required context; overlap IDs / restriction or obstruction | 4,096 |
| cover domain, covered, or uncovered IDs | 16,384 |
| cover `source_ids` (`D` plus two contexts and invariant) | 16,387 |
| Sections / cover; Section IDs / attempt, candidate, or obstruction | 2 |
| claim source IDs retained / Section | 128 |
| each Section binding/evidence/verification/decision/finding set | 64 |
| Section `qualification_source_ids` | 32 |
| Section `source_ids` (`7 + 128 + 5*64`) | 455 |
| assignments / Section; context pairs / attempt | 1 |
| restrictions / attempt; restriction IDs / attempt or candidate | 2 |
| Restriction claim IDs | 1 |
| each Restriction evidence/verification/decision/finding set | 64 |
| Restriction `qualification_source_ids` | 32 |
| Restriction `source_ids` (`8 + 4,096 + 5*64`) | 4,424 |
| Attempt claim IDs | 2 |
| each Attempt evidence/verification/decision/finding union | 128 |
| Attempt `source_ids` maximum | 4,944 |
| Candidate claim IDs | 2 |
| each Candidate evidence/verification/decision/finding union | 128 |
| Candidate `qualification_source_ids` union | 64 |
| Candidate `source_ids` maximum | 4,944 |
| Obstruction claim IDs | 2 |
| each Obstruction evidence/verification/decision/finding union | 128 |
| Obstruction `source_ids` maximum | 4,944 |
| obstruction/candidate option, blocks, or required-resolution tags | 1 |
| covers / run; attempts / run; gluing obstructions / attempt | 1 |
| omission-loss `source_ids` for descriptor/cover/Section/attempt/restriction/candidate/obstruction array | `4/1/2/1/2/1/1` |
| canonical descriptor object bytes | 65,536 |
| canonical gluing bundle event bytes | 1,048,576 |

Existing StoreLimits remain fixed: event line `1,048,576`, CAS object `67,108,864`, index rows `1,000,000`, index serialized bytes `67,108,864`, index query bytes `16,777,216`, and index working bytes `268,435,456`. M5 creates exactly two bounded canonical descriptor CAS objects before the bundle; any extra object/registration is refused.

The `4,944` union ceiling is the exact joint maximum of fixed cover/invariant (2), descriptor/registration IDs (4), Section/Restriction IDs (4), context/obligation/claim IDs (6), overlap plus retained context-member claim sources (at most 4,224), five per-Section M4 trace sets (640), and qualifications (64). The `4,224` term follows from both context member sets being capped at 4,096: overlap 3,968 plus 128 disjoint retained sources from each side reaches the maximum; a larger overlap reduces available disjoint member sources one-for-one or faster. Duplicate IDs reduce the actual set and never buy additional capacity. The obstruction adds only contexts/overlap already counted and explicitly excludes its owner attempt. Every row above has a real exact-limit acceptance test, a `limit+1` refusal before reserve/encoding, duplicate/order mutation tests, and checked-`u64` overflow tests; testing only the containing event byte limit is insufficient.

For v5, `Rows5` counts every inherited-v4 and M5 row; `Icells5` counts every non-null INTEGER cell; `Tbytes5` sums every non-null TEXT UTF-8 length; `SQL5=Tbytes5 + 8*Icells5 + Rows5`; `Qbytes5` is canonical complete `IndexSnapshotV5` bytes; and `Owned5` is recursive decoded-snapshot ownership (UTF-8 string/key bytes, 8/list slot, 16/object entry, 8/integer/float, 1/Boolean, 0/null). `ObservedObject5` is the largest actual CAS-object byte length referenced by any inherited or v4 artifact-registration row in this rebuilt run, or zero when there is none. `ObservedEventLine5` is the largest actual confirmed canonical v4 event-line byte length including LF, or zero for an empty prefix. Exact peak:

```text
Working5 = SQL5 + Qbytes5 + Owned5 + ObservedObject5 + ObservedEventLine5
```

The two observed terms are measured run data, not the configured `StoreLimits` caps; they are independently required not to exceed the CAS-object and event-line limits. All arithmetic is checked `u64`, overflow observes `u64::MAX`, and Rows/Qbytes/Working are checked against StoreLimits before reserve/allocation/return. Page size, compressed bytes, allocator/RSS observations, configured maxima substituted for observed values, and a v4 delta are invalid substitutes.

Report v4 retains v3 limits (8,192 inherited registrations/executions; 131,072 claims; 8,192 ordinary obstructions; 3 views; 4,096 losses; 200,000 rows; 67,108,864 output bytes; 268,435,456 working bytes) and adds bounds of two `ArtifactRegistrationV4` records, two gluing-input descriptors, one cover, two Sections, one attempt, two restrictions, one candidate, and one gluing obstruction. Exact row formula:

```text
report_rows_v4 = report_rows_v3 + artifact_registrations_v4
               + gluing_input_descriptors + covers + sections + attempts + restrictions
               + global_candidates + gluing_obstructions
```

Let `J` be confirmed canonical event bytes plus LF, `I5=Qbytes5`, `Rr4` preflight report reservation, `R4` realized recursive ownership, `S4` largest canonical complete record, and `O4` canonical output bytes. Exact enforced peaks are `projection_peak_v4=J+I5+Rr4` and `serialization_peak_v4=J+I5+R4+S4+O4`. Limit/overflow returns `Incomplete { operation, limit, observed }` before output/index snapshot publication.

### 7. Event order, replay, CAS, and crash protocol

V4 adds exactly two new payload variants; only the second owns M5 topology records:

```text
artifact_registered_v4(ArtifactRegistrationV4)
actor = engine:reviewgraphen.m5_gluing_input@1
gluing_bundle_recorded_v4(GluingBundleV4)
actor = engine:reviewgraphen.m5_gluing@1
```

The only legal order is: confirmed D2/M4 prefix (including any inherited v3 registration payloads); zero, one, or two trusted descriptor CAS puts and exact `artifact_registered_v4` events in context-ID order; then, only after exactly two registrations, one `gluing_bundle_recorded_v4`. Thus the legal pre-bundle confirmed prefix contains exactly 0, 1, or 2 descriptors/registrations, never an out-of-order or third one. A gluing-input descriptor can never use inherited `artifact_registered`. Core prepares the complete bundle against a cloned replay aggregate and the exact current tail. Store appends that single canonical line atomically; one cover, 0..2 Sections, 0..2 Restrictions, one attempt, 0..1 candidate, and 0..1 obstruction therefore share one event tuple and cannot become independently durable. The bundle-owned arrays are all empty before that event and atomically complete after it. A second M5 bundle/cover/attempt is always `AlreadyComplete`, never a later revision.

A crash before descriptor registration can leave an unregistered CAS object. It is neither replay nor report authority and may be adopted only by the exact immutable-object rule in §2; otherwise it is `OrphanCanonicalInput`. A crash after one or both registrations can leave valid unused registrations, but no M5 result. Reopen reconstructs them from the journal and exact CAS bytes; the same two inputs may then be used only to prepare the not-yet-durable bundle. A pre-sync bundle failure leaves no M5 record. Post-sync uncertainty consumes the bundle and basis, returns `SessionUncertain`, and requires v4 recovery to decide whether the one event landed; it is never retried. A durable bundle is replayed whole or the journal is refused as noncanonical.

M5 performs no process/network/tool operation. Recovery, v5 rebuild, and report generation use only the confirmed canonical journal, registered CAS descriptors, v4 trust roots/basis, and reconstructed state. Cached overlap, SQLite, reports, or caller objects are never replay authority.

### 8. Index v5 and report v4

Index v5 observes payloads only through Core's complete authority replay. The
session-bound replay entry has an optional synchronous projection visitor that
is called once per event with private-field, non-Clone, non-Serde
`BorrowedV4EventMetadata<'_>` and a private-constructed closed
`BorrowedProjectionPayloadV4<'_>`. Metadata exposes only the index-required
sequence, ID, fixed schema, event/payload hashes, actor, logical time, and the
exact canonical JSONL byte count including LF. Core computes that count once
when it validates/constructs the V4 envelope, and full-prefix preflight and the
visitor reuse the number; neither callback nor Store may call a canonical byte
producer. Payload variants contain private-field, non-`Clone`, non-Serde
projection wrappers over the already decoded genesis, inherited D2/M4 DTO, v4
registration, or validated M5 bundle while that replay step is live. Those
wrappers expose only scalar borrows and opaque nested item iterators required
by index v5; they expose neither a complete DTO reference nor a canonical
byte/JSON producer. Fixed context-policy arrays are likewise enumerated by
Core-owned opaque iterators, so Store does not duplicate policy constants. The
visitor exposes no `EventEnvelope`, `RawValue`, JSON string, mutable aggregate,
admission, or authority constructor. Core's visitor implementation performs no
second decode, serialization, clone, or retained typed-payload allocation. A
Store scalar adapter may stream the exposed scalar borrows into its
allocation-free measurement/accounting path, including scalar serialization
needed for that measurement; it must not construct an owned DTO or a canonical
JSON/byte buffer from the visitor. Higher-ranked references cannot escape the
callback. All callback observations are
provisional and Store discards them unless the complete replay, deferred CAS
closures, replay basis construction, and session binding return `Ok`. Thus an
early valid event in a later-invalid prefix cannot create an index fact. The
ordinary replay entry delegates to the same implementation with a no-op
visitor, so V1--V3 behavior and V4 authority semantics are unchanged.

For the post-admission index pass, a successful V4 replay retains one bounded
lookup-trace entry per non-genesis event. An entry contains only its closed
payload variant, the minimum stable lookup IDs, and (for an obligation
transition) its lifecycle scalar; it never contains a payload DTO, source
collection, prose, canonical JSON, raw bytes, or authority. Core reserves the
trace to the exact replay event count. The aggregate dynamic ID allocation must
fit within the already bounded canonical payload bytes, plus one fixed inline
trace value per event, and replay refuses success if that derived bound or the
one-to-one envelope/trace cardinality is violated. After replay, Core uses this
trace only to reconstruct higher-ranked borrowed opaque wrappers from accepted
aggregate state; this traversal performs no second payload decode.

Store's synchronous replay callback retains only allocation-free scalar
accounting, confirmed byte offset, and event count. Index v5 first admits the
complete exact phase-0 resource charge. Only after that admission may Store run
the trace traversal, allocate each projected row exactly once, verify the
materialized offset, event count, and every event-derived array length against
phase 0, and move those vectors into the snapshot without cloning or retaining
a second row cache. Recovery-created editable sessions reconstruct the same
scalar charge from the successful replay trace before exposing index v5.

After successful replay, Store may read only the final descriptive state it
cannot retain through the callback: an allocation-free obligation-lifecycle
lookup and a stable borrowed iterator over replayed claim assessments. These
surfaces expose no mutable aggregate, serialization, admission, or authority.

`SCHEMA_V5` is one literal, never `SCHEMA_V4 + patch`. It copies every inherited D2/M4 domain table byte-for-byte except the required event/index metadata contract replacements below, then adds the complete M5 tables. Every `*_canonical_json` is bounded-decoded, requires sorted/unique set order as applicable, canonical-reserializes byte-identically before insert and after query.

```sql
CREATE TABLE index_meta (
  singleton INTEGER PRIMARY KEY CHECK(singleton=1),
  index_schema_version INTEGER NOT NULL CHECK(index_schema_version=5),
  projection_contract_version TEXT NOT NULL CHECK(projection_contract_version='reviewgraphen.index_projection.v5'),
  event_contract_version TEXT NOT NULL CHECK(event_contract_version='reviewgraphen.review_event.v4'),
  projection_mode TEXT NOT NULL CHECK(projection_mode='v4_gluing'),
  run_id TEXT NOT NULL,
  genesis_hash TEXT NOT NULL,
  confirmed_offset INTEGER NOT NULL CHECK(confirmed_offset>=0),
  tail_hash TEXT NOT NULL,
  event_count INTEGER NOT NULL CHECK(event_count>=0),
  policy_revision_hash TEXT NOT NULL,
  authority_replay_basis_digest TEXT NOT NULL
) STRICT;

CREATE TABLE events (
  sequence INTEGER PRIMARY KEY CHECK(sequence>0),
  event_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.review_event.v4'),
  event_hash TEXT NOT NULL,
  payload_hash TEXT NOT NULL,
  payload_kind TEXT NOT NULL CHECK(payload_kind IN (
    'obligation_transition','run_genesis_manifest','artifact_registered',
    'snapshot_sources_recorded','review_plan_recorded','context_envelope_projected',
    'review_execution_recorded','evidence_recorded_v3','evidence_bound_v3',
    'verification_recorded_v3','decision_recorded_v3','finding_recorded_v3',
    'artifact_registered_v4','gluing_bundle_recorded_v4'
  )),
  actor TEXT NOT NULL,
  logical_time INTEGER NOT NULL CHECK(logical_time>=0),
  UNIQUE(sequence,event_id)
) STRICT;

-- `artifact_registrations` is also present, copied byte-for-byte from the
-- complete ADR 0021 v4-index DDL. It contains only inherited
-- ArtifactRegistrationV3 rows and its five old source kinds.
CREATE TABLE artifact_registrations_v4 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0),
  event_id TEXT NOT NULL,
  registration_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.artifact_registration.v4'),
  run_id TEXT NOT NULL,
  cas_hash TEXT NOT NULL,
  media_type TEXT NOT NULL,
  size INTEGER NOT NULL CHECK(size>=0),
  sensitivity TEXT NOT NULL CHECK(sensitivity IN (
    'canonical_state','workspace_source','sensitive'
  )),
  source_kind TEXT NOT NULL CHECK(source_kind IN (
    'run_genesis','snapshot_ingest','reviewer_execution',
    'verifier_artifact','external_harness_witness','gluing_input'
  )),
  source_canonical_json TEXT NOT NULL,
  descriptor_id TEXT NOT NULL UNIQUE,
  body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,registration_id),
  UNIQUE(registration_id,descriptor_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id),
  FOREIGN KEY(descriptor_id)
    REFERENCES gluing_input_descriptors_v4(descriptor_id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

-- source_canonical_json is exactly one closed ArtifactSourceV4 object in §1.
-- For source_kind=gluing_input, core additionally requires the registration
-- event actor engine:reviewgraphen.m5_gluing_input@1 and exact equality of the
-- nested descriptor hash/size/media/sensitivity with the outer columns.

CREATE TABLE gluing_input_descriptors_v4 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0),
  event_id TEXT NOT NULL,
  descriptor_id TEXT NOT NULL UNIQUE,
  registration_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.gluing_input_descriptor.v4'),
  run_id TEXT NOT NULL,
  snapshot_id TEXT NOT NULL,
  universe_id TEXT NOT NULL,
  plan_id TEXT NOT NULL,
  profile_descriptor_id TEXT NOT NULL CHECK(profile_descriptor_id='reviewgraphen.double_submit_gluing@1'),
  context_id TEXT NOT NULL CHECK(context_id IN ('context:payment','context:ui-event')),
  assignment_key TEXT NOT NULL CHECK(assignment_key='caller_duplicate_protection'),
  assignment_value TEXT NOT NULL CHECK(assignment_value IN ('satisfied','required','unknown')),
  qualification_source_ids_canonical_json TEXT NOT NULL,
  descriptor_hash TEXT NOT NULL,
  descriptor_size INTEGER NOT NULL CHECK(descriptor_size>=0),
  body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,descriptor_id),
  UNIQUE(run_id,context_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id),
  FOREIGN KEY(registration_id,descriptor_id)
    REFERENCES artifact_registrations_v4(registration_id,descriptor_id)
    DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE context_covers_v4 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0),
  event_id TEXT NOT NULL,
  cover_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.context_cover.v4'),
  run_id TEXT NOT NULL,
  snapshot_id TEXT NOT NULL,
  universe_id TEXT NOT NULL,
  plan_id TEXT NOT NULL,
  profile_descriptor_id TEXT NOT NULL CHECK(profile_descriptor_id='reviewgraphen.double_submit_gluing@1'),
  selected_obligation_ids_canonical_json TEXT NOT NULL,
  required_context_ids_canonical_json TEXT NOT NULL,
  cover_domain_ids_canonical_json TEXT NOT NULL,
  covered_domain_ids_canonical_json TEXT NOT NULL,
  uncovered_domain_ids_canonical_json TEXT NOT NULL,
  source_ids_canonical_json TEXT NOT NULL,
  body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,cover_id),
  UNIQUE(event_sequence,event_id,cover_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE sections_v4 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0),
  event_id TEXT NOT NULL,
  section_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.section.v4'),
  cover_id TEXT NOT NULL REFERENCES context_covers_v4(cover_id),
  context_id TEXT NOT NULL CHECK(context_id IN ('context:payment','context:ui-event')),
  snapshot_id TEXT NOT NULL,
  property_id TEXT NOT NULL CHECK(property_id='payment.at_most_once'),
  invariant_id TEXT NOT NULL CHECK(invariant_id='invariant:payment-at-most-once'),
  obligation_id TEXT NOT NULL,
  claim_id TEXT NOT NULL REFERENCES claims(claim_id),
  claim_assessment_id TEXT NOT NULL REFERENCES claim_assessments_v3(claim_id),
  input_descriptor_id TEXT NOT NULL REFERENCES gluing_input_descriptors_v4(descriptor_id),
  input_registration_id TEXT NOT NULL REFERENCES artifact_registrations_v4(registration_id),
  assignment_key TEXT NOT NULL CHECK(assignment_key='caller_duplicate_protection'),
  assignment_value TEXT NOT NULL CHECK(assignment_value IN ('satisfied','required','unknown')),
  passed_current_verification INTEGER NOT NULL CHECK(passed_current_verification IN (0,1)),
  source_ids_canonical_json TEXT NOT NULL,
  qualification_source_ids_canonical_json TEXT NOT NULL,
  binding_ids_canonical_json TEXT NOT NULL,
  evidence_ids_canonical_json TEXT NOT NULL,
  verification_ids_canonical_json TEXT NOT NULL,
  decision_ids_canonical_json TEXT NOT NULL,
  finding_ids_canonical_json TEXT NOT NULL,
  body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,section_id),
  UNIQUE(cover_id,context_id),
  UNIQUE(event_sequence,event_id,section_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE gluing_attempts_v4 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0),
  event_id TEXT NOT NULL,
  attempt_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.gluing_attempt.v4'),
  cover_id TEXT NOT NULL REFERENCES context_covers_v4(cover_id),
  snapshot_id TEXT NOT NULL,
  property_id TEXT NOT NULL CHECK(property_id='payment.at_most_once'),
  invariant_id TEXT NOT NULL CHECK(invariant_id='invariant:payment-at-most-once'),
  input_descriptor_ids_canonical_json TEXT NOT NULL,
  section_ids_canonical_json TEXT NOT NULL,
  restriction_ids_canonical_json TEXT NOT NULL,
  result TEXT NOT NULL CHECK(result IN ('failed','unknown','candidate','glued_with_qualification','glued')),
  global_candidate_id TEXT UNIQUE,
  obstruction_id TEXT UNIQUE,
  source_ids_canonical_json TEXT NOT NULL,
  claim_ids_canonical_json TEXT NOT NULL,
  evidence_ids_canonical_json TEXT NOT NULL,
  verification_ids_canonical_json TEXT NOT NULL,
  decision_ids_canonical_json TEXT NOT NULL,
  finding_ids_canonical_json TEXT NOT NULL,
  body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,attempt_id),
  UNIQUE(event_sequence,event_id,attempt_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

CREATE TABLE restrictions_v4 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0),
  event_id TEXT NOT NULL,
  attempt_id TEXT NOT NULL REFERENCES gluing_attempts_v4(attempt_id),
  restriction_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.restriction.v4'),
  section_id TEXT NOT NULL REFERENCES sections_v4(section_id),
  context_pair_canonical_json TEXT NOT NULL,
  overlap_member_ids_canonical_json TEXT NOT NULL,
  assignment_key TEXT NOT NULL CHECK(assignment_key='caller_duplicate_protection'),
  assignment_value TEXT NOT NULL CHECK(assignment_value IN ('satisfied','required','unknown')),
  source_ids_canonical_json TEXT NOT NULL,
  qualification_source_ids_canonical_json TEXT NOT NULL,
  claim_ids_canonical_json TEXT NOT NULL,
  evidence_ids_canonical_json TEXT NOT NULL,
  verification_ids_canonical_json TEXT NOT NULL,
  decision_ids_canonical_json TEXT NOT NULL,
  finding_ids_canonical_json TEXT NOT NULL,
  body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,restriction_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id),
  FOREIGN KEY(event_sequence,event_id,attempt_id)
    REFERENCES gluing_attempts_v4(event_sequence,event_id,attempt_id),
  FOREIGN KEY(event_sequence,event_id,section_id)
    REFERENCES sections_v4(event_sequence,event_id,section_id)
) STRICT;

CREATE TABLE global_candidates_v4 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0),
  event_id TEXT NOT NULL,
  attempt_id TEXT NOT NULL REFERENCES gluing_attempts_v4(attempt_id),
  global_candidate_id TEXT NOT NULL UNIQUE REFERENCES gluing_attempts_v4(global_candidate_id),
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.global_candidate.v4'),
  cover_id TEXT NOT NULL REFERENCES context_covers_v4(cover_id),
  invariant_id TEXT NOT NULL CHECK(invariant_id='invariant:payment-at-most-once'),
  property_id TEXT NOT NULL CHECK(property_id='payment.at_most_once'),
  required_section_ids_canonical_json TEXT NOT NULL,
  restriction_ids_canonical_json TEXT NOT NULL,
  qualification_source_ids_canonical_json TEXT NOT NULL,
  source_ids_canonical_json TEXT NOT NULL,
  claim_ids_canonical_json TEXT NOT NULL,
  evidence_ids_canonical_json TEXT NOT NULL,
  verification_ids_canonical_json TEXT NOT NULL,
  decision_ids_canonical_json TEXT NOT NULL,
  finding_ids_canonical_json TEXT NOT NULL,
  body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,global_candidate_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id),
  FOREIGN KEY(event_sequence,event_id,attempt_id)
    REFERENCES gluing_attempts_v4(event_sequence,event_id,attempt_id),
  FOREIGN KEY(event_sequence,event_id,cover_id)
    REFERENCES context_covers_v4(event_sequence,event_id,cover_id)
) STRICT;

CREATE TABLE gluing_obstructions_v4 (
  event_sequence INTEGER NOT NULL CHECK(event_sequence>0),
  event_id TEXT NOT NULL,
  attempt_id TEXT NOT NULL REFERENCES gluing_attempts_v4(attempt_id),
  obstruction_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK(schema='reviewgraphen.gluing_obstruction.v4'),
  kind TEXT NOT NULL CHECK(kind IN ('required_section_missing','section_unknown','required_overlap_missing','assignment_conflict')),
  conflicting_context_ids_canonical_json TEXT NOT NULL,
  section_ids_canonical_json TEXT NOT NULL,
  overlap_member_ids_canonical_json TEXT NOT NULL,
  assignment_key TEXT NOT NULL CHECK(assignment_key='caller_duplicate_protection'),
  left_assignment_value TEXT CHECK(left_assignment_value IN ('satisfied','required','unknown')),
  right_assignment_value TEXT CHECK(right_assignment_value IN ('satisfied','required','unknown')),
  source_ids_canonical_json TEXT NOT NULL,
  claim_ids_canonical_json TEXT NOT NULL,
  evidence_ids_canonical_json TEXT NOT NULL,
  verification_ids_canonical_json TEXT NOT NULL,
  decision_ids_canonical_json TEXT NOT NULL,
  finding_ids_canonical_json TEXT NOT NULL,
  affected_invariant_id TEXT NOT NULL CHECK(affected_invariant_id='invariant:payment-at-most-once'),
  severity TEXT NOT NULL CHECK(severity IN ('high','critical')),
  required_resolution TEXT NOT NULL CHECK(required_resolution IN (
    'record_required_section','resolve_context_overlap',
    'resolve_unknown_duplicate_protection_assignment',
    'resolve_duplicate_protection_responsibility'
  )),
  human_decision_required INTEGER NOT NULL CHECK(human_decision_required IN (0,1)),
  blocks_canonical_json TEXT NOT NULL,
  body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,obstruction_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;
```

`index_meta.authority_replay_basis_digest` is the session-independent `basis_digest` defined in
§1, recomputed from the same confirmed prefix and complete authority-entry contents during every
v5 rebuild. No `session_identity`, prepared token, lease, admission, or receipt is projected into
v5. Equality of this digest means equality of that deterministic replay-basis projection only;
it grants no append authority and does not make tokens substitutable across sessions.

`IndexSnapshotV5` retains every complete top-level `IndexSnapshotV4` field and adds these independent arrays; it does not hide v4 registrations inside descriptor rows:

```rust
ArtifactRegistrationV4IndexItem {
  event_sequence,
  event_id,
  event_actor,
  registration_id,
  schema,
  run_id,
  cas_hash,
  media_type,
  size,
  sensitivity,
  source_kind,
  source: ArtifactSourceV4,
  descriptor_id,
  body_hash,
}
GluingInputDescriptorV4IndexItem {
  event_sequence,
  event_id,
  descriptor: GluingInputDescriptorV4,
  registration_id,
  descriptor_hash,
  descriptor_size,
  body_hash,
}
ContextCoverV4IndexItem {
  event_sequence,
  event_id,
  cover: ContextCoverV4,
  body_hash,
}
SectionV4IndexItem {
  event_sequence,
  event_id,
  section: SectionV4,
  body_hash,
}
RestrictionV4IndexItem {
  event_sequence,
  event_id,
  attempt_id,
  restriction: RestrictionV4,
  body_hash,
}
GluingAttemptV4IndexItem {
  event_sequence,
  event_id,
  attempt: GluingAttemptV4,
  body_hash,
}
GlobalCandidateV4IndexItem {
  event_sequence,
  event_id,
  attempt_id,
  candidate: GlobalCandidateV4,
  body_hash,
}
GluingObstructionV4IndexItem {
  event_sequence,
  event_id,
  obstruction: GluingObstructionV4,
  body_hash,
}
IndexSnapshotV5 {
  // all exact IndexSnapshotV4 top-level fields, unchanged
  artifact_registrations_v4: Vec<ArtifactRegistrationV4IndexItem>,
  gluing_input_descriptors: Vec<GluingInputDescriptorV4IndexItem>,
  context_covers: Vec<ContextCoverV4IndexItem>,
  sections: Vec<SectionV4IndexItem>,
  restrictions: Vec<RestrictionV4IndexItem>,
  gluing_attempts: Vec<GluingAttemptV4IndexItem>,
  global_candidates: Vec<GlobalCandidateV4IndexItem>,
  gluing_obstructions: Vec<GluingObstructionV4IndexItem>,
}
```

All eight item objects are closed; the seven M5 topology item shapes above are exactly nested as written and no flattened alternative is accepted. Their nested DTO is the complete strict DTO from §2 or §3, and `body_hash` is recomputed from only that nested DTO. The registration index item is closed and complete: its scalar/source fields equal the complete `ArtifactRegistrationV4`, `event_actor` equals its journal envelope, and `body_hash` is recomputed from that DTO only. `registration_id` equals the DTO ID. `descriptor_id` is a required decoded FK equal to `source.descriptor_id` and to exactly one `gluing_input_descriptors_v4.descriptor_id`; that descriptor row's `registration_id` points back to this row. Rebuild inserts each registration/descriptor pair in one transaction with deferred foreign keys enabled; commit fails for a missing, crossed, or nonreciprocal pair. This reciprocal reference is validation, not source authority.

For the admitted M5 profile the independent registration and descriptor arrays each have exactly 0, 1, or 2 items in a legal pre-bundle snapshot and exactly two after the bundle. They are ordered by decoded `source.context_id`/`descriptor.context_id` (`context:payment`, then `context:ui-event`), with registration or descriptor ID only as a tie-breaker that can never be exercised because contexts are unique. Section/Restriction arrays use the same context order; singleton arrays use their sole record. Every other inherited array retains ADR 0021 ordering.

`Qbytes5` is the UTF-8 length of canonical JSON for this entire `IndexSnapshotV5`, using exactly the item keys and nesting above and explicitly including every key/string/value in `artifact_registrations_v4`; `Owned5` recursively charges all eight decoded arrays, their complete nested DTO/source objects, owner/descriptor FKs, and body hashes. `Rows5` includes every `artifact_registrations_v4` SQL row independently from `gluing_input_descriptors_v4`. Omitting, flattening, or nesting away the independent registration array for accounting is a limit error.

Core checks beyond DDL: profile identity; all derived IDs/projected body hashes; fixed run/snapshot/plan/universe closure; descriptor registration/trust binding; exact `D` partition; exact overlap; Section eligibility and trace equality; restriction derivation; compatibility/result table; result/option cardinality; and all source-set formulas. Every bundle-owned cover/Section/restriction/attempt/candidate/obstruction row must have the identical `(event_sequence,event_id)` of its `gluing_bundle_recorded_v4` event; the tuple FKs on restrictions and candidates are mandatory rather than inferred through `attempt_id`. FKs/CHECKs are backstops. `IndexSnapshotV5` projects exactly the legal 0/1/2 pre-bundle descriptor and independent registration prefix. Its six bundle-owned arrays are simultaneously empty before the bundle and atomically complete afterward. It uses §3 context order for descriptors/Sections/Restrictions and singleton order for cover/attempt/candidate/obstruction; nested wire arrays retain the same context order. It never falls back to record-ID sorting for those positional arrays. “No partial M5 projection” means no partial bundle projection; it does not erase a legal descriptor prefix.

`schemas/reviewgraphen.report.v4.schema.json` is Draft 2020-12 and closed at every object. Its top-level keys are v3's; `schema=reviewgraphen.review.report.v4`, `report_version=4`. Metadata retains v3 keys and adds `gluing_profile_descriptor_id` fixed to `reviewgraphen.double_submit_gluing@1`. Scenario retains v3 keys and adds the singleton sorted `context_cover_ids`. Result retains every v3 key and adds required arrays `gluing_input_descriptors`, `context_covers`, `sections`, `gluing_attempts`, `restrictions`, `global_candidates`, and `gluing_obstructions`.

The registration projection and its containing descriptor item have exactly these keys:

```rust
ArtifactRegistrationV4ReportItem {
  schema: "reviewgraphen.artifact_registration.v4.report_item",
  event_sequence,
  event_id,
  event_actor,
  registration: ArtifactRegistrationV4 {
    schema, id, run_id, cas_hash, media_type, size, sensitivity, source,
  },
  body_hash,
}
GluingInputDescriptorV4ReportItem {
  schema: "reviewgraphen.gluing_input_descriptor.v4.report_item",
  descriptor: GluingInputDescriptorV4 {
    schema, id, run_id, snapshot_id, universe_id, plan_id,
    profile_descriptor_id, context_id, assignment_key, assignment_value,
    qualification_source_ids,
  },
  descriptor_body_hash,
  registration_id,
  registration: ArtifactRegistrationV4ReportItem,
}
```

Both objects deny unknown fields. `event_actor` is required and equals the exact journal envelope actor `engine:reviewgraphen.m5_gluing_input@1`; it is not inferred from source kind. `body_hash` is SHA-256 of only the canonical complete nested `ArtifactRegistrationV4`, and `descriptor_body_hash` is SHA-256 of only the canonical complete descriptor. `registration_id` equals `registration.registration.id`; that registration's `source.descriptor_id` equals `descriptor.id`, and its outer/nested CAS tuple equals the canonical descriptor bytes. Event tuple/actor, registration, hashes, and descriptor must equal journal/CAS/index byte-for-byte.

Every bundle item is the complete strict DTO in §3 plus its common `event_sequence`/`event_id` and computed `body_hash`. Each `gluing_input_descriptors` item contains the complete decoded descriptor and its required nested complete `ArtifactRegistrationV4ReportItem`; the nested item supplies the sole registration event tuple/actor and both objects supply their exact computed hashes. It therefore preserves the closed source object, CAS tuple, descriptor ID, and registration ID without widening the inherited v3 `artifact_registrations` array. Coverage is byte-for-byte v3 shape/predicates; M5 adds no numerator. The v3 report-status table is unchanged.

For each closed projection view and each of the seven report-result arrays below, let `R_a` be the exact record IDs in that source array and `V_a` the IDs represented completely in the view. For `gluing_input_descriptors`, `R_a` contains both descriptor and nested registration IDs; the other arrays contribute their item IDs. If `R_a\V_a` is nonempty, the view has exactly one loss for that array; otherwise it has none. The seven `recovery_ref` values form a closed enum of real report-v4 JSON Pointer locations:

| Array | Exact versioned `recovery_ref` |
| --- | --- |
| `gluing_input_descriptors` | `reviewgraphen.review.report.v4#/result/gluing_input_descriptors` |
| `context_covers` | `reviewgraphen.review.report.v4#/result/context_covers` |
| `sections` | `reviewgraphen.review.report.v4#/result/sections` |
| `gluing_attempts` | `reviewgraphen.review.report.v4#/result/gluing_attempts` |
| `restrictions` | `reviewgraphen.review.report.v4#/result/restrictions` |
| `global_candidates` | `reviewgraphen.review.report.v4#/result/global_candidates` |
| `gluing_obstructions` | `reviewgraphen.review.report.v4#/result/gluing_obstructions` |

Every such loss has exactly:

```text
kind                = omitted_m5_gluing_records
reason              = view omits complete records from <exact array name>
source_ids          = sorted(R_a \ V_a)
affected_properties = [payment.at_most_once]
meaningful          = true
recoverable         = true
recovery_ref        = the table entry for a
```

`<exact array name>` is substituted byte-for-byte with the left table cell; it is not an open string. At most seven M5 omission losses exist per view. Other inherited v3 losses remain separately derived. A view never converts an omitted obstruction into a finding, combines arrays under a nonexistent path, or omits IDs from the corresponding exact loss set.

The source-bound v4 generator holds the shared journal lock while obtaining the v5 snapshot; validates confirmed event chain, v4 genesis/tail, inherited M4 and gluing-input CAS/replay basis, and the one complete M5 bundle; recomputes hashes/index equality; and emits that bundle at/before tail. A legal 0/1/2-descriptor pre-bundle prefix is indexable but report generation returns typed `M5BundleIncomplete` without output until exactly two descriptors and the one complete bundle are durable. Any journal/index/CAS/replay/denominator/assignment/overlap/restriction/candidate/obstruction/loss/report mismatch rejects without output.

### 9. Runtime sequence

Core owns strict DTO construction, canonical IDs/hashes, pure cover/overlap/restriction/compatibility derivation, and replay validation. It exposes no arbitrary assignment/prose/raw-event constructor. Store owns journal session/locking/recovery, v5 rebuild/query, and durable append; no mutable aggregate, raw event append, caller index row, or report import. Runtime follows exactly:

```text
open ReplayedV4RunSession and AuthorityReplayBasisV4 over confirmed M4 state
-> validate profile/plan/universe/ProgramSpace closure
-> canonicalize/CAS-put and append two artifact_registered_v4 descriptors through
   TrustedGluingInputAdmissionV4 in context-ID order, advancing the v4 basis each time
-> derive cover/Sections/restrictions/result/candidate-or-obstruction from replayed state
-> mint one sealed tail-bound GluingBundleV4
-> append one gluing_bundle_recorded_v4 event and update the basis only on success
-> reopen/replay before source-bound report generation
```

Prepared admissions/bundles are nonserializable and exact run/genesis/tail/sequence-bound. Each registration success advances the legal durable 0/1/2-descriptor prefix; bundle pre-append failure leaves all bundle-owned arrays empty and report v4 unavailable. `SessionUncertain` forces v4 recovery and never retries the prepared value. No M5 operation runs a verifier/command, reads arbitrary workspace paths, or uses source/reviewer prose as instructions.

### 10. Required tests and reference scenario

Implementation uses deterministic fake/M4 fixture inputs only and includes:

1. Independently constructed equivalent v4 runs produce identical canonical descriptors/bundle/v5 snapshot/source-bound report bytes despite shuffled input.
2. Exact cover-domain/partition, missing ProgramSpace/context/invariant/member/obligation/plan/universe, empty selected set, and every individual §6 collection at exact/+1/overflow reject or retain explicit uncovered IDs exactly.
3. UI/payment member intersection is exactly `function:checkout-submit`; changing labels/attributes/prose/order cannot alter it.
4. Claim summary/assumptions/prompt-injection text never changes assignment; unknown key/value/alias/profile/configuration, CAS/registration/trust-binding mismatch, and duplicate descriptor context are rejected.
5. Both compatible pairs, conflict directions, all unknown combinations, and UI/payment `satisfied`/`required` are covered. The latter emits exactly one conflict, preserves Sections, and creates no duplicate finding/claim/acceptance/coverage transition.
6. Missing Section, missing overlap, unknown descriptor value, candidate, qualified, glued, and conflict outcomes prove the exact result/candidate/obstruction matrix, trace unions, nullability, and non-authority.
7. Mutate every literal ID kind, identity key, event DTO ID/schema/key/enum/set order/duplicate, every computed index/report body hash, descriptor/Section/Restriction context wire order, pair, partition, overlap, assignment, trace/source set, result, candidate, obstruction, resolution, blocks, actor, sequence/tail, and event tuple. Append/replay/index/report all refuse; source-graph traversal is acyclic and never follows the obstruction owner back-reference.
8. Crash before/after each descriptor CAS put and registration and during/after atomic bundle append; test exact orphan adoption and mismatch/ambiguity/order refusal, basis update rules, `SessionUncertain`, keyed attributed v4 recovery, legal 0/1/2 descriptor prefixes, no partial bundle record, and no duplicate prepared append.
9. Rebuild v5 twice equally; v1/v2/v3/v4 images return `RebuildRequired` unchanged.
10. Closed report-v4 schema mutations, every exact `ArtifactRegistrationV4ReportItem`/descriptor nested key and actor/reference mutation, every M5 array closure/order mutation, each of the seven exact versioned recovery refs and omitted ID sets, exact row/byte/working bounds including observed object/event-line terms, tail/index/CAS/replay tampering, pre-bundle `M5BundleIncomplete`, and source-bound equality. The double-submit v4 fixture is generated from actual v4 journal/CAS/index output, never a detached report assertion.
11. Every inherited M4 authority event is accepted at its exact v4 position only with matching v4 trust roots; substituted v3 basis/token/trusted capability/resume authority, predecessor, sequence, CAS bytes, policy, harness, or human grant refuses mint/append/open/recovery. Every successful append advances basis tail/count; authority append entry vectors advance exactly; pre-durable failure, interrupted bundle, and uncertainty do not.
12. `ArtifactRegistrationV4` accepts the exact closed `gluing_input` source/actor/outer tuple and rejects inherited-v3 substitution, every old/new source-kind confusion, nested/outer hash-size-media-sensitivity mismatch, unknown field, wrong actor, and wrong `registration-v4` identity preimage.
13. Each snapshot-ingest, reviewer-raw, verifier-input, verifier-output, and external-witness v3 registration body can append at a v4 position only through its matching sealed bridge role. The full all-pairs source-role substitution matrix refuses. Snapshot tests mutate run/snapshot/adapter, actor, selected CAS tuple, ProgramSpace fact, bundle hash/count/total bytes/order/missing/extra entry, tail, and sequence; success updates basis tail/count only, uncertainty updates nothing. External witness admission is possible only immediately after its exact confirmed receipt.
14. `IndexSnapshotV5.artifact_registrations_v4` is independently present in context order with every exact field/body hash/actor/descriptor FK; the seven topology items use their exact nested shapes. Omission, flattened substitution, nesting-only registration projection, duplicate row, wrong order/FK, crossed reciprocal pair, same-ID/different-event restriction or candidate ownership, or exclusion from `Qbytes5`/`Owned5`/`Rows5` refuses the whole snapshot.
15. Pure `EventLogV4::from_bootstrap_request` and `replay_confirmed_v4_prefix` accept only the exact source closure or supplied canonical envelopes and perform no storage effect; Store-only `EventJournal::publish_new_v4` accepts only Core's opaque validated log and cannot construct or edit event 1. Crash tests cover orphan CAS before event durability, complete event uncertainty followed by inspected exact-key attributed recovery, absent-log `None` hashes, recoverable partial line, malformed complete line, duplicate genesis, retry-before-inspection refusal, closed `GenesisRecoveryV4` branches, and successful committed recovery before basis construction. Every kind/outcome cross-product outside the allowed table cells, every wrong-kind key, and every file/tail/marker mutation between inspection and recovery refuses without mutation. M4 marker tests cover stage0 cleanup to ordinary editable, every strict-interior ADR 0021 prefix to an opaque resume-only holder plus one-shot authority with no public basis, sealed exact-suffix durability, editable session/basis release only after full replay confirmation, uncertainty requiring re-recovery, already-complete cleanup to ordinary editable, exact marker receipt stage/action/hashes, and duplicate/gapped/reordered/overlong/wrong-body/wrong-suffix/malformed refusal with no cleanup.

## Consequences

M5 makes the local-to-global boundary auditable without treating prose as semantics or treating gluing as verification/acceptance. Its narrow profile produces a reproducible UI/payment conflict with sources and a required resolution. The cost is a fresh major event/index/report contract, required to preserve source and authority boundaries.

## Decision acceptance

This ADR remains Accepted as amended 2026-08-11. The amendment closes the previously underspecified Core/Store bootstrap and attributed recovery boundary, resume-only interrupted-bundle state, exact orphan adoption, reciprocal and same-event SQL constraints, v5 item shapes and observed accounting, and the legal pre-bundle prefix/report boundary. Root-lock realization and internal validated-v5 traversal remain implementation seams and do not change the wire, authority, durability, or accounting contracts fixed here. This ADR does not claim M4/M5 code, a v4 schema, v5 index, or source-bound double-submit fixture already exists.
