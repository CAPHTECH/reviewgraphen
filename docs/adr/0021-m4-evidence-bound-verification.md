# ADR 0021: M4 Evidence-Bound Verification and Authority Bridge

- Status: Accepted
- Date: 2026-08-10
- Scope: defines the smallest authority-bearing M4 vertical slice for a fresh
  event-v3 run: immutable D2-claim assessment, deterministic static and
  fixed-test verification, evidence, human decisions, findings, index v4, and
  report v3. It does not implement M5 gluing, M6 change morphisms, a generic
  command runner, a network verifier, a provider adapter, a policy gate, or a
  CLI.

## Context

ADR 0018 intentionally made D2 execution observations authority-free. Its
`ExecutionClaimV2` is immutable and is stored separately from the historical
M1 `ReviewClaim`; the latter alone is currently understood by evidence
bindings, verification, decisions, findings, and their admissions. Index v3
stores authority payloads only as unreconciled shadows and report v2 requires
empty authority arrays and zero verified/accepted coverage. Reusing those
records for a D2 claim would either mutate the execution body that its hash
commits to, or let a legacy claim ID stand in for a D2 claim. Both are invalid.

M4 needs one narrow way to support the reference `issue_present` claim with a
counterexample test witness, preserve an unsupported or inconclusive result,
and obtain a human acceptance with a complete trace. The verifier boundary is
also a security boundary: repository text, reviewer prose, and a requested
evidence string must never become a command.

## Decision

### 1. Frozen contracts and fresh event-v3 runs

`reviewgraphen.review_event.v2`, `reviewgraphen.index_projection.v3`, and
`reviewgraphen.review.report.v2` are frozen. In particular, a v2 execution
claim remains unconditionally `proposed`/`unreviewed`, and a v2 report never
acquires authority fields by inference or a side channel.

M4 uses the new homogeneous stream schema
`reviewgraphen.review_event.v3`. A v3 stream has the same canonical genesis
shape as v2 and preserves byte-for-byte only the legacy D2 planning, context,
execution, and claim field encodings inside its own v3 event envelopes. The v3
artifact-registration identity is an intentional versioned change: its
preimage adds `size` as §5 defines. V3 adds only the payload kinds in §3. A stream is wholly
v1, v2, or v3; readers fail forward on an unknown schema or payload kind.

There is no in-place v2-to-v3 migration. A v2 journal, index v3, and report v2
remain readable/auditable under their original contracts. To obtain M4 state,
the operator starts a new v3 run from the exact ProgramSpace/snapshot/profile
input and re-runs planning, context, review execution, and verification. Raw
v2 reviewer artifacts may be retained as historical artifacts, but never
re-registered as v3 evidence or used for a v3 decision without a new v3
execution and verification. This conservative rerun is the migration path; it
avoids pretending that registrations, admissions, and human authority survive
a different run/genesis/chain position.

### 2. Immutable claim body and derived assessment state

`ExecutionClaimV2` is an immutable reviewer observation. M4 does not alter its
canonical bytes, identity preimage, `disposition`, `review_status`, or body
hash. Instead, the v3 aggregate exposes this derived per-D2-claim state:

```rust
ClaimAssessmentV3 {
  claim_id: StableId,
  disposition: Proposed | Supported | Accepted | Rejected,
  review_status: Unreviewed | HumanReviewed | Accepted | Rejected,
  binding_ids: BTreeSet<StableId>,
  evidence_ids: BTreeSet<StableId>,
  verification_ids: BTreeSet<StableId>,
  decision_ids: BTreeSet<StableId>,
  finding_ids: BTreeSet<StableId>,
  active_decision_id: Option<StableId>,
  current_finding_id: Option<StableId>,
  decision_conflict: bool,
}
```

It is derived only from v3 events and is not a mutable serialization of the
D2 claim. The initial state for every D2 claim is `Proposed`/`Unreviewed` with
empty sets/nulls and `decision_conflict = false`. `candidate_confidence`, structured execution, a passing process
exit, and the number of reviewers never change this state by themselves.

For the MVP one-obligation D2 baseline, all binding, verification, decision,
and finding records must reference a claim from a structured execution with
exactly the same one obligation. The claim's property, targets, sources,
execution, plan, envelope, snapshot, and raw registration closure are first
revalidated through ADR 0018's D2 rules before any M4 record is considered.

State and trace accumulation are derived in event order. Let
`evidence_baseline(c)` ignore every decision and return `Supported` when the
claim has any valid current `Reproduces` binding, otherwise `Proposed`:

```text
Reproduces binding: Proposed|Supported -> Supported
Qualifies binding: no disposition transition
passed verification: records verification; no implicit disposition change
human Accept: evidence_baseline=Supported -> Accepted/Accepted
human Reject: any evidence_baseline -> Rejected/Rejected
human Defer/Exception: disposition=evidence_baseline, status=HumanReviewed
```

Appending another binding or verification that leaves the same state is
valid: its distinct ID is accumulated and remains auditable. It is never
discarded as an idempotent duplicate. Every authority decision seals the
complete then-current claim-local binding/evidence/verification closure. A
later claim-local binding or verification applies this exact invalidation
transition, regardless of the old decision outcome:

```text
disposition       := evidence_baseline(claim)
review_status      := HumanReviewed
active_decision_id := null
decision_conflict  := true
current_finding_id := null when it names the invalidated decision/trace
```

The historical decision and finding IDs remain in their sets. Valid
`Reproduces`/Passed records also remain valid, so conflict removes only current
human sign-off; it does not erase evidence-supported, verified, or
fresh-verified coverage. A later decision is always evaluated against the
complete expanded closure and `evidence_baseline`, not against the historical
`Accepted` or `Rejected` state. On success it becomes
`active_decision_id`, applies the outcome transition above, and sets
`decision_conflict = false`. This permits an exact re-decision, including a
different outcome or actor, while retaining the old decision. Thus
post-decision additional evidence cannot leave a historical acceptance in
current accepted coverage.

All transitions not listed above are typed `IllegalTransition`. M4 rejects
cross-snapshot evidence at construction and replay, so every admitted M4
evidence record is current-snapshot evidence; historical/stale evidence import
and invalidation are deferred to M6. An accepted claim requires, for its one
claimed obligation, at least one `Reproduces` binding, a `Passed`
verification citing that
evidence, and an active human `Accept` decision that cites the claim,
binding evidence, and verification. A rejection needs a human `Reject` and an
explicit citation to its claim; it does not erase evidence, verification,
finding history, or an obstruction. `Exception` never accepts a claim or
finding.

### 3. New v3 event records and exact identities

The only new v3 payload kinds are:

```text
evidence_recorded_v3
evidence_bound_v3
verification_recorded_v3
decision_recorded_v3
finding_recorded_v3
```

The corresponding strict record schema discriminators are exactly
`reviewgraphen.evidence.v3`, `reviewgraphen.evidence_binding.v3`,
`reviewgraphen.verification.v3`, `reviewgraphen.human_decision.v3`, and
`reviewgraphen.finding.v3`. The sealed bundle schema is
`reviewgraphen.verification_bundle.v3`; it is an in-memory validation label,
not a serializable wire object. Unknown/missing discriminators and unknown
fields are rejected.

Their actors are respectively `verifier:<descriptor_id>` for the first three,
`human:<actor_id>` for decisions, and `projection:<descriptor_id>` for
findings. The D2 payload kinds retain their ADR 0018 actor/shape rules. Each
new object has `id = StableId::derived(kind, canonical identity body)`; all
identity bodies below have exactly the listed keys, lexicographic canonical
JSON, UTF-8 bytes, no whitespace, and sorted/unique set arrays.

```rust
EvidenceV3 {
  schema, id, kind, snapshot_id, subject_ids, descriptor_id, procedure_version,
  input_registration_id, output_registration_id, observation
}
// evidence identity: descriptor_id, input_registration_id, kind, observation,
//                    output_registration_id, procedure_version, snapshot_id,
//                    subject_ids

EvidenceBindingV3 { schema, id, claim_id, evidence_id, relation, property_id }
// binding identity: claim_id, evidence_id, property_id, relation

VerificationV3 {
  schema, id, claim_id, descriptor_id, procedure_version, input_registration_id,
  output_registration_id, evidence_ids, outcome, limitations
}
// verification identity: claim_id, descriptor_id, evidence_ids,
//                        input_registration_id, limitations, outcome,
//                        output_registration_id, procedure_version

HumanDecisionV3 {
  schema, id, policy_revision_hash, run_id, universe_id, claim_id, property_id, outcome, actor,
  authority_id, snapshot_id, source_ids, rationale, issued_at, expires_at
}
// decision identity: actor, authority_id, claim_id, expires_at, issued_at,
//                    outcome, policy_revision_hash, property_id, rationale,
//                    run_id, snapshot_id, source_ids, universe_id

FindingV3 {
  schema, id, projection_descriptor_id, claim_id, status, evidence_ids,
  verification_ids, decision_id, supersedes_finding_id
}
// finding identity: claim_id, decision_id, evidence_ids,
//                   projection_descriptor_id, status,
//                   supersedes_finding_id, verification_ids
```

`kind` is exactly `static_fact` or `test_witness`. `observation` is exactly
`witnessed` or `fact_present`; no free-form verdict exists.
`relation` uses the existing wire names but M4 admits exactly `qualifies` and
`reproduces`; all other relation values are typed `Unsupported`. `outcome` is exactly
`passed`, `inconclusive`, or `unsupported`; M4 does not record `failed`,
`not_attempted`, or `expired` as a verifier event. `limitations` is a
sorted/unique nonempty-string set and is an explicit description, never a
reason to promote a result. Finding statuses admitted by M4 are exactly
`unverified_candidate`, `verified_candidate`, `accepted`, and `rejected`.

All IDs must have their stated StableId kind (`evidence`, `binding`,
`verification`, `decision`, `finding`). A repeated ID is `IdCollision`, even
when the body bytes are identical. A body whose supplied ID is not the derived
ID is a typed `Validation` failure. No implementation may accept an arbitrary
caller-chosen ID or infer missing identity fields.

For every record, `body_hash` means SHA-256 of the complete canonical record,
including its exact `schema` and derived `id`; the index stores this computed
value but the event record does not carry a redundant caller-supplied hash.
Identity hash, body hash, event payload hash, event hash, and CAS hash are
distinct preimages and are never substituted for one another.

`EvidenceV3` is independently stored and has no `claim_id`. Its snapshot must
equal the run's ProgramSpace snapshot; importing an observation from any other
snapshot is a typed `SnapshotMismatch`, not a stale record. Each subject is a
known ProgramSpace ID. Its input and output registrations must be prior, unique v3 registrations
in the same run, with verified CAS hash/size/media type and the exact sources
in §5. `EvidenceBindingV3` is the sole claim/evidence edge; its property must
equal the D2 claim property and evidence subjects must intersect the claim
targets or sources. `VerificationV3` must cite only bindings for its exact
claim; `Passed` requires one or more current-snapshot reproducing cited
evidence IDs. `Unsupported` has no evidence IDs and requires the fixed
descriptor's capability rejection output. `Inconclusive` may cite evidence
but cannot produce a finding above `unverified_candidate`.

`rationale` is a nonempty UTF-8 string of at most 8,192 bytes. `issued_at` is
a required canonical RFC3339 UTC timestamp with second precision. It is input
data supplied by the trusted host, not a clock read by core or store. It is
part of the decision body and identity; changing it, the rationale, or the
scope creates a different decision rather than reusing an admission.

### 4. Descriptor registry and deterministic static verifier

M4 adds `reviewgraphen-verifier`. Descriptors are code-owned constants, not
repository configuration and not model output:

```text
reviewgraphen.static_fact_verifier@1
  kinds: [static_fact]
  properties: [payment.at_most_once]
  procedure: reviewgraphen.static_fact.projection@1
  network: none; process: none; workspace_write: false

reviewgraphen.fixture_test_verifier@1
  kinds: [test_witness]
  properties: [payment.at_most_once]
  procedure: reviewgraphen.fixture_test.duplicate_submit@1
  network: none; process: none; workspace_write: false
```

The static verifier accepts only a D2 claim for the exact current property ID
`payment.at_most_once`. Let `o` be the claim's exact one obligation, `T` its
`normalized_target_refs`, `K` its `normalized_context_ids`, and
`S = o.source_ids U claim.source_ids`. The exact applicability closure is
`C = T U K U S`. An invariant is a candidate only when all of these hold:

```text
invariant.property_id = claim.property_id = o.property_id
invariant.scope_ids is a nonempty subset of C
invariant.scope_ids intersects (claim.target_refs U K U claim.source_ids)
```

No relation traversal, substring match, severity, verification-mode hint, or
StableId ordering participates. Core computes the complete candidate ID set
before constructing a result. Cardinality zero is applicability `Absent`;
cardinality one is `Unique`; cardinality greater than one is the normal DTO
applicability value `Ambiguous` with the complete `candidate_invariant_ids`.
It is not an error. The verifier never chooses the first canonical ID.
`Absent` emits no evidence and `Inconclusive` with the
exact limitation `required declared invariant is absent`. `Unique` emits
`EvidenceV3(kind=static_fact, observation=fact_present)`, one `Qualifies`
binding, and `Inconclusive` with the exact limitation
`declared invariant is not an observed violation`. `Ambiguous` emits no
evidence and `Inconclusive` with the exact limitation
`multiple applicable declared invariants`. An unsupported property emits
`Unsupported`, no evidence, and exactly
`unsupported property: reviewgraphen.static_fact_verifier@1 supports only payment.at_most_once`.
A declared invariant remains applicability metadata, not a counterexample.
The static descriptor never emits `Passed`, `Supports`, `Reproduces`, or a
finding, and never turns declaration, ambiguity, or absence into an
`issue_present`/`issue_absent` conclusion.

The fixture-test verifier is the M4 allow-listed test verifier. It is not a
generic test-command facility. It has exactly one compiled-in profile,
`duplicate-submit-payment@1`, and recognizes exactly these 145 canonical bytes
(without LF):

```json
{"charge_count":2,"expected_max":1,"outcome":"witnessed","schema":"reviewgraphen.test_witness_result.v1","test_artifact_id":"test:double-submit"}
```

Their required hash is
`sha256:8d673f965d089dfc08fb3e9c85453f4654894de71e0bd331a9f3cf97bcea5355`
and media type is
`application/vnd.reviewgraphen.test-witness+json;version=1`. The descriptor
compiles in the complete bytes, size, hash, and media type. It also requires
the exact `test:double-submit` artifact ID and the exact
`payment.at_most_once` `issue_present` claim closure. Bytes and a hash alone
cannot mint evidence: the caller must supply the nonserializable exact harness
admission defined below. A valid admission plus these bytes emits one
test-witness evidence record, one `Reproduces` binding, and one `Passed`
verification. M4 has no admitted `not_witnessed` fixture; the static verifier
already represents nonproof as `Inconclusive`.

```rust
ExternalWitnessAdmissionV3 {
  policy_revision_hash,
  harness_id: "reviewgraphen.double_submit_harness@1",
  harness_revision: "1",
  repository_id, repository_source_hash, harness_source_hash,
  run_id, genesis_hash, tail_hash, expected_first_sequence,
  snapshot_id, universe_id, property_id: "payment.at_most_once",
  claim_id, claim_body_hash,
  descriptor_id: "reviewgraphen.fixture_test_verifier@1",
  procedure_version: "reviewgraphen.fixture_test.duplicate_submit@1",
  test_artifact_id: "test:double-submit",
  witness_hash: "sha256:8d673f965d089dfc08fb3e9c85453f4654894de71e0bd331a9f3cf97bcea5355",
  witness_size: 145,
  witness_media_type: "application/vnd.reviewgraphen.test-witness+json;version=1",
  witness_sensitivity: CanonicalState,
}
```

`TrustedFixtureHarnessV1` is a one-shot, nonserializable, non-`Clone`
capability constructed only by a host that actually ran the checked-in
harness. It contains exactly the repository ID, repository source hash, run
ID, genesis hash, snapshot ID, universe ID, exact property ID, claim ID and
claim body hash, harness-policy revision hash, harness ID/revision, harness
source hash, test artifact ID, descriptor ID/procedure version, result hash,
result size, result media type, and result sensitivity `CanonicalState`
shown above. The
descriptor registry compiles the expected harness ID/revision/source hash and
the implementation commit adds the checked-in harness source plus a fixture
that recomputes that exact source hash; a registry/source mismatch is
`HarnessSourceMismatch` before registration.

Only the lock-held
`ReplayedV3RunSession::admit_external_witness(TrustedFixtureHarnessV1,
claim_id, witness_registration_id)` method may consume the capability and mint
the displayed token after the exact witness registration is durable. The
registration source is the closed durable variant
`ExternalHarnessWitness { policy_revision_hash, repository_id,
repository_source_hash, run_id, genesis_hash, snapshot_id, universe_id,
property_id, claim_id, claim_body_hash, harness_id, harness_revision,
harness_source_hash, test_artifact_id, descriptor_id, procedure_version }`;
its registered CAS hash, size, media type, and `CanonicalState` sensitivity must equal
the capability result tuple. The token additionally binds the current
lock-held tail, next append position, universe, property, immutable claim body,
descriptor, and procedure. It is not serializable, clonable across runs,
reconstructable merely from CAS/report/SQLite, or constructible from
reviewer-requested evidence. The consumed capability cannot authorize a
second witness. A wrong token or a token used after any intervening append is
`WitnessAdmissionMismatch` before evidence construction.
The capability policy hash must equal both the current replay basis and the
exact harness binding in `AuthorityTrustRootsV3`; policy mismatch refuses
registration/admission before either can become authority.

Every other profile, executable, argument, working directory, environment,
network request, source path, requested-evidence string, or tool call is
rejected as typed `Unsupported`; it is never shell-escaped, ignored, or run.

This fixture-only verifier is deliberately process-free. A real repository
test runner is not silently substituted. A later ADR may add a different
descriptor only after defining an OS sandbox with read-only workspace,
dedicated writable temp directory, no network, empty secret environment,
fixed argv (no shell), workspace-scoped cwd, CPU/wall/memory/process/output
limits, input/output hashes, and sandbox configuration identity. The M4
descriptor registry has no wildcard, path, executable, argument, environment,
or network escape hatch.

### 5. CAS, bounds, and durable append order

Verifier input and output are strict, `deny_unknown_fields` DTOs. Their schema
and media type pairs are exact:

| DTO schema | Exact media type |
| --- | --- |
| `reviewgraphen.static_fact_input.v1` | `application/vnd.reviewgraphen.static-fact-input+json;version=1` |
| `reviewgraphen.static_fact_result.v1` | `application/vnd.reviewgraphen.static-fact-result+json;version=1` |
| `reviewgraphen.test_witness_result.v1` | `application/vnd.reviewgraphen.test-witness+json;version=1` |
| `reviewgraphen.fixture_test_result.v1` | `application/vnd.reviewgraphen.fixture-test-result+json;version=1` |

`StaticFactInputV1` contains exactly `schema`, `claim_id`, `snapshot_id`,
`property_id`, `obligation_id`, `obligation_target_refs`,
`obligation_context_ids`, `obligation_source_ids`, `claim_source_ids`,
`candidate_invariant_ids`, `selected_invariant_id`, and
`selected_invariant_scope_ids`. The candidate array is the complete §4 set;
the selected ID/scope are nonnull/nonempty only for cardinality one and are
null/empty otherwise. `StaticFactResultV1` contains exactly `schema`,
`claim_id`, `applicability`, `outcome`, `observation`, and `limitations`.
`applicability` is exactly `absent|unique|ambiguous|unsupported_property`, its
outcome is `inconclusive|unsupported`, observation is `fact_present|null`, and
the one-element limitations set is exactly the corresponding string in §4.
`FixtureTestResultV1` contains
exactly `schema`, `claim_id`, `property_id`, `descriptor_id`,
`procedure_version`, `witness_hash`, `subject_ids`, `outcome`; outcome is
exactly `passed`, the hash is the §4 witness hash, and `subject_ids` is the
sorted union of `test:double-submit` and the claim target refs.

Every input/result is canonical JSON in CAS with a prior
`ArtifactRegistered` event. Static input/result and fixture verification result
use sensitivity `CanonicalState` and the closed `VerifierArtifact { run_id,
claim_id, descriptor_id, procedure_version, role: input|output }` source
variant. The fixture witness input instead uses the exact
`ExternalHarnessWitness` source in §4 and sensitivity exactly
`CanonicalState`; neither source variant accepts unknown fields. The witness
is the compiled-in, secret-free canonical counterexample result that must be
replayed and reported as authority, so `Sensitive` and `WorkspaceSource` are
rejected at construction, replay, index, and report validation. For event v3, the registration
identity preimage is exactly `run_id`, `cas_hash`, `media_type`, `size`,
`sensitivity`, and that complete source object; v1/v2 identity remains frozen.
A registration can therefore
be derived before the later verification identity exists; there is no
registration-ID/verification-ID cycle. `VerifierArtifact` and
`ExternalHarnessWitness` are permitted only for their §4 descriptor roles and
are not interchangeable with `ReviewerExecution`, snapshot source, model
output, or an arbitrary external artifact.

Before a bundle can be sealed, CAS bytes, hash, size, media type, source role,
descriptor, procedure, run, and claim must all equal the strict DTO and its
registration. The generated records are also exact: static evidence subjects
are the sorted union of the matched invariant ID and claim targets, its
observation is `fact_present`, its relation is `Qualifies`, and its verification
is `Inconclusive`; fixture evidence subjects equal the fixture result subjects,
its observation is `witnessed`, its relation is `Reproduces`, and its
verification is `Passed`. Verification registration IDs must equal the DTO
registration IDs, and verification evidence IDs must equal the IDs of the
bundle's evidence records. Any extra, missing, substituted, reordered, or
merely hash-shaped value is `VerificationBundleMismatch`.

M4 limits are inclusive actual UTF-8/byte limits:

| Item | Limit |
| --- | ---: |
| evidence subjects | 128 |
| verification evidence IDs | 128 |
| bindings / evidence / verifications / decisions / findings per claim | 64 each |
| decision source IDs | 256 |
| finding evidence IDs / verification IDs | 64 each |
| limitation strings | 32 |
| one limitation string | 2,048 bytes |
| descriptor/procedure/actor/authority string | 256 bytes |
| canonical static input/result object | 65,536 bytes each |
| canonical fixture witness input | exactly 145 bytes |
| canonical fixture verification result | 65,536 bytes |
| canonical evidence/binding/verification/decision/finding body | 65,536 bytes |
| all retained verifier-stage working bytes | 16,777,216 bytes |
| canonical event plus LF | 1,048,576 bytes and store max-event-line, whichever is smaller |

Counts, lengths, capacity reservations, and sum peaks use checked `u64`
arithmetic before allocation. A limit/overflow returns typed
`Incomplete { operation, limit, observed }`, creates no partial record, and
does not downgrade to `Unsupported` or `Inconclusive`.

`ValidatedVerificationBundleV3` is sealed: fields are private, it implements
neither `Serialize`, `Deserialize`, nor `Clone`, and only a healthy lock-held
`ReplayedV3RunSession` may mint it. The public APIs are exactly:

```rust
ReplayedV3RunSession::mint_verification_bundle(
    &mut self,
    cas: &CasReader,
    request: VerificationBundleRequestV3,
    witness: Option<&ExternalWitnessAdmissionV3>,
) -> Result<ValidatedVerificationBundleV3>;

ReplayedV3RunSession::append_verification_bundle(
    &mut self,
    bundle: ValidatedVerificationBundleV3,
    basis: &mut AuthorityReplayBasisV3,
) -> Result<VerificationBundleReceiptV3>;
```

The request names only the exact claim, compiled-in descriptor, and already
durable input/output registrations. Minting rechecks the session root identity,
tail/sequence, aggregate, CAS bytes, DTO equality, and optional witness token;
it stages the complete evidence/binding/verification command sequence on a
cloned core log, mints exact nonserializable `EvidenceAdmissionV3`,
`EvidenceBindingAdmissionV3`, and `VerificationAdmissionV3` for their staged
positions, and seals those tokens, the start position, and every resulting
event hash. No admission or staged command accessor is public.
This position sealing does not originate trust: static output is limited to
non-passing deterministic qualification, fixture Passed requires the exact
external witness authority, and human authority is never accepted by this API.
The sealed value cannot survive an intervening append: append consumes it and
requires the same session identity, tail, and next sequence.

The trusted runtime performs one verification in this order:

1. resolve and revalidate the exact D2 claim and its CAS-backed source closure;
2. select the compiled-in descriptor and preflight all bounds/capabilities;
3. construct the strict canonical input/result through the process-free verifier;
4. put input and output bytes into CAS and append their exact registrations;
5. for a fixture pass, while the same session lock is held, obtain the exact
   external witness admission at this post-registration tail; then mint the
   sealed bundle (static verification passes `None`);
6. consume the bundle through `append_verification_bundle`, which appends
   `EvidenceRecordedV3`, `EvidenceBoundV3`, then `VerificationRecordedV3`,
   omitting the first two only for no-evidence inconclusive/unsupported; and
7. separately mint/append a finding or human decision after re-reading the
   resulting aggregate.

A bundle append commits each event and its in-memory aggregate stage only after
that line is durable. A confirmed mid-bundle failure returns typed
`BundleAppendInterrupted { durable_stage }`; reopen/resume recognizes the exact
prefix and appends only missing staged records. Post-sync uncertainty returns
`SessionUncertain` and exposes no state until reopen/recovery. A crash after
any completed step leaves only that durable prefix. Resume
revalidates every prior registration/event and appends the first missing step;
it never reuses a different output under an existing verification ID, guesses
a registration, or invokes a process. An orphan CAS object is typed
incomplete. A session uncertainty is propagated unchanged and requires reopen
or recovery before reads/retry. Store owns locks, CAS verification, and
durability; runtime owns descriptor invocation and ordering; core owns all
record/transition/admission validation. Store/session binds delegated authority
to a durable position but never originates verifier or human authority.

A confirmed partial fixture bundle is resumable only through a distinct
one-shot, nonserializable `VerificationBundleResumeAuthorityV3`; an
`AuthorityReplayBasisV3` entry is never append authority. While holding the
journal lock, `recover_verification_bundle_resume(&AuthorityTrustRootsV3,
claim_id)` scans the canonical confirmed prefix, revalidates the exact
claim-bound harness tuple from §4 for fixture work (or the compiled static
descriptor plus exact claim closure for static work), and reconstructs:

```rust
VerificationBundleResumeAuthorityV3 {
  policy_revision_hash, repository_id, repository_source_hash,
  run_id, genesis_hash, snapshot_id, universe_id, property_id,
  claim_id, claim_body_hash, harness_id, harness_revision,
  harness_source_hash, test_artifact_id, descriptor_id, procedure_version,
  result_hash, result_size, result_media_type,
  result_sensitivity: CanonicalState, prefix_stage, confirmed_tail_hash,
  expected_next_sequence, remaining: Vec<ExpectedAuthorityEventV3>,
}
ExpectedAuthorityEventV3 { sequence, payload_kind, record_id, body_hash }
```

`remaining` is exactly the suffix of the originally deterministic
input-registration/output-registration/evidence/binding/verification plan.
`resume_verification_bundle(authority, &mut basis)` consumes the authority and
may append only those bodies at those positions; any intervening event, body
change, trust-root change, or gap is `BundleResumeAuthorityMismatch` and
appends nothing. The allowed prefix states are exhaustive:

| Confirmed canonical prefix | Resume result |
| --- | --- |
| claim's raw reviewer registration absent or invalid | refuse `ClaimRawClosureMissing` |
| raw only | suffix begins input registration |
| raw + input registration | suffix begins output registration |
| raw + input + output registrations | suffix begins evidence |
| above + evidence | suffix begins binding |
| above + binding | suffix begins verification |
| above + verification | `AlreadyComplete`; no resume authority |
| any duplicate, reorder, gap, foreign body, or partial line | typed refusal |

Here `raw` is the exact reviewer-output registration sealed by the immutable
D2 claim; input/output are the exact §5 registrations. Static no-evidence
Inconclusive/Unsupported plans omit evidence/binding, so their corresponding
table rows jump directly from output registration to verification. The resume
authority is rebuilt only from the trust-root policy and exact claim-bound
scope that authorized fresh mint (fixture harness binding, or pure static
descriptor binding) and can be consumed once. It neither chooses a
new result nor broadens repository, run, snapshot, universe, property, claim,
harness, test, result, tail, sequence, or body authority.

Fresh mint authority and durable replay validation are separate APIs and
types. `EvidenceAdmissionV3`, `EvidenceBindingAdmissionV3`,
`VerificationAdmissionV3`, `ExternalWitnessAdmissionV3`, and
`DecisionAdmissionV3` authorize only a new next append and are never
serialized or regenerated. Existing durable events are instead revalidated
into this sealed, caller-retained value:

```rust
AuthorityReplayBasisV3 {
  schema: "reviewgraphen.authority_replay_basis.v3",
  run_id, genesis_hash, confirmed_tail_hash, confirmed_event_count,
  policy_revision_hash,
  entries: Vec<AuthorityReplayEntryV3>,
  basis_digest,
}

AuthorityReplayEntryV3 {
  event_sequence, event_id, payload_kind, record_id, record_body_hash,
  predecessor_event_hash, trust_binding_digest,
}
```

The fields are private; the type implements neither `Deserialize` nor a public
constructor. `entries` contains every authority-bearing evidence, binding,
verification, and decision event in sequence order. Its digest is SHA-256 of
the canonical object excluding only `basis_digest`. It contains no secret and
is not itself an authority source: every open/recovery recomputes it from the
canonical confirmed journal prefix, registered CAS bytes, the aggregate just
before each event, and host-supplied `AuthorityTrustRootsV3`.

`AuthorityTrustRootsV3` is a nonserializable host capability containing an
exact `policy_revision_hash`, repository ID/source hash, the allowed harness
bindings `(policy_revision_hash, repository_id,repository_source_hash,
harness_id,harness_revision,harness_source_hash,test_artifact_id,
descriptor_id,procedure_version,result_hash,size,media_type,sensitivity,run_id,genesis_hash,snapshot_id,universe_id,
property_id,claim_id,claim_body_hash)`, and human grants
`(policy_revision_hash,actor,authority_id,capabilities,run_id,snapshot_id,
universe_id,property_ids,claim_ids,valid_from,valid_until)`. The harness policy
authorizes only that one complete tuple: it cannot be applied to a sibling
claim, another claim body at the same ID, another property/universe/test, or a
different result registration. Replay checks static events by
rerunning the pure descriptor over exact registered DTO bytes; fixture events
additionally require an exact durable `ExternalHarnessWitness` registration
and matching harness trust-root tuple; human decisions require an exact grant
whose interval contains `issued_at`. It recomputes every ID, body hash,
predecessor/tail position, claim closure, decision source set, and transition.
It never consults SQLite/report state and never treats a basis supplied by a
caller as sufficient proof.

Fixture fresh, replay, and resume share one internal canonical `AuthorityScopeV3`
preimage with no optional fields:

```text
policy revision, repository ID/source hash, run/genesis, snapshot/universe,
property, claim ID/body hash, harness ID/revision/source hash, test artifact,
result hash/size/media type/sensitivity, descriptor/procedure
```

The fresh capability/admission, durable external-source registration plus its
outer CAS tuple, harness trust-root binding, replay entry
`trust_binding_digest`, and resume authority must all derive this identical
preimage. Tail/sequence/event-body suffixes are additional narrowing for fresh
or resume; they never replace a scope field. Equality is field-for-field and
digest equality is only a redundant check. This is the authority non-widening
invariant tested across all three paths. Human fresh/replay authority likewise
shares the decision body (now including `policy_revision_hash`), exact grant
tuple, claim closure digest, and basis policy revision; human authority has no
resume path.

The exact open/recovery API is:

```rust
EventJournal::replayed_v3_session(
    &AuthorityTrustRootsV3,
) -> Result<(ReplayedV3RunSession, AuthorityReplayBasisV3)>;

EventJournal::recover_replayed_v3_session(
    &AuthorityTrustRootsV3,
) -> Result<(ReplayedV3RunSession, AuthorityReplayBasisV3)>;
```

Both hold the journal lock while scanning the confirmed canonical prefix and
return no session on `AuthorityReplayRefused { event_sequence, reason }`,
`HarnessTrustRootMissing`, `HumanTrustRootMissing`, a CAS mismatch, or an
uncertain prefix. `recover_replayed_v3_session` first performs the existing
canonical tail recovery, then validates exactly the recovered confirmed
prefix. This is deterministic for fixed journal/CAS/trust roots.

Every v3 append API, including registration, bundle, decision, and finding
append, receives `&mut AuthorityReplayBasisV3`. It verifies the basis tail
against the session before doing work. Only an `Ok(receipt)` return updates the
caller-retained basis to the new confirmed tail and adds newly durable
authority entries. `BundleAppendInterrupted` and `SessionUncertain` leave the
caller's basis byte-for-byte unchanged even if a prefix may be durable; the
session becomes non-editable and the next operation must call the recovery API
under the lock to rebuild a replacement basis by canonical prefix scan; a
partial bundle additionally requires the separate consumed resume authority
above before any suffix append. Thus a
crash can lose an in-memory basis but cannot lose or fabricate authority, and
an append token is never reused as a replay token.

### 6. Host-supplied human authority and replay validation

Only a trusted host may construct `TrustedHumanAdmissionV3`:

```rust
TrustedHumanAdmissionV3 {
  policy_revision_hash,
  actor: "human:<nonempty>",
  authority_id: <nonempty, <=256 bytes>,
  capabilities: { accept_finding, reject_finding, defer_finding, record_exception },
  run_id, snapshot_id, universe_id,
  property_ids: { "payment.at_most_once" },
  claim_ids: { exact admitted D2 claim IDs },
  valid_from, valid_until, now
}
```

The three timestamps are canonical RFC3339 UTC with second precision and must
satisfy `valid_from <= now <= valid_until`. This validity authorizes issuance
at the bound `now`; it does not create a time-varying core clock. The token is
nonserializable and exact-run scoped. A decision whose run, snapshot,
universe, property, claim, actor, or authority is outside the token is
`HumanAuthorityScopeMismatch`. `Accept`, `Reject`, `Defer`, and `Exception`
respectively require the four named capabilities. The core mints a nonserializable
`DecisionAdmissionV3` immediately before append. It binds the complete
canonical decision body, run ID, genesis hash, previous tail hash, expected
sequence, universe ID, D2 claim body hash, every current claim-local binding,
evidence, verification, prior decision, and finding body in a closure digest.
It also binds the selected host actor/authority/capability, validity interval,
and `now`; `decision.issued_at` must equal that exact `now`. A token cannot be
used for another decision, claim, body, stream, tail, or append position.
The admission `policy_revision_hash` must equal the current
`AuthorityReplayBasisV3.policy_revision_hash` and the exact human grant in
`AuthorityTrustRootsV3`; mismatch is `AuthorityPolicyMismatch` before mint.

Decision `run_id`, `snapshot_id`, `universe_id`, and `property_id` must equal
the current aggregate and target D2 claim. `expires_at` is
canonical RFC3339 UTC with second precision or explicit `null`; it is required
and non-null for `Exception`, and must be null for `Accept`, `Reject`, and
`Defer`. For an exception, `expires_at` must be strictly later than `issued_at`.
It must also be no later than the authority token's `valid_until`. An exception
is historical/auditable but cannot sign off a claim. No wall clock is read by
core.

`source_ids` is never caller-selected. Core computes and requires these exact
sets from the current assessment:

```text
Accept    = {claim_id}
            U every connected Reproduces binding ID
            U every evidence ID cited by those bindings
            U every Passed verification ID over those evidence IDs
Reject    = {claim_id}
Defer     = {claim_id}
            U every Inconclusive/Unsupported verification ID
            U their cited evidence and binding IDs
Exception = {claim_id}
```

`Accept` additionally refuses an empty `Reproduces`/Passed portion. Extra or omitted
IDs are `DecisionSourceMismatch`. The closure digest still includes all
claim-local records, including records not in the outcome-specific source set.
A later record therefore invalidates the active
decision as §2 specifies rather than being hidden outside its sources.

Fresh append of any v3 authority-bearing payload requires its matching exact
nonserializable evidence, binding, verification, or decision admission. Replay
never recreates that admission and never uses SQLite, a report, CAS metadata
alone, or a human-looking actor string as authority. It uses only the §5
canonical-event/trust-root validator to mint a private replay-validation proof
for that already-durable event and to extend `AuthorityReplayBasisV3`.
Missing, wrong, or position-mismatched fresh admissions reject append;
missing/mismatched trust roots or canonical closure reject replay before an
editable session is returned. A finding has no fresh authority admission, but
fresh append and replay both accept it only after aggregate validation of the
exact claim/evidence/verification/decision closure.

### 7. Finding and reportable state

The sole finding projection descriptor is the compiled-in constant
`reviewgraphen.finding_projection@1`; any other descriptor is `Unsupported`.
`FindingV3` is an explicit projection event, not an inferred severity, and
references one D2 claim. M4 permits it only when that claim has exact
`property_id = payment.at_most_once`, exact `polarity = issue_present`, and the
one-obligation closure from §2; every other claim returns
`FindingNotProjectable` without a finding event. Its trace arrays are exact,
not subsets selected by a renderer:

- `unverified_candidate`: assessment is `Proposed`, or is `Supported` without
  a complete Passed trace,
  `evidence_ids` is every connected evidence ID, `verification_ids` is every
  current non-passed verification ID, and `decision_id = null`.
- `verified_candidate`: assessment is `Supported`, the arrays are the complete
  connected `Reproduces` evidence and Passed verification sets, and
  `decision_id = null`.
- `accepted`: assessment is `Accepted`, the arrays equal the active Accept
  decision's complete `Reproduces`/Passed trace, and `decision_id` is that
  decision.
- `rejected`: assessment is `Rejected`, the arrays equal the active Reject
  decision's exact empty evidence/verification trace, and `decision_id` is
  that decision.

The first finding has `supersedes_finding_id = null`. Every later finding for
the claim must name the immediately preceding finding ID, use a newly derived
ID, and exactly reflect the assessment at its event position. Same-status
replacement is allowed when its evidence or verification set is a strict
superset, or when an exact re-decision changes the nonnull `decision_id` while
the status and required trace sets remain exact. It is otherwise
`RedundantFinding`. All finding events remain history. `current_finding_id` is the last
finding whose exact trace still equals the current assessment. Any later
binding/verification invalidating an active decision also makes its historical
accepted/rejected finding non-current and sets `current_finding_id = null`
until a new exact finding is recorded. No delete or in-place status rewrite is
permitted.

`mint_finding` chooses, rather than accepts from the caller, the strongest
currently satisfied status in this strict priority:
`accepted > rejected > verified_candidate > unverified_candidate`. If none of
the four exact predicates holds it returns `FindingNotProjectable`. The caller
cannot request a weaker status or select a subset trace.

M4 accepts evidence only for the current run snapshot (§3). Consequently,
every admitted verification record is current-snapshot evidence and the report
alias is exactly `fresh_verified == verified`. Cross-snapshot import,
historical reuse, change morphisms, and stale propagation are M6 work and are
rejected—not simulated—by M4. Severity, gate status, gluing, and cross-context
global conclusions are out of scope.

### 8. Index v4 and report v3

M4 adds `reviewgraphen.index_projection.v4` with `PRAGMA user_version = 4`.
V4 is built only from a verified v3 journal and its CAS registrations; index
v3 remains read-only and is rebuilt only as v3. The schema name stored in
`index_meta.projection_contract_version` is exactly
`reviewgraphen.index_projection.v4`; `index_meta.index_schema_version` and
`PRAGMA user_version` are both exactly `4`. V4 does not inherit an ambiguous
v3 table shape. It replaces `index_meta`, `events`, and
`artifact_registrations` with these complete STRICT definitions, then uses the
unchanged complete v3 D2 domain tables (`snapshot_source_index`, `review_plans`,
`context_envelopes`, `executions`, `claims`, and obligation lifecycle tables)
plus the six complete M4 tables below:

The checked-in `SCHEMA_V4` constant is one literal containing every table,
index, CHECK, and FK; implementations may not construct it as
`SCHEMA_V3 + patch` or rely on an installed v3 image. “Unchanged” above means
the legacy D2 table field encodings from ADR 0018 are copied byte-for-byte into
that literal and exercised by the v4 schema fixture, not inherited at runtime;
the v3 registration identity/source columns are the deliberate versioned
replacement shown below.

```sql
CREATE TABLE index_meta (
  singleton INTEGER PRIMARY KEY CHECK (singleton=1),
  index_schema_version INTEGER NOT NULL CHECK (index_schema_version=4),
  projection_contract_version TEXT NOT NULL
    CHECK (projection_contract_version='reviewgraphen.index_projection.v4'),
  event_contract_version TEXT NOT NULL
    CHECK (event_contract_version='reviewgraphen.review_event.v3'),
  projection_mode TEXT NOT NULL CHECK (projection_mode='v3_authority'),
  run_id TEXT NOT NULL,
  genesis_hash TEXT NOT NULL,
  confirmed_offset INTEGER NOT NULL CHECK (confirmed_offset>=0),
  tail_hash TEXT NOT NULL,
  event_count INTEGER NOT NULL CHECK (event_count>=0),
  policy_revision_hash TEXT NOT NULL,
  authority_replay_basis_digest TEXT NOT NULL
) STRICT;

CREATE TABLE events (
  sequence INTEGER PRIMARY KEY CHECK (sequence>0),
  event_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK (schema='reviewgraphen.review_event.v3'),
  event_hash TEXT NOT NULL,
  payload_hash TEXT NOT NULL,
  payload_kind TEXT NOT NULL CHECK (payload_kind IN (
    'obligation_transition', 'run_genesis_manifest', 'artifact_registered',
    'snapshot_sources_recorded', 'review_plan_recorded',
    'context_envelope_projected', 'review_execution_recorded',
    'evidence_recorded_v3', 'evidence_bound_v3',
    'verification_recorded_v3', 'decision_recorded_v3',
    'finding_recorded_v3'
  )),
  actor TEXT NOT NULL,
  logical_time INTEGER NOT NULL CHECK (logical_time>=0),
  UNIQUE(sequence,event_id)
) STRICT;

CREATE TABLE artifact_registrations (
  event_sequence INTEGER NOT NULL CHECK (event_sequence>0),
  event_id TEXT NOT NULL,
  registration_id TEXT NOT NULL UNIQUE,
  run_id TEXT NOT NULL,
  cas_hash TEXT NOT NULL,
  media_type TEXT NOT NULL,
  size INTEGER NOT NULL CHECK (size>=0),
  sensitivity TEXT NOT NULL CHECK (sensitivity IN (
    'canonical_state','workspace_source','sensitive'
  )),
  source_kind TEXT NOT NULL CHECK (source_kind IN (
    'run_genesis','snapshot_ingest','reviewer_execution',
    'verifier_artifact','external_harness_witness'
  )),
  source_canonical_json TEXT NOT NULL,
  body_hash TEXT NOT NULL,
  PRIMARY KEY(event_sequence,registration_id),
  FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;

-- source_canonical_json is exactly one closed tagged object:
-- run_genesis: {kind,run_id}
-- snapshot_ingest: {adapter_id,kind,run_id,snapshot_id}
-- reviewer_execution: {execution_id,kind,reviewer_id,run_id}
-- verifier_artifact:
--   {claim_id,descriptor_id,kind,procedure_version,role,run_id}
-- external_harness_witness:
--   {claim_body_hash,claim_id,descriptor_id,genesis_hash,harness_id,harness_revision,
--    harness_source_hash,kind,policy_revision_hash,procedure_version,property_id,repository_id,
--    repository_source_hash,run_id,snapshot_id,test_artifact_id,universe_id}

CREATE TABLE evidence_v3 (
  event_sequence INTEGER NOT NULL CHECK (event_sequence > 0),
  event_id TEXT NOT NULL,
  evidence_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK (schema = 'reviewgraphen.evidence.v3'),
  kind TEXT NOT NULL CHECK (kind IN ('static_fact','test_witness')),
  snapshot_id TEXT NOT NULL,
  subject_ids_canonical_json TEXT NOT NULL,
  descriptor_id TEXT NOT NULL,
  procedure_version TEXT NOT NULL,
  input_registration_id TEXT NOT NULL,
  output_registration_id TEXT NOT NULL,
  observation TEXT NOT NULL CHECK (observation IN ('fact_present','witnessed')),
  body_hash TEXT NOT NULL,
  PRIMARY KEY (event_sequence, evidence_id),
  FOREIGN KEY (event_sequence,event_id) REFERENCES events(sequence,event_id),
  FOREIGN KEY (input_registration_id) REFERENCES artifact_registrations(registration_id),
  FOREIGN KEY (output_registration_id) REFERENCES artifact_registrations(registration_id),
  CHECK (
    (kind='static_fact' AND observation='fact_present' AND
     descriptor_id='reviewgraphen.static_fact_verifier@1' AND
     procedure_version='reviewgraphen.static_fact.projection@1') OR
    (kind='test_witness' AND observation='witnessed' AND
     descriptor_id='reviewgraphen.fixture_test_verifier@1' AND
     procedure_version='reviewgraphen.fixture_test.duplicate_submit@1')
  )
) STRICT;

CREATE TABLE evidence_bindings_v3 (
  event_sequence INTEGER NOT NULL CHECK (event_sequence > 0),
  event_id TEXT NOT NULL,
  binding_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK (schema = 'reviewgraphen.evidence_binding.v3'),
  claim_id TEXT NOT NULL,
  evidence_id TEXT NOT NULL,
  relation TEXT NOT NULL CHECK (relation IN ('qualifies','reproduces')),
  property_id TEXT NOT NULL CHECK (property_id = 'payment.at_most_once'),
  body_hash TEXT NOT NULL,
  PRIMARY KEY (event_sequence, binding_id),
  FOREIGN KEY (event_sequence,event_id) REFERENCES events(sequence,event_id),
  FOREIGN KEY (claim_id) REFERENCES claims(claim_id),
  FOREIGN KEY (evidence_id) REFERENCES evidence_v3(evidence_id)
) STRICT;

CREATE TABLE verifications_v3 (
  event_sequence INTEGER NOT NULL CHECK (event_sequence > 0),
  event_id TEXT NOT NULL,
  verification_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK (schema = 'reviewgraphen.verification.v3'),
  claim_id TEXT NOT NULL,
  descriptor_id TEXT NOT NULL,
  procedure_version TEXT NOT NULL,
  input_registration_id TEXT NOT NULL,
  output_registration_id TEXT NOT NULL,
  evidence_ids_canonical_json TEXT NOT NULL,
  outcome TEXT NOT NULL CHECK (outcome IN ('passed','inconclusive','unsupported')),
  limitations_canonical_json TEXT NOT NULL,
  body_hash TEXT NOT NULL,
  PRIMARY KEY (event_sequence, verification_id),
  FOREIGN KEY (event_sequence,event_id) REFERENCES events(sequence,event_id),
  FOREIGN KEY (claim_id) REFERENCES claims(claim_id),
  FOREIGN KEY (input_registration_id) REFERENCES artifact_registrations(registration_id),
  FOREIGN KEY (output_registration_id) REFERENCES artifact_registrations(registration_id),
  CHECK (
    (descriptor_id='reviewgraphen.static_fact_verifier@1' AND
     procedure_version='reviewgraphen.static_fact.projection@1' AND
     outcome IN ('inconclusive','unsupported')) OR
    (descriptor_id='reviewgraphen.fixture_test_verifier@1' AND
     procedure_version='reviewgraphen.fixture_test.duplicate_submit@1' AND
     outcome='passed')
  )
) STRICT;

CREATE TABLE decisions_v3 (
  event_sequence INTEGER NOT NULL CHECK (event_sequence > 0),
  event_id TEXT NOT NULL,
  decision_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK (schema = 'reviewgraphen.human_decision.v3'),
  policy_revision_hash TEXT NOT NULL,
  run_id TEXT NOT NULL,
  universe_id TEXT NOT NULL,
  claim_id TEXT NOT NULL,
  property_id TEXT NOT NULL CHECK (property_id = 'payment.at_most_once'),
  outcome TEXT NOT NULL CHECK (outcome IN ('accept','reject','defer','exception')),
  actor TEXT NOT NULL CHECK (actor GLOB 'human:?*'),
  authority_id TEXT NOT NULL,
  snapshot_id TEXT NOT NULL,
  source_ids_canonical_json TEXT NOT NULL,
  rationale TEXT NOT NULL CHECK (length(CAST(rationale AS BLOB)) BETWEEN 1 AND 8192),
  issued_at TEXT NOT NULL,
  expires_at TEXT,
  body_hash TEXT NOT NULL,
  PRIMARY KEY (event_sequence, decision_id),
  FOREIGN KEY (event_sequence,event_id) REFERENCES events(sequence,event_id),
  FOREIGN KEY (claim_id) REFERENCES claims(claim_id),
  CHECK ((outcome='exception' AND expires_at IS NOT NULL) OR
         (outcome!='exception' AND expires_at IS NULL))
) STRICT;

CREATE TABLE findings_v3 (
  event_sequence INTEGER NOT NULL CHECK (event_sequence > 0),
  event_id TEXT NOT NULL,
  finding_id TEXT NOT NULL UNIQUE,
  schema TEXT NOT NULL CHECK (schema = 'reviewgraphen.finding.v3'),
  projection_descriptor_id TEXT NOT NULL
    CHECK (projection_descriptor_id='reviewgraphen.finding_projection@1'),
  claim_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN (
    'unverified_candidate','verified_candidate','accepted','rejected'
  )),
  evidence_ids_canonical_json TEXT NOT NULL,
  verification_ids_canonical_json TEXT NOT NULL,
  decision_id TEXT,
  supersedes_finding_id TEXT,
  body_hash TEXT NOT NULL,
  PRIMARY KEY (event_sequence, finding_id),
  FOREIGN KEY (event_sequence,event_id) REFERENCES events(sequence,event_id),
  FOREIGN KEY (claim_id) REFERENCES claims(claim_id),
  FOREIGN KEY (decision_id) REFERENCES decisions_v3(decision_id),
  FOREIGN KEY (supersedes_finding_id) REFERENCES findings_v3(finding_id),
  CHECK ((status IN ('accepted','rejected') AND decision_id IS NOT NULL) OR
         (status IN ('unverified_candidate','verified_candidate') AND decision_id IS NULL))
) STRICT;

CREATE TABLE claim_assessments_v3 (
  claim_id TEXT PRIMARY KEY,
  disposition TEXT NOT NULL CHECK (disposition IN ('proposed','supported','accepted','rejected')),
  review_status TEXT NOT NULL CHECK (review_status IN ('unreviewed','human_reviewed','accepted','rejected')),
  binding_ids_canonical_json TEXT NOT NULL,
  evidence_ids_canonical_json TEXT NOT NULL,
  verification_ids_canonical_json TEXT NOT NULL,
  decision_ids_canonical_json TEXT NOT NULL,
  finding_ids_canonical_json TEXT NOT NULL,
  active_decision_id TEXT,
  current_finding_id TEXT,
  decision_conflict INTEGER NOT NULL CHECK (decision_conflict IN (0,1)),
  confirmed_event_sequence INTEGER NOT NULL CHECK (confirmed_event_sequence > 0),
  FOREIGN KEY (claim_id) REFERENCES claims(claim_id),
  FOREIGN KEY (active_decision_id) REFERENCES decisions_v3(decision_id),
  FOREIGN KEY (current_finding_id) REFERENCES findings_v3(finding_id)
) STRICT;
```

SQL checks/FKs are backstops. Before insert and after query, core bounded-decodes
every `*_canonical_json`, requires sorted/unique arrays, reserializes, and
requires byte equality; it also validates JSON-contained evidence/source IDs,
timestamps, identity/body hashes, exact finding traces, claim polarity, and
all §2 transitions that SQL cannot express. It likewise bounded-decodes
`source_canonical_json`, requires its `kind` to equal `source_kind`, requires
the exact field set above, and for an external harness source requires the
outer registration hash/size/media tuple and every claim-bound field to equal
the replay trust root and `sensitivity = 'canonical_state'`. `claim_assessments_v3` is rebuilt
only from ordered source events and is never replay input. Index build first
obtains an exact §5 `AuthorityReplayBasisV3`; trust-root refusal produces no
v4 image or partial snapshot. Index v3 images are `RebuildRequired { found: 3,
required: 4 }` and are never altered in place.

`IndexSnapshotV4` contains all complete v3 snapshot fields plus exact ordered
arrays `evidence`, `evidence_bindings`, `verifications`, `decisions`,
`findings`, and `claim_assessments`, and scalar
`policy_revision_hash`/`authority_replay_basis_digest`. Its registration row
replaces `source_id` with `source_kind` plus the decoded closed
`source_canonical_json` union above. Query order is
`(event_sequence, record_id)` for every source array and `claim_id` for
assessments. `snapshot_current_v4(&AuthorityTrustRootsV3)` returns this full
type only after the journal tuple, index markers, replay-basis digest, source
union, body hashes, FKs, canonical JSON, and derived assessment all agree; it
has no authority-free or partial variant.

V4 index accounting is zero-based over the complete `IndexSnapshotV4`; it may
not add an M4 delta to a v3 estimate. `Rows4` is the checked sum of the marker
row and every returned row in `events`, `artifact_registrations`,
`snapshot_source_index`, ProgramSpace source rows, universe rows, obligation/
lifecycle rows, plans/waves, context envelopes/views/losses, executions,
claims, legacy unreconciled-authority shadows, legacy projected findings,
`evidence_v3`, `evidence_bindings_v3`, `verifications_v3`, `decisions_v3`,
`findings_v3`, and `claim_assessments_v3`. Empty legacy shadow arrays still
participate as exact empty fields in the snapshot.

The normative symbols are:

```text
Icells4 = count of every nonnull INTEGER cell read for all Rows4
Tbytes4 = sum UTF-8 lengths of every nonnull TEXT cell read for all Rows4
SQL4    = Tbytes4 + (8 * Icells4) + Rows4
Qbytes4 = UTF-8 length of canonical JSON for the complete IndexSnapshotV4
Owned4  = recursive_ownership_charge(the complete decoded IndexSnapshotV4)
Working4= SQL4 + Qbytes4 + Owned4 + max_verified_CAS_object_bytes
                    + max_canonical_event_line_bytes
```

`recursive_ownership_charge` visits every field of the marker, events,
registrations and decoded source union, ProgramSpace/snapshot/universe/
obligation, plan/context/execution/claim, shadow/finding, all six M4 arrays,
and assessments. It uses actual UTF-8 bytes for strings and object keys,
8 bytes per list slot, 16 bytes per object entry, 8 bytes per integer/float,
1 byte per Boolean, and 0 for null, recursively with checked `u64`. Thus source
objects, canonical sets, projection views, and information-loss records cannot
disappear from working accounting merely because they originated in a
replaced v3 table.

`Rows4`, `Qbytes4`, and `Working4` are checked respectively against
`max_index_rows`, `max_index_query_bytes`, and `max_index_working_bytes` before
reserve/allocation/return. Every term, multiplication, and addition is checked;
exact and +1 fixtures populate every listed component and mutate every field
class. SQLite page size, compressed bytes, a prior-version delta, and allocator
estimates are forbidden substitutes. In report §8 formulas, `I4` means exactly
`Qbytes4`, the canonical byte length of the complete index snapshot;
`Icells4` is the integer-cell count and the two symbols are never aliased.

`reviewgraphen.review.report.v3` is a source-bound projection of a confirmed
v3 journal, CAS objects, and index v4. It includes complete D2 execution and
claim bodies unchanged, full M4 evidence/binding/verification/decision/finding
bodies, the derived assessment, descriptor/procedure IDs, registration source
tuples, tail markers, and separate counts for selected, visited, completed,
evidence-supported, verified, fresh-verified, and accepted. Every numerator
is an explicit StableId set checked against the recorded universe denominator;
none is inferred from comment count, confidence, or a report field. Its JSON
Schema file is `schemas/reviewgraphen.report.v3.schema.json`, its top-level
`schema` value is exactly `reviewgraphen.review.report.v3`, and its checked-in
example is `schemas/reviewgraphen.report.v3.example.json`.

That schema is normative, Draft 2020-12, and closed with
`additionalProperties: false` on every object. Its exact top-level required
keys are the v2 names `schema`, `report_type`, `report_version`, `metadata`,
`scenario`, `result`, `coverage`, and `projection`; `report_type = review` and
`report_version = 3`. `metadata` requires all v2 metadata keys plus
`authority_policy_revision_hash` and `authority_replay_basis_digest`.
`result` requires exactly `status`, `artifact_registrations`, `executions`,
`claims`, `evidence`, `evidence_bindings`, `verifications`, `decisions`,
`findings`, `claim_assessments`, and `obstructions`. `coverage` requires
exactly `universe_id`, `denominator_obligation_ids`,
`visited_obligation_ids`, `completed_obligation_ids`,
`evidence_supported_obligation_ids`, `verified_obligation_ids`,
`fresh_verified_obligation_ids`, `accepted_obligation_ids`, and the seven
matching integer counts `selected`, `visited`, `completed`,
`evidence_supported`, `verified`, `fresh_verified`, and `accepted` (with
`selected` shared by its denominator). `scenario` and `projection` retain
their complete closed v2 shapes. Each new result array item is the complete
strict §3 record or assessment shape, with the same enum, timestamp, ID-set,
cardinality, and byte limits; no summary-only authority item is permitted.

Report v3 defines a new closed `artifactRegistrationV3`; it does not widen or
reuse frozen report-v2's registration definition. Its exact required keys are
`event_sequence`, `event_id`, `registration_id`, `run_id`, `cas_hash`,
`media_type`, `size`, `sensitivity`, `source`, and `body_hash`. `source` is a
`oneOf` with `additionalProperties:false` for exactly the five tagged objects
listed in the v4 DDL comment: `run_genesis`, `snapshot_ingest`,
`reviewer_execution`, `verifier_artifact`, and
`external_harness_witness`. The external member requires every claim-bound
field including policy/repository/run/genesis/snapshot/universe/property/
claim/body/harness/test IDs and hashes; its enclosing registration supplies
and must match sensitivity `canonical_state` plus the trusted result CAS hash,
size, and media type. The local
schema rejects a source-kind/field mismatch; the source-bound validator also
requires exact equality to the journal registration and replay trust root.

The implementation commit must add that schema, its valid example, local
validator mutations for every missing/extra/wrong discriminator and bound,
the source-bound validator, the complete v4 DDL above, and v4 index query
types together. None of report v3, index v4, or event v3 may be exposed when
one of those artifacts is absent. Report v3 extends, and does not replace with
a scalar estimate, ADR 0018's ownership model. Its row and byte preimages are:

```text
report_rows_v3 = registrations + executions + claims + evidence
               + evidence_bindings + verifications + decisions + findings
               + claim_assessments + obstructions + projection_views
               + sum(per_view_information_loss)
O3 = UTF-8 length of the complete canonical report JSON (no LF)
```

Let `J` be the checked sum of every confirmed canonical event plus one LF,
`I4` the canonical byte length of the complete `IndexSnapshotV4`, `Rr3` the
preflight reservation for every report-owned vector/string/scalar capacity,
`R3` the realized report ownership, `S3` the largest canonical byte length of
any complete registration/execution/claim/evidence/binding/verification/
decision/finding/assessment/obstruction/view record, and `O3` the output above.
`Rr3` and `R3` use ADR 0018's exact recursive charge: actual UTF-8 bytes for
strings and keys, 8 bytes per list slot, 16 bytes per object entry, 8 bytes per
integer/float, 1 byte per Boolean, and 0 for null. The only permitted peaks are:

```text
projection_peak_v3    = J + I4 + Rr3
serialization_peak_v3 = J + I4 + R3 + S3 + O3
```

All row terms, capacities, UTF-8 lengths, ownership terms, and additions use
checked `u64` before reserve/allocation; changed lifetimes add and never
subtract simultaneously live buffers. Exact-limit and +1 tests construct real
bounded Rust values for every term, plus overflow at each addition, and prove
refusal before allocation/output. The schema validator validates exact `O3`;
source validation recomputes it from confirmed journal/index/CAS/replay basis
and requires byte equality. Python/container capacity and `canonical*4` are
not normative substitutes.

For the selected obligation set `S`, every report numerator is an exact
obligation-ID set with these predicates at the confirmed tail:

```text
visited = { o in S | at least one D2 execution addresses o }
completed = { o in S | a structured execution addresses o and lifecycle(o)=Completed }
evidence_supported = {
  o in S | exists D2 claim c addressing o and a connected
           Reproduces binding to current-snapshot EvidenceV3
}
verified = {
  o in S | exists D2 claim c addressing o and a Passed VerificationV3 whose
           entire nonempty evidence set has connected Reproduces
           bindings for c and o
}
fresh_verified = verified
accepted = {
  o in S | exists current FindingV3(status=accepted) for a claim addressing o,
           with its exact active Accept decision and complete verified trace
}
```

Each numeric field equals the corresponding set cardinality, and every set is
a subset of `S`, which is itself checked against the recorded universe and
plan. A claim spanning more than one obligation is outside the M4 one-
obligation baseline and rejected, so no verification can be counted for an
untraced sibling obligation. `Unsupported`, `Inconclusive`, historical
decision/finding rows contribute to no predicate they do not satisfy.
`decision_conflict = true` excludes only `accepted`; valid current-snapshot
`Reproduces`/Passed traces continue to count in `evidence_supported`,
`verified`, and `fresh_verified` exactly as defined above.

Report v3 must reject a missing, cross-snapshot, cross-run, unregistered, or
CAS-hash/size-mismatched authority trace. It may show `unsupported` and
`inconclusive` as outcomes, but neither becomes verified/accepted. It reports
an accepted finding only when the exact chain in §2 exists. Report v2 is never
upcast, patched, or used as a report-v3 source. The report schema, local
validator, source-bound validator, and index snapshot version change together
in the M4 report implementation commit.

### 9. Public API boundaries

`reviewgraphen-core` owns strict DTO constructors/decoders, identity/hash
recomputation, immutable-D2 assessment, state transitions, closure checks,
and opaque admissions. Constructors for the five authority payload commands
are crate-private and usable only by sealed bundle/decision/finding values;
there is no public raw `EventCommand` constructor for them.

`reviewgraphen-store` owns CAS, v3 journal locking/recovery, and index v4.
It exposes only the §5 trust-root-bound open/recovery APIs and sealed
verification methods. Human writes use
`mint_decision(&TrustedHumanAdmissionV3, DecisionInputV3)` followed by
`append_decision(ValidatedDecisionV3, &mut AuthorityReplayBasisV3)`; finding writes use
`mint_finding("reviewgraphen.finding_projection@1", claim_id)` followed by
`append_finding(ValidatedFindingV3, &mut AuthorityReplayBasisV3)`.
Registration append and `append_verification_bundle` likewise take the same
mutable basis and obey §5's success/uncertainty rule. Fresh validated values
are session/tail bound, nonserializable, and consumed on append; replay proofs
are private and can validate only an existing canonical event. Store never
exposes a mutable aggregate, raw authority command/envelope append, path-based
CAS bypass, admission constructor, or replay-proof constructor.

`reviewgraphen-verifier` owns only descriptor selection and pure result
construction. It cannot append events, access arbitrary paths, mint
admissions, or execute a process. `reviewgraphen-runtime` receives an already
opened session plus explicit fresh host capabilities and the current replay
basis, performs the ordered CAS/event protocol, and propagates typed failures.
It cannot turn verifier output into a
decision or open an unchecked session. CLI, generic policies, and a real
sandboxed process adapter are deferred.

### 10. Required refusal cases and test matrix

Every M4 implementation change includes fixture/schema/unit/replay tests for:

1. A real v3 D2 `issue_present` -> exact repository/run/snapshot/source-bound
   consumed `TrustedFixtureHarnessV1` -> exact 145-byte witness and admission ->
   reproduces binding -> passed verification -> scoped trusted human accept ->
   accepted finding chain, with canonical-byte and replay-basis determinism
   across two independently constructed equivalent v3 runs.
2. Static applicability candidate cardinality zero/one/multiple yields exact
   Absent/Unique/Ambiguous results and limitation strings; wrong property
   yields the exact Unsupported string. Candidate selection exercises exact
   obligation target/context/source closure and proves canonical ID order never
   selects one of multiple invariants. Static never creates Passed, Supports,
   verified, or accepted state.
3. Confidence at every value, structured execution, repeated reviewer output,
   and a second LLM-like artifact cannot create evidence, verification, or
   acceptance.
4. Missing/dangling/cross-run/cross-snapshot claim, source, subject, evidence,
   registration, descriptor, plan, envelope, or raw-CAS closure; duplicate or
   non-derived IDs; unknown schema/fields; wrong media type/DTO/registration
   role; noncanonical order; exact witness byte/hash/size mismatch; and every
   bound exact, plus-one, and arithmetic-overflow refusal.
5. Every illegal assessment/finding transition; passed-without-evidence,
   `Reproduces` binding with wrong property/target,
   cross-snapshot evidence construction/replay, same-state evidence
   accumulation, the full post-decision invalidation tuple, verified coverage
   retention during conflict, exact re-decision, incomplete decision source
   closure, same-status finding replacement by trace growth or changed active
   decision ID, forbidden redundant replacement, non-`issue_present`/wrong-
   property finding refusal, and exact finding supersession/currentness.
6. Fresh-admission and replay attacks: wrong actor/capability/validity/now, out-of-scope
   run/snapshot/universe/property/claim, altered rationale or outcome-specific
   source set, altered D2 claim body, wrong genesis/tail/sequence, omitted
   witness/evidence/binding/verification/decision fresh admission, wrong
   repository/harness revision/source/result binding, reused consumed harness
   capability, human or harness policy-revision mismatch, missing/altered
   claim-bound trust root, altered canonical prefix or replay
   basis, and a report or index snapshot substituted for authority.
7. Allow-list attacks: arbitrary profile/executable/argv/cwd/env/network/
   source path/requested-evidence text, shell metacharacters, write request,
   oversized input/output, and fixture-test non-witness. All refuse or return
   typed unsupported/inconclusive without process execution or partial state.
8. CAS/register/evidence/binding/verification/decision/finding crash seams,
   sealed-bundle wrong-session/tail refusal, mid-bundle durable-stage prefix,
   caller basis unchanged on interrupted/uncertain return, lock-held canonical
   recovery to an identical replacement basis, every raw/input/output/evidence/
   binding/verification resume-table row, suffix sequence/body mutation,
   one-shot resume consumption, orphan CAS detection, duplicate resume refusal,
   and `SessionUncertain` propagation.
9. The normative STRICT v4 DDL creates every column/check/FK; delete/rebuild/
   query-state equality uses trust-root replay; every `*_canonical_json` byte
   mutation and zero-based exact `Rows4/Icells4/Tbytes4/SQL4/Qbytes4/Owned4/
   Working4` limit/overflow case refuses; v2/v3
   index/report remain byte/schema-compatible and cannot be reinterpreted as
   v4/report-v3.
10. Report v3 closed schema, required/extra-field mutations, deterministic
    canonical bytes and exact row/`J/I4/Rr3/R3/S3/O3` peak accounting,
    authority trace tamper matrix,
    every exact obligation numerator predicate, conflict retaining verified
    but excluding accepted, unsupported/inconclusive and current-versus-
    historical decision/finding rows, and rejection of inferred severity,
    acceptance, freshness, or coverage.

## Consequences

M4 gains an auditable, deterministic evidence path without redefining model
claims as facts or granting a repository command capability. It intentionally
requires a fresh v3 run and exact host trust roots; short-lived append
admissions are not durable, and canonical replay reconstructs only a sealed
basis. This makes the authority boundary explicit. Generic test execution, gluing, incremental
staleness, and policy gates remain separate future decisions rather than
implicit behavior of a verifier result.
