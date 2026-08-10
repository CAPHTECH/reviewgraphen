# ADR 0013: M3 Plan, Context, and Reviewer Contract

- Status: Accepted
- Date: 2026-08-09
- Scope: the M3 vertical slice (`docs/18_mvp_roadmap.md` §4 M3; `docs/19_implementation_backlog.md`
  RG-500/RG-600). Adds to `crates/reviewgraphen-core`: `ReviewContextEnvelope`, `EnvelopeLoss`,
  `SourceArtifactRef`, `ReviewPlan`/`ScheduleWave`/`PlanBudget`/`RiskDescriptor`, `ExecutionRecord`,
  `ExecutionOutcome`, `AbstentionReason`, three additive `PersistedPayload` event variants
  (`ReviewPlanRecorded`, `ContextEnvelopeProjected`, `ReviewExecutionRecorded`), the `reviewgraphen.
  review_event.v2` event-stream contract, and the `ReviewAggregate` validation that closes the
  existing unchecked `ReviewClaim::execution_id` reference. Introduces a new crate,
  `reviewgraphen-reviewer`, owning the `Reviewer` trait, the deterministic fake reviewer, the
  structured reviewer-output parser, and prompt-injection boundary checks. Corrects
  `docs/11_coverage_and_scheduling.md` §3.4. Does not touch `reviewgraphen-ingest`'s Git/Cargo
  subprocess boundary, or the M1 evidence/verification/decision/finding contract beyond the schema
  version bump this ADR and ADR 0014 jointly require. ADR 0014 is a companion ADR: it owns the
  content-addressed store this ADR's Envelope builder and execution/plan records depend on for
  durable source bytes, and it defines the three additional `PersistedPayload` variants
  (`RunGenesisManifest`, `ArtifactRegistered`, `SnapshotSourcesRecorded`) that anchor/register that
  content. The two ADRs share
  one event-schema migration (§2) and must not be read independently of each other for that reason.

> Execution-contract note (2026-08-10):
> [ADR 0018](0018-d2-execution-claim-report-and-index-v3.md) is the accepted
> design closure for this ADR's reserved v2 execution payload. It
> supplies the missing system-prompt/tool-call trace, full D2 claim DTO,
> bounds, canonical identities, CAS ordering, report v2, and index schema v3.
> ADR 0018's explicitly listed superseded text governs those execution
> details; its implementation Definition of Done remains separate.

## Context

M1 (`crates/reviewgraphen-core`) already implements a deterministic `ReviewAggregate` over
`ProgramSpace` + `UniverseDescriptor` + `Obligation`, with a closed, append-only `PersistedPayload`
event vocabulary: `ObligationTransition`, `ClaimProposed`, `EvidenceRecorded`, `EvidenceBound`,
`VerificationRecorded`, `DecisionRecorded`, `FindingRecorded` (`crates/reviewgraphen-core/src/
event.rs`). `ObligationLifecycle` already declares `Generated -> Planned -> InProgress -> Completed
-> {Stale, Superseded}` transitions (`crates/reviewgraphen-core/src/review.rs`), and the generic
`EventLog::obligation_transition` constructor can already drive any of them — M1 just never had a
caller that needed `Planned`, `InProgress`, or `Completed` in practice, because nothing in M1
executes obligations. `Coverage::from_aggregate` (`crates/reviewgraphen-core/src/coverage.rs`)
already defines its `raw` ("completed") measure as exactly the set of obligations whose lifecycle is
`Completed` — a narrower, already-correct definition than the one written in prose in
`docs/11_coverage_and_scheduling.md`.

Four concrete gaps block M3:

1. **No canonical context-projection record.** `docs/04_highergraphen_mapping.md`/ADR 0004 define
   `ReviewContextEnvelope` and require every execution to reference exactly one Envelope version
   with a declared information-loss set, but no such type exists in `reviewgraphen-core` today.
2. **No canonical execution record, and an unchecked reference.** `ReviewClaim::execution_id`
   (`review.rs:510`) is stored on **every** claim (`propose_ai` and `propose`, `review.rs:536-585`)
   but `ReviewAggregate::validate` never resolves it against anything — M1 has no execution record
   for it to resolve against. A claim's `execution_id` can today be any syntactically valid
   `StableId` with no backing execution ever having occurred. `docs/15_storage_and_repository_
   layout.md` §7 already documents this as a known M1 placeholder ("M1 report execution
   references"): reports currently synthesize `unknown`/`unresolved` sentinels and an
   `execution_metadata_unavailable` obstruction specifically because no execution is ever observed.
   M3 is what makes an execution real, and it must close this reference for every claim, not only
   AI-authored ones.
3. **No canonical scheduling record.** `docs/11` §6-9 already describe risk, priority, and
   scheduling modes in prose, but nothing derives a deterministic, auditable order in which
   obligations were actually handed to a reviewer, nor records the policy/budget that produced it.
4. **A direct doc contradiction on what "completed" means.** `docs/09_review_execution_and_agent_protocol.md`
   §9 states plainly: "abstentionはcoverageを`visited`まで進めても`completed`や`verified`へ進めません"
   (an abstention advances coverage to `visited` but never to `completed` or `verified`), and its
   §16 Execution invariant 5 repeats "abstentionをerrorやno-issueへ変換しない." `docs/07_review_obligation_model.md`
   §14 agrees: "obligationの`completed`は、reviewerが構造化結果を返したことを意味します" (only a
   structured result). But `docs/11_coverage_and_scheduling.md` §3.4 currently reads: "Completion
   coverage: structured resultまたはvalid abstentionを得たobligation" — explicitly counting a valid
   abstention as `completed`. `docs/11` §12's own worked example is already consistent with the
   *safe* (docs/09) reading — its `abstained: 22` count sits under `unresolved`, disjoint from
   `completed: 398` — so only the §3.4 prose sentence is the actual defect, not the rest of the
   document or the code. `AGENTS.md`'s boundary list ("`reviewed`、`evidence_supported`、`verified`、
   `human_accepted`を同義にしない") and its coding-review-order item 3 ("accepted/inferred/verified
   の状態遷移") make this a boundary-safety question, not a wording nit: M3 is the milestone that
   makes abstention a real, frequent execution outcome, so this contradiction must be resolved
   *before* implementation, in the safe direction, not discovered after a scheduler or CI gate has
   already been built against the permissive reading.

`docs/18` M3 deliverables are: `ReviewContextEnvelope` builder, source selection policy, projection
loss declaration, fake reviewer, provider-neutral LLM reviewer adapter, structured claim parser,
abstention/malformed output handling. Its exit criteria require: an Envelope carrying obligation,
source IDs, included/excluded, unknowns, and loss; raw model prose never stored as canonical state;
malformed response producing a typed failure without fabricating a claim; the same Envelope being
reproducible/auditable; and repository-embedded prompt injection never being executed as an
instruction. `docs/19` RG-500/RG-600 provide the concrete backlog items this ADR accepts, defers,
or narrows.

## Decision

### 1. M3 scope boundary

In scope (implemented under this ADR):

| RG item | Deliverable | Priority |
| --- | --- | --- |
| RG-501 | Risk descriptor | P0 |
| RG-502 | Baseline (unweighted) deterministic scheduler | P0 |
| RG-504 | `ReviewContextEnvelope` builder | P0 |
| RG-505 | Information loss declaration | P0 |
| RG-601 | `Reviewer` trait | P0 |
| RG-602 | Deterministic fake reviewer | P0 |
| RG-605 | Claim parser and validator | P0 |
| RG-606 | Abstention taxonomy | P0 |
| RG-608 | Prompt-injection boundary tests | P1, pulled forward |

RG-608 is pulled forward from P1 to M3 because a reviewer contract that cannot demonstrate its
prompt-injection boundary is not a safe contract to hand a real provider adapter later; deferring
it would let M3's own exit criterion ("repository内のprompt injectionをinstructionとして実行しない")
go untested. Explicitly deferred, each to a dedicated follow-on ADR when it is implemented:

| RG item | Deliverable | Reason deferred |
| --- | --- | --- |
| RG-503 | Weighted coverage scheduler | Needs calibration data M3 cannot yet produce; RG-502's deterministic ordering, plus this ADR's baseline `RiskDescriptor` (§5a), is sufficient to exercise the rest of the M3 contract. |
| RG-506 | Context size estimator | Token/byte estimation is provider-specific; no real provider adapter exists yet to calibrate against. |
| RG-507 | Envelope materiality comparison | Depends on incremental review (M6) semantics not yet defined. |
| RG-603 | Command/process reviewer adapter | Needs its own sandboxing/tool-policy ADR (`docs/16` §4-§5). |
| RG-604 | One LLM provider adapter | Needs its own data-policy ADR (`docs/16` §6) and cannot be provider-neutral by construction. |
| RG-607 | Parallel execution coordinator | Needs the stable-commit-order store contract ADR 0014 defines; sequencing single-threaded execution first isolates the contract from concurrency bugs. |

The deterministic fake reviewer (RG-602) is the only reviewer M3 wires end-to-end; it is exercised
by every test in this ADR's negative-test list.

### 2. `reviewgraphen.review_event.v2`: the M3 event-stream contract

`EVENT_SCHEMA` moves from `"reviewgraphen.review_event.v1"` to `"reviewgraphen.review_event.v2"`.
This supersedes this ADR's earlier draft framing ("vocabulary widens under an unchanged v1 tag") —
that framing is wrong for what this ADR actually requires, and is retracted, not kept as an
alternative: v1 never enforced execution closure (Context item 2), so a stream tagged v1 cannot
honestly claim to satisfy this ADR's Invariant 3 (§7) merely because its payload vocabulary grew.
The schema tag itself must say which contract a stream was produced and validated under.

Core introduces `EventContractVersion::{V1, V2}`. `EventLog` stores it explicitly; the normal
constructor is v2-only, while import/replay derives it from a non-empty homogeneous envelope
sequence (an empty replay must receive the version explicitly). `event_id`, `envelope_hash`, and
their validators take the selected schema string as an input instead of reading one global
`EVENT_SCHEMA` constant. Thus legacy v1 hashes are recomputed with the exact old v1 string and are
not invalidated by the new default. V1 retains its existing ReviewAggregate serialization hash;
`EventLog::new_v2` instead builds ADR 0014's canonical `RunGenesisSnapshot`, verifies it reconstructs
the supplied pristine aggregate, and uses that snapshot's SHA-256/CAS hash as `genesis_hash`. A v2 append requires sequence 1 to be the sole
`RunGenesisManifest`; v1 import rejects every v2-only payload.

- **v2 is what any run built under this ADR mints.** `EventLog::new` always produces a v2 stream;
  there is no code path that mints a new v1 event after this ADR ships.
- **v2 requires execution closure.** Every `ReviewClaim.execution_id` in a v2 stream must resolve to
  a `Completed` `ExecutionRecord` whose `parsed_claim_ids` contains that claim's own ID (§7's
  Invariant 3), and every `ObligationLifecycle::Completed` transition must be backed by one (§7's
  Invariant 1). `ReviewAggregate::validate` enforces this unconditionally for a v2 stream.
- **v1 replay is read-only and permits legacy unresolved execution.** A stream tagged v1 (only ever
  a pre-M3 stream imported from before this ADR shipped) replays under the same relaxed rule the M1
  code already had: `ReviewClaim.execution_id` is carried and hashed but never resolved against any
  execution record, exactly matching `docs/15` §7's existing `unknown`/`unresolved` sentinel/
  `execution_metadata_unavailable` obstruction path. This is the only place that legacy behavior
  survives; it is not available to any stream tagged v2.
- **One stream, one schema tag.** `EventEnvelope::validate_sequence` rejects a prefix that mixes v1
  and v2 tags across its events — a stream is v1 throughout or v2 throughout, never a splice. An
  importer that needs to carry a pre-M3 history forward into new M3 work must start a new v2 run
  whose own genesis is derived from (and audit-traces back to) the imported v1 run's final state,
  not append v2 events onto a v1 tail.
- **`reviewgraphen-store`'s replay path — mediated entirely through ADR 0014 §3's
  `ValidatedEventView`, the only way `reviewgraphen-store` reads events — is schema-tag-aware.**
  Authority-free/authority-bearing classification (ADR 0014 §3) applies identically under both tags;
  what differs between v1 and v2 is only the execution-closure rule above, which is a
  `reviewgraphen-core` aggregate-validation concern, not a store concern.
- **`RunGenesisManifest` (ADR 0014 §3) records which contract a run's genesis committed to.** Every
  v2 run's `RunGenesisManifest.event_contract_version` is `"reviewgraphen.review_event.v2"`; a v1
  stream being replayed for read/import has no `RunGenesisManifest` of its own (that record is new
  in this ADR/ADR 0014 and was never minted for pre-M3 runs) — its absence is exactly the signal
  that legacy relaxed validation, not v2 execution closure, applies. ADR 0014 §3 defines the type
  and its persistence; this ADR only fixes the version string it must carry for a v2 run.

### 3. Execution outcome is separate from obligation lifecycle

M3 introduces `ExecutionOutcome`, a closed enum distinct from `ObligationLifecycle`:

```rust
pub enum ExecutionOutcome {
    /// The reviewer returned a structured result. The claims it proposed
    /// (at least one — see §6a) are carried by the same atomic
    /// `ReviewExecutionRecorded` event as this outcome, never by this enum.
    Completed,
    /// The reviewer validly declined, citing one closed taxonomy reason (§11).
    Abstained { reason: AbstentionReason, detail: String },
    /// The raw response could not be parsed/validated into a structured result.
    Malformed { reason: MalformedOutputReason, diagnostic: String },
    /// The reviewer adapter itself failed (transport, timeout, tool failure).
    ProviderFailure { retryable: bool, diagnostic: String },
}
```

`Completed` carries no `claim_ids` field. Earlier drafts of this ADR put a `claim_ids` set directly
on the `Completed` variant; that duplicated `ExecutionRecord.parsed_claim_ids` (§6) with no
mechanism forcing the two to agree, which is exactly the kind of redundant derived state `AGENTS.md`'s
bug-fixing discipline asks not to introduce. `ExecutionRecord.parsed_claim_ids` is now the **only**
place a `Completed` execution's claim-ID set is recorded; §6a's atomic event is what keeps it
consistent with the claims actually attached to that execution, by construction rather than by a
separately-checked invariant between two copies of the same set.

An `ObligationTransition` event to `ObligationLifecycle::Completed` is valid **only** when it is
backed by at least one `ExecutionRecord` (§6) for that obligation whose `outcome` is
`ExecutionOutcome::Completed` (§7 states this as an aggregate invariant, not just an orchestration
convention). `Abstained`, `Malformed`, and `ProviderFailure` never justify a `Completed`
transition: the orchestrator leaves the obligation at `InProgress` (already `visited` per
`docs/11` §3.3, since it was actually handed to a reviewer) and may retry the same obligation with
a new `ExecutionRecord` (new attempt number, §6) without any lifecycle transition at all. This is
the mechanism, not just the policy, behind the `docs/11` correction in §15: `Coverage::raw`
("completed") was already defined purely in terms of `ObligationLifecycle::Completed`
(`coverage.rs:72`); M3 guarantees that lifecycle value is reachable only through a structured
result, so the existing coverage code requires no change — only the outdated prose describing it
does (§15 below).

Only explicit policy — never the execution layer itself — may move a persistently
abstaining/failing obligation to `Stale` or `Cancelled`; M3 does not add that policy (it is a
scheduler/gate concern for a later milestone) and an obligation may remain `InProgress` across
arbitrarily many retried `ExecutionRecord`s.

### 4. `ReviewContextEnvelope`: canonical, cache-keyed core record

```rust
pub struct ReviewContextEnvelope {
    id: StableId,                       // context-envelope:<derived>
    obligation_ids: BTreeSet<StableId>,  // one, or a tightly coupled batch (docs/09 §2)
    snapshot_id: StableId,               // exactly one snapshot (ADR 0004 invariant)
    projection_policy_version: String,   // e.g. "context.baseline@1"
    candidate_source_ids: BTreeSet<StableId>,       // the deterministic candidate set; see below
    included_sources: Vec<SourceArtifactRef>,       // ordered; see below
    normalized_included_source_ids: BTreeSet<StableId>, // artifact IDs only, for set validation
    excluded_sources: Vec<ExcludedSourceRef>,       // ordered by artifact_id; see below
    unknowns: Vec<EnvelopeUnknown>,      // unresolved references, never prose-filled (docs/08 §5.4)
    assumptions: Vec<String>,            // reviewer-facing, canonical and identity-bearing
    losses: Vec<EnvelopeLoss>,           // §5
    projection_hash: ContentHash,        // sha256 over the canonical envelope body, this field excluded
}

pub struct ExcludedSourceRef {
    artifact_id: StableId,   // a candidate_source_ids member the builder did not include
    reason: String,          // why (bounded-expansion policy limit, irrelevant to target/property, ...)
}

pub struct EnvelopeUnknown {
    description: String,
    source_ids: BTreeSet<StableId>,      // may be empty: an unknown can predate any accepted fact
}
```

`candidate_source_ids` is the deterministic set of source artifact IDs the builder considered before
applying the bounded-expansion policy: in M3 it is exactly every accepted `file:*` artifact ID in
the snapshot's ProgramSpace, in StableId order. This deliberately broad, directly enumerable
baseline avoids inventing a target/property lookup API that core does not have; later policies may
replace it only under a new projection-policy version. It is computed before any callers/callees-depth,
path-count, or excerpt-line bound narrows it — the same scope §5's `EnvelopeLoss` invariant already
measures the included set against. Completeness is a checked partition, not a convention:
`normalized_included_source_ids` and `excluded_sources`' artifact IDs are disjoint, and their union
equals `candidate_source_ids` exactly — every candidate has a recorded fate, included or
excluded-with-`reason`, and none silently falls out of the projection with no trace of the decision.
An excluded candidate names the policy bound it fell outside of in `reason` (e.g., "beyond callee
depth 3"), which is what gives §5's `EnvelopeLoss` invariant something concrete to check against —
`losses` must be non-empty whenever `excluded_sources` is non-empty (§5, restated there).

`included_sources` replaces a bare `included_source_ids: Vec<StableId>`. Naming a source ID was
never enough for a reviewer to actually read it: nothing connected the Envelope to retrievable
bytes, and `docs/15`/ADR 0014 exist precisely because "re-open the workspace later" is not a safe
retrieval path (ADR 0014 Context). Each entry is a versioned reference into the ADR 0014 CAS:

```rust
pub struct SourceArtifactRef {
    registration_id: StableId,  // resolves within this snapshot's SnapshotSourcesRecorded map
    artifact_id: StableId,      // the file:* artifact this excerpt belongs to
    content_hash: ContentHash,  // ADR 0014 §2 SnapshotSourceEntry.content_hash for the same bytes
    cas_hash: ContentHash,      // ADR 0014 §2 SnapshotSourceEntry.cas_hash — the CAS storage key
    excerpt: Option<ExcerptRange>, // None = the whole artifact; Some = a bounded line range (docs/08 §5.3)
}

pub struct ExcerptRange {
    start_line: u32,
    end_line: u32,   // inclusive, 1-based
}
```

The trusted caller that constructs a `ReviewerRequest` (§8) — the orchestrator, never
`reviewgraphen-reviewer` itself — resolves each `cas_hash` against ADR 0014's CAS to obtain the exact
full registered bytes it passes as `ResolvedSourceInput.artifact_bytes`; the request constructor
validates them and exposes only the declared excerpt. `content_hash` is retained alongside it because
it is the same field ADR 0014 §2 already carries on `SnapshotSourceEntry` and it is what `docs/08`
§13's cache key ("source IDs and hashes") already names — dropping it here would force the Envelope
to re-derive it indirectly through the CAS instead of carrying it directly. Neither
`reviewgraphen-core` nor `reviewgraphen-reviewer` performs CAS I/O (ADR 0014's crate-ownership rule,
unchanged, and now extended to the reviewer crate by §8's `ReviewerRequest` contract): both only
carry and validate these hash references, exactly as `reviewgraphen-core` already does for every
other `ContentHash`-typed field.

`id` is `StableId::derived("context-envelope", ...)` over the complete canonical projection body:
`snapshot_id`; the ordered `obligation_ids`; `projection_policy_version` (the only
profile/policy/version input this ADR's baseline builder carries); `candidate_source_ids`;
`included_sources` in order, each contributing its `registration_id`, `artifact_id`, `content_hash`,
`cas_hash`, and `excerpt`; `excluded_sources` in order, each contributing its `artifact_id` and `reason`;
`unknowns`; `assumptions`; and `losses` — exactly the same set `projection_hash` (below) hashes, so
the two values are always derived from one canonical body, never two independently-maintained ones.
This supersedes an earlier ADR draft that derived `id` from a narrower `docs/08` §13 cache-key
subset (`snapshot_id`, obligation keys, `projection_policy_version`, and `included_sources`' artifact
IDs/hashes only, omitting `unknowns`/`assumptions`/`losses`/excerpts/excluded sources): that narrower
tuple let a change to, say, a loss declaration or an excerpt's line range leave `id` unchanged while
`projection_hash` moved, splitting one record's two identity signals out of sync for no reason —
every one of the listed components changing now changes both `id` and `projection_hash`, with no
exception. Model/prompt/tool-policy version remain **excluded**, as before — `docs/08` §13 places
them in the Execution cache key, not the Envelope cache key, and §6 confirms context construction is
a deterministic, snapshot-bound projection independent of which reviewer later consumes it. Two
obligations that happen to produce an identical projection therefore still share one Envelope ID and
one event record; an obligation whose projection changes in any of the listed components (different
snapshot, different candidate/included/excluded set, different excerpt, different declared loss,
different policy version) always gets a new Envelope ID and a new `projection_hash`, per ADR 0004's
"typed expansion operation" requirement — there is no in-place Envelope mutation.

`projection_hash` is a separate content hash over that same complete canonical body — `snapshot_id`,
`obligation_ids`, `projection_policy_version`, `candidate_source_ids`, `included_sources`,
`excluded_sources`, `unknowns`, `assumptions`, `losses` — **with `id` and `projection_hash`
themselves each fixed out of their own preimage before it is computed**: `id`'s derivation input
never includes `id`'s own value, and `projection_hash`'s hash preimage never includes
`projection_hash`'s own value; no other field is excluded from either. A hash or a derived ID cannot
honestly include itself, and earlier ADR text left this implicit rather than stated for
`projection_hash` alone, saying nothing about `id`; both exclusions are now explicit and symmetric,
and neither excludes anything beyond its own field. `projection_hash` is used only for byte-level
materiality comparison; it is redundant with `id` today (nothing yet needs to compare content
without re-deriving identity) but is retained now because RG-507 (deferred, §1) will need it and
retrofitting it as a schema-breaking addition later is worse than an unused field now.

The envelope builder's default policy is fixed bounded expansion, taken directly from `docs/08`
§5.3: callers depth 2, callees depth 3, path count ≤ 20, source excerpt ≤ configured line bound,
related tests ≤ 10. **Full-snapshot inclusion is never the implicit default** — `RG-500`'s own
acceptance criterion. The only way to obtain a broader Envelope is the explicit typed expansion
operation ADR 0004 already names, which always produces a new Envelope ID.

### 5. `EnvelopeLoss`: information-loss declaration

```rust
pub struct EnvelopeLoss {
    description: String,
    severity: Severity,                  // reused from program.rs; not a new enum
    affected_properties: BTreeSet<String>,
    source_ids: BTreeSet<StableId>,       // MAY be empty — see below
}
```

`Severity` is the existing `program.rs` enum, reused rather than duplicated (`AGENTS.md`
"同じlogicが2箇所以上に現れる場合、共有関数へ抽出"; the same rationale applies to a shared value
type — this ADR reuses it a second time for `RiskDescriptor`, §5a). `EnvelopeLoss` is a distinct
type from `program::Limitation`, not a reuse of it, because their non-emptiness contracts differ on
purpose: a `Limitation`'s `source_ids` must be non-empty (ADR 0011 §4 — a limitation must ground
out in a concrete fact), but an `EnvelopeLoss` describes *omission*, and some omissions have no
enumerable source set at all — "indirect calls beyond depth 3 omitted" or "full repository not
included" (`docs/08` §6) name a scope, not a fact. Reusing `Limitation`'s non-empty-source rule for
that case would force a fabricated placeholder ID; keeping the two types distinct keeps both rules
honest.

Aggregate-level invariant (checked wherever an `EnvelopeLoss` is validated, mirroring how
`ProgramSpaceBuilder::build` already checks `Limitation`): if `excluded_sources` (§4) is non-empty —
equivalently, if `normalized_included_source_ids` does not cover all of `candidate_source_ids` —
`losses` must be non-empty. `candidate_source_ids` is itself already checked (§4) to be exactly the
set the snapshot's `ProgramSpace::known_ids()` could have offered for this obligation's
target/property, so this restates ADR 0004's own invariant as an enforced rule over the Envelope's
own recorded fields, rather than a documentation-only claim that reaches back into `ProgramSpace`
independently of what the Envelope itself declares.

### 5a. `RiskDescriptor` (RG-501): a scheduler-policy value, not a program fact

```rust
pub struct RiskDescriptor {
    impact: Severity,       // reused program.rs enum — see §5
    likelihood: Severity,   // reused program.rs enum
    rationale: String,      // descriptive only, excluded from every derived ID (docs/07 §5 precedent)
}
```

`docs/11` §6 already names the concept in prose ("risk: 欠陥が存在した場合の影響と可能性に関する
descriptor") without a concrete type; this closes RG-501 with the minimum structure the M3 baseline
scheduler (§5b) actually needs to order obligations, and no more. `RiskDescriptor` is explicitly
**not** a program fact and never gains a `source_ids` field the way `Limitation`/`EnvelopeLoss` do:
`docs/11` §16 Invariant 5 already requires "risk scoreをtruth probabilityと呼ばない," and `docs/11`
§15 requires the risk *formula* itself to stay out of `reviewgraphen-core`, as ReviewGraphen
scheduler policy instead. M3 honors both: `impact` is computed by a single, fixed, versioned bucket
mapping over `Obligation::weight()` (already an existing field, "risk weight used only for weighted
coverage") — a small ordinal lookup table, not a weighted formula — named by a policy-version string
carried on `ReviewPlan` (§5b), never hard-coded silently inside the mapping function itself.
`likelihood` is fixed to `Severity::Medium` uniformly for every obligation in M3: no calibrated
likelihood model exists yet (RG-503, deferred, §1), and reporting a fabricated per-obligation
likelihood would be a worse failure than reporting an honest constant. This is a deliberately
provisional, baseline-only concretization of RG-501, revisited when RG-503 lands (Revisit triggers).

### 5b. Baseline deterministic scheduler and `ReviewPlan`

```rust
pub struct ReviewPlan {
    id: StableId,                              // plan:<derived>
    universe_id: StableId,
    snapshot_id: StableId,                     // direct binding, mirrors ReviewContextEnvelope (§4)
    planner_policy_version: String,            // e.g. "scheduler.baseline@1"
    planner_policy_hash: ContentHash,          // hash of the fixed threshold/ordering table this version names
    budget: PlanBudget,
    risk_breakdown: BTreeMap<StableId, RiskDescriptor>, // one entry per obligation placed in a wave or deferred
    waves: Vec<ScheduleWave>,                  // ordered; obligation IDs disjoint across waves
    deferred: BTreeMap<StableId, String>,      // obligation ID -> deferral reason; never removed from the universe denominator
}

pub struct PlanBudget {
    max_waves: u32,
    max_obligations_per_wave: u32,
}

pub struct ScheduleWave {
    id: StableId,
    obligation_ids: Vec<StableId>,       // ordered, deterministic
    reason: String,
}
```

RG-502's baseline scheduler is a deterministic priority queue: obligations are ordered by
`(risk.impact, -dependency_depth, StableId)` — `StableId` as the final tiebreaker guarantees a
total, reproducible order with no hidden randomness or hash-map iteration dependence, and `risk` is
exactly the `RiskDescriptor` (§5a) this same plan records in `risk_breakdown`. Dependency edges
(`Obligation::depends_on`) are respected: an obligation only enters a wave once every ID in its
`depends_on` set has already been placed in an earlier wave. `deferred` holds every obligation
`budget` could not place, each with an explicit reason string; it is reported, never dropped from
`UniverseDescriptor`, matching `docs/11` §10/§14 ("deferred obligationはcoverageに残す").
**"Priority" is represented structurally, not as a separate numeric field**: a plan's own wave
sequence, and each wave's ordered `obligation_ids`, *is* the processing order `docs/11` §6 calls
priority — recording a redundant priority score alongside an already-ordered `Vec` would be exactly
the kind of derived, potentially-divergent duplicate state `AGENTS.md`'s bug-fixing discipline warns
against.

`ReviewPlan.id` derives from `(universe_id, snapshot_id, planner_policy_version,
planner_policy_hash, budget, wave contents in wave order, deferred obligation-ID set)`. Description
text, `reason` strings (both `ScheduleWave.reason` and each `deferred` reason), and `risk_breakdown`
are excluded from identity — `risk_breakdown` records *why* a given plan ordered things as it did,
but the plan's *policy and budget* (already bound into the ID) are what make that ordering
reproducible; carrying the derived breakdown into the ID as well would bind identity to a value
that is itself computed from already-bound inputs, matching the existing `Obligation.id` precedent
(`docs/07` §5: "説明文、priority score、timestampはIDへ含めません").

### 6. `ExecutionRecord`: fixed identity, version, and trace

```rust
pub struct ExecutionRecord {
    id: StableId,                         // execution:<derived>
    plan_id: StableId,                    // the ReviewPlanRecorded this execution's wave came from
    wave_id: StableId,                    // must resolve within that plan
    obligation_ids: BTreeSet<StableId>,   // subset of the referenced envelope's obligation_ids, and of that wave's obligation_ids
    envelope_id: StableId,
    reviewer_kind: String,                // "fake" | "llm" | ... (open string, closed set enforced by caller policy)
    reviewer_id: String,                  // e.g. "reviewgraphen.fake_reviewer@1"
    provider: Option<String>,
    model: Option<String>,
    model_revision: Option<String>,       // recorded when the provider exposes one; never fabricated
    prompt_template_version: String,      // required even for the fake reviewer ("fixture@1")
    inference_config: BTreeMap<String, String>, // e.g. {"temperature": "0.0"}; empty for the fake reviewer
    tool_policy_version: String,          // required; M3's fixed policy grants no tools (§13)
    attempt: u32,                         // 1-based; retries increment this, never reuse it
    raw_artifact_registration_id: StableId, // Sensitive ReviewerExecution registration (ADR 0014)
    raw_artifact_hash: ContentHash,       // sha256 over exactly what the reviewer returned
    parsed_claim_ids: BTreeSet<StableId>, // the sole record of a Completed execution's claim set (§3); empty unless outcome is Completed
    outcome: ExecutionOutcome,
}
```

This is the concrete, per-execution instantiation of the list `AGENTS.md`'s "確率的部分" section
already requires: provider, model, model revision when available, prompt/template version,
inference settings, context projection ID (`envelope_id`), tool call record (§13: always empty in
M3, never omitted), raw response artifact hash, parsed claim IDs, and abstention/parse failure
(`outcome`). `raw_artifact_hash` is **required**, not optional, for every `ExecutionRecord`
regardless of reviewer kind — including the fake reviewer, whose "raw artifact" is its fixture's
own JSON bytes. Requiring it uniformly avoids a special case in downstream audit/report code and
keeps `docs/09` §14's execution cache key ("obligation semantic key / context envelope hash /
reviewer descriptor / model and prompt version / tool policy version") meaningful for every
reviewer kind, not only probabilistic ones. `ExecutionRecord.id` is
`StableId::derived("execution", ...)` over exactly that cache-key tuple **plus** `attempt`, so a
retry always mints a new, distinct record (`docs/09` §10: "retryにはattempt番号とraw artifactを残す")
while a byte-identical re-run of the same attempt against the same inputs collides safely (the
existing `IdRegistry`/`DomainError::IdCollision` pattern already handles identical-content replay
vs. genuine collision, `id.rs:212`).

`plan_id`/`wave_id` are new: every execution now traces back to the deterministic plan that
scheduled it, not only to the envelope it reviewed. **Plan inclusion validation** (enforced by
`ReviewAggregate::validate`, §7): `plan_id` must resolve to a recorded `ReviewPlanRecorded`;
`wave_id` must name one of that plan's `waves`; every ID in `obligation_ids` must be contained in
that wave's `obligation_ids`. This is the same "resolves within its referenced collection" pattern
already used for `envelope_id`/its `obligation_ids`, applied one level up.

Only `attempt`, `outcome`, `raw_artifact_registration_id`, `raw_artifact_hash`, and
`parsed_claim_ids` vary across the life of a
retried obligation; every other field is bound once by the scheduler/envelope pairing that started
the attempt.

The physical bytes `raw_artifact_hash` addresses live in the content-addressed store ADR 0014
defines; `reviewgraphen-core` itself has no filesystem access and never reads them — it only
carries and validates the hash reference, exactly as it already does for every other
`ContentHash`-typed field.
`raw_artifact_registration_id` removes registration ambiguity when identical bytes have multiple
contexts: event apply requires that exact registration's hash to equal `raw_artifact_hash`, its
sensitivity to be `Sensitive`, and its source to be `ReviewerExecution` for this execution/reviewer.

### 6a. `ReviewExecutionRecorded`: one atomic execution+claims event

```rust
ReviewExecutionRecorded {
    execution: ExecutionRecord,
    claims: Vec<ReviewClaim>,
}
```

This is the **only** path that introduces a claim under the v2 contract (§2). Earlier ADR drafts
kept `ExecutionRecord` and each of its claims as separate events (`ReviewExecutionRecorded(
ExecutionRecord)` plus one `ClaimProposed(ReviewClaim)` per claim); that design let a caller commit
an execution record whose claims never arrived, or claims whose backing execution never committed —
exactly the "unchecked reference" gap Context item 2 already flags for the pre-M3 code, just moved
one layer down instead of closed. Bundling both into one event closes it structurally.

`ReviewAggregate` applies this event **atomically**: it clones the current aggregate, applies the
execution record and then every claim to the clone, and only replaces the live aggregate with that
clone if every one of the following holds — otherwise the event is rejected in full and the live
aggregate is left completely unchanged (no partial application, matching the "clone, mutate,
validate, swap" idiom `EventLog::append_envelope` already uses for every other event kind):

1. `execution.parsed_claim_ids` equals exactly the set of `claims[*].id` — no claim is silently
   dropped from one side or invented on the other.
2. Every `claims[*].execution_id` equals `execution.id`.
3. **Obligation scope**: every `claims[*].obligation_ids` is a subset of `execution.obligation_ids`
   (which is itself already a subset of the referenced envelope's `obligation_ids`, §4/§6, and of
   the referenced wave's `obligation_ids`, §6) — a claim can never address an obligation its own
   execution was never scoped to.
4. **Source grounding**: every `claims[*].source_ids` entry resolves within the envelope's
   `normalized_included_source_ids` (§4/§10's second validation layer, re-checked here as the
   authoritative aggregate-level boundary, not only at the reviewer-output parser).
5. If `execution.outcome` is `Completed`, `claims` is **non-empty** — at least one structured claim,
   which may itself be a `ClaimPolarity::IssueAbsent` claim recording "reviewed, nothing found."
   Earlier ADR text allowed a `Completed` outcome with zero claims (mirroring `docs/09`'s "possibly
   empty" phrasing for `claim_ids`); this ADR tightens that: an empty claim set is no longer a valid
   way to express "no issues" — `docs/09` already names the structured `issue_absent` claim as the
   correct way to say that, so requiring at least one claim removes the ambiguity between "reviewed
   and found nothing" and "silently produced nothing," without adding a new claim shape.
6. If `execution.outcome` is **not** `Completed` (`Abstained`/`Malformed`/`ProviderFailure`),
   `claims` is empty and `execution.parsed_claim_ids` is empty — an execution that did not complete
   never carries claims, matching §3.
7. **Collision**: reapplying an identical `execution`/`claims` pair (byte-identical content at an
   already-used ID) is idempotent, per the existing `IdRegistry` pattern; a colliding ID with
   different content is `DomainError::IdCollision`, exactly as every other aggregate record already
   behaves.

`ObligationTransition` to `ObligationLifecycle::Completed` is valid only **after** a
`ReviewExecutionRecorded` event with `outcome: Completed` addressing that obligation has already
been committed (§3) — there is no path to `Completed` lifecycle through any other event.

### 7. Three additive event payloads; closing the `execution_id` gap

`PersistedPayload` gains three variants, additive to the existing closed enum (ADR 0014 §3 adds three
more of its own, `RunGenesisManifest`/`ArtifactRegistered`/`SnapshotSourcesRecorded`, as companion additions —
see ADR 0014 §3 for those):

```rust
enum PersistedPayload {
    // ...existing ObligationTransition, ClaimProposed, EvidenceRecorded, EvidenceBound,
    // VerificationRecorded, DecisionRecorded, FindingRecorded, unchanged...
    ReviewPlanRecorded(ReviewPlan),
    ContextEnvelopeProjected(ReviewContextEnvelope),
    ReviewExecutionRecorded { execution: ExecutionRecord, claims: Vec<ReviewClaim> },
}
```

`ClaimProposed` is **not removed**: a v1 stream (§2) still decodes and replays it exactly as M1
always has, since retiring a payload kind a real historical stream may contain would break replay,
not just this ADR's own new tests. But under the v2 contract, no caller can construct a
`ClaimProposed` command whose resulting claim passes `ReviewAggregate::validate`: `Invariant 3` (§7
below) requires every claim's `execution_id` to resolve to a `Completed` execution record that lists
that claim's own ID in `parsed_claim_ids`, and the only event that can establish that connection is
`ReviewExecutionRecorded` (§6a), atomically. Orchestration code targeting v2 must not call
`EventCommand::claim_proposed` for new work; it is retained purely as v1-replay-compatible dead
capability under v2, the same way a closed enum keeps an unused-but-decodable historical variant.

`ReviewAggregate` gains three new maps (`plans: BTreeMap<StableId, ReviewPlan>`, `envelopes:
BTreeMap<StableId, ReviewContextEnvelope>`, `executions: BTreeMap<StableId, ExecutionRecord>`) and
`validate()` gains, for a v2 stream:

- Every `ExecutionRecord.envelope_id` resolves to a recorded `ContextEnvelopeProjected`.
- Every `ExecutionRecord.plan_id`/`wave_id` resolves within a recorded `ReviewPlanRecorded` (§6's
  plan-inclusion validation).
- Every `ExecutionRecord.obligation_ids` is a subset of its envelope's `obligation_ids` and of its
  resolved wave's `obligation_ids`.
- Every envelope source registration matches the same snapshot's recorded artifact/content/CAS
  tuple and valid excerpt; every execution raw registration matches its hash, `Sensitive` class,
  and `ReviewerExecution` source for that execution (ADR 0014 §3a).
- **Every `ReviewClaim.execution_id` resolves to an `ExecutionRecord` whose `outcome` is
  `ExecutionOutcome::Completed` and whose `parsed_claim_ids` contains that claim's own ID.** This
  closes the gap named in Context item 2: a claim can no longer name an execution that never
  happened, or that abstained/failed/malformed instead of producing that exact claim.
- An `ObligationTransition` to `Completed` requires at least one connected `ExecutionRecord` with
  `outcome: Completed` addressing that obligation (§3's invariant, enforced here).
- `ReviewExecutionRecorded`'s own atomic apply rules (§6a) run before any of the above cross-checks
  can see the claims/execution it introduces.

For a v1 stream (§2), none of the above run; the M1-era unresolved `execution_id` behavior applies
unchanged. `docs/15` §7's M1 placeholder note ("execution、reviewer、provider/model、context
envelope、raw outputをcanonical stateとして保持しない...`execution_metadata_unavailable`") remains
the correct, and only, behavior for a v1 stream; it never applies to a v2 stream, which always has
real `ContextEnvelopeProjected`/`ReviewPlanRecorded`/`ReviewExecutionRecorded` events backing every
claim it contains.

### 8. `Reviewer` trait and crate placement

A new crate, `reviewgraphen-reviewer`, depends on `reviewgraphen-core` only (never on
`reviewgraphen-ingest` or `reviewgraphen-store`) and owns:

```rust
pub struct ReviewerRequest {
    envelope: ReviewContextEnvelope,
    resolved_sources: Vec<ResolvedSourceInput>, // one per `envelope.included_sources` entry, same order
}

pub struct ResolvedSourceInput {
    registration_id: StableId,
    artifact_id: StableId,
    content_hash: ContentHash,
    cas_hash: ContentHash,
    excerpt: Option<ExcerptRange>,
    artifact_bytes: Vec<u8>, // full registered bytes; fields stay private
}

pub struct ReviewerDescriptor {
    reviewer_kind: String,             // M3 closed policy admits "fake"
    reviewer_id: String,               // e.g. "reviewgraphen.fake_reviewer@1"
    provider: Option<String>,
    model: Option<String>,
    model_revision: Option<String>,
    prompt_template_version: String,   // fake uses "fixture@1"
    inference_config: BTreeMap<String, String>,
    tool_policy_version: String,       // M3 fixed no-tools policy
}

pub trait Reviewer {
    fn descriptor(&self) -> ReviewerDescriptor;
    fn review(&self, request: &ReviewerRequest) -> ReviewerResponse;
}

pub struct ReviewerResponse {
    raw_artifact: Vec<u8>,    // always present, independent of `outcome`
    outcome: ReviewerOutcome,
}

pub enum ReviewerOutcome {
    Structured(ParsedReviewerOutput),   // §10 — already validated
    Abstained { reason: AbstentionReason, detail: String },
    Malformed { reason: MalformedOutputReason, diagnostic: String },
    ProviderFailure { retryable: bool, diagnostic: String },
}
```

`ReviewerDescriptor::new` rejects empty kind/ID/prompt/tool-policy strings, requires provider and
model to be both present or both absent, forbids a revision without a model, and canonicalizes the
inference map by its `BTreeMap` order. `ExecutionRecord` copies these fields exactly; event apply
rejects a descriptor/execution mismatch rather than independently accepting two descriptions of one
reviewer attempt.

`ReviewerRequest::new(envelope, resolved_sources)` is the only constructor, and it is fallible: it
rejects — before any `Reviewer` implementation ever runs — a `resolved_sources` whose length,
order, or per-entry `registration_id`/`artifact_id`/`content_hash`/`cas_hash`/`excerpt` does not match
`envelope.included_sources` (§4) exactly, entry for entry. It also recomputes SHA-256 over each full
`artifact_bytes`, checks both content/CAS hashes and excerpt line bounds, then exposes only the
selected excerpt through a read-only `review_bytes()` accessor; reviewer implementations cannot
access the unprojected remainder. Thus this is an independently checked byte boundary, not only a
metadata assertion.
`reviewgraphen-reviewer` never opens a file or touches ADR 0014's CAS itself. Resolving each
`SourceArtifactRef` to `bytes` is the job of a trusted caller outside this crate (the orchestrator,
which already depends on `reviewgraphen-store`/ADR 0014's CAS for other reasons) — the same
CAS/filesystem-free rule §4 already states for `reviewgraphen-core` now extends to the reviewer
crate as well. A `Reviewer` implementation therefore never receives bytes whose identifiers or
excerpt disagree with its envelope.

This replaces the earlier `ReviewerOutcome::RawArtifact(Vec<u8>)` variant, which asked one variant
of a closed enum to "always accompany every variant above" — impossible, since a value of an enum
occupies exactly one variant, never one-plus-a-sibling. `raw_artifact` now lives on
`ReviewerResponse`, a struct wrapping the fixed `outcome` enum, so the raw bytes are always present
exactly once, at the same field, regardless of which `ReviewerOutcome` a reviewer returns.

This matches `docs/05` §8.3's trait shape and crate responsibility ("reviewer adapter traits、
structured output parsing、prompt contract"), narrowed for M3: only the deterministic fake
reviewer implements it. The trait itself is provider-neutral by construction — it has no
knowledge of HTTP, subprocesses, or any specific model API — so a later RG-604/RG-603 ADR adds new
implementors without changing this trait.

### 9. Deterministic fake reviewer (RG-602)

Fixture-driven: given a `ReviewerRequest`, it reads `obligation_ids` and `snapshot_id` off
`request.envelope`, looks up a pre-registered fixture keyed by that pair, and returns the fixture's
declared `ReviewerResponse` verbatim, including every `ExecutionOutcome` kind carried by `outcome`
— `Completed` (with at least one fixture claim, per §6a), `Abstained` (covering all 8 taxonomy
reasons, §11), `Malformed` (covering every parser rejection reason, §12), and `ProviderFailure`
(both `retryable: true` and `retryable: false`). It never calls `review_bytes()` on any
`request.resolved_sources` entry and never performs I/O of its own: `ReviewerRequest::new` (§8) has
already proven those entries' declared identifiers, hashes, and excerpts match the envelope's
sources, and the fake reviewer trusts that proof instead of re-deriving anything from the bytes
themselves. Its `ReviewerResponse.raw_artifact` is always the
fixture's own canonical JSON, present alongside every outcome kind including `Malformed` and
`ProviderFailure`. This is what makes every negative test in §14 runnable without a real provider.

### 10. Structured reviewer-output parser

New schema `reviewgraphen.reviewer_output.v1` (`schemas/reviewgraphen.reviewer_output.v1.schema.json`,
with a matching example fixture), following `docs/09` §7's shape: `schema`, `execution_id` (assigned
by the orchestrator, not the reviewer), `claims[]` (`polarity`, `property_id`, `target_refs`,
`statement`, `source_ids`, `assumptions`, `confidence`, `requested_evidence`), `abstention`. Two
validation layers, matching the existing two-layer pattern in `schemas/README.md`:

1. **JSON Schema** — shape, required fields, enum ranges, `additionalProperties: false`.
2. **Cross-record validator** (`reviewgraphen-reviewer`, before any `ReviewClaim` is constructed)
   — every `source_ids` entry must resolve within `request.envelope`'s `normalized_included_source_ids`
   (a claim cannot cite a source the envelope never included — `docs/09` §8's source-citation rule
   plus ADR 0004's audit requirement); `target_refs` and `polarity` must be valid; `confidence` (if
   present) must be in `[0.0, 1.0]` (already enforced by `ReviewClaim::validate_initial`,
   `review.rs:594`, and re-checked here before that constructor is even called, so a malformed
   confidence never reaches it as a panic-shaped surprise); if `outcome` would be `Completed`,
   `claims` must be non-empty (§6a).

A parser failure at either layer becomes `ReviewerOutcome::Malformed` inside the reviewer's
`ReviewerResponse` (`raw_artifact` stays the raw bytes that failed to parse), never a Rust-level
`panic` or a silently-discarded value, and never an `Err` that aborts the whole run — it is a valid,
expected, retryable outcome (§3, §12; the orchestrator carries it forward as `ExecutionOutcome::
Malformed`). The parser's output is what the orchestrator packages into one `ReviewExecutionRecorded`
event (§6a); the parser itself never calls `EventLog::append`.

### 11. Abstention taxonomy — closed 8-reason enum

```rust
pub enum AbstentionReason {
    InsufficientContext,
    UnresolvedSymbol,
    RequiredEvidenceUnavailable,
    PropertyNotUnderstood,
    ConflictingSources,
    ToolCapabilityMissing,
    BudgetExhausted,
    PromptInjectionSuspected,
}
```

Exactly the 8 reasons `docs/09` §9 names, no more, no fewer, matching its own closed-list framing.
`ToolCapabilityMissing` is retained even though M3 grants no reviewer any tool capability (§13) —
a reviewer must still be able to name "I would need a tool I don't have" as a legitimate reason,
and the taxonomy should not need a schema-breaking addition the day RG-603/604 grant the first
real tool.

### 12. Malformed-output handling never fabricates a claim

```rust
pub enum MalformedOutputReason {
    SchemaViolation,
    UnresolvedSourceId,
    UnknownObligationId,
    InvalidPolarity,
    ConfidenceOutOfRange,
    UnknownField,
}
```

A closed set mirroring `docs/09` §7's own rejection list ("unknown field、missing source refs、
不正enum、範囲外confidence"). Every rejection reason is one of these; the parser never falls back
to inventing an `issue_absent` claim or a zero-claim "no issues found" `Completed` outcome —
`RG-600`'s acceptance criterion is enforced structurally: `ExecutionOutcome::Malformed` and
`ExecutionOutcome::Completed` are different enum variants, so a caller cannot accidentally treat
one as the other without an explicit, visible match arm, and §6a's non-empty-claims rule for
`Completed` closes the remaining "empty but Completed" loophole at the aggregate boundary too.

### 13. Prompt-injection boundary

M3's reviewer contract grants **zero** tool capabilities — `tool_policy_version` (§6) names a
fixed, empty-capability policy (e.g. `"reviewgraphen.tool_policy.none@1"`), consistent with M3's
scope excluding RG-603 (the first adapter that could invoke anything). Concretely:

- Source content resolved into a `ReviewerRequest` (as `ResolvedSourceInput.artifact_bytes`, exposed
  to a `Reviewer` only through the excerpt-bounded `review_bytes()` accessor, §8, read from the CAS
  by the trusted orchestrator before the request is built) is passed to a `Reviewer`
  implementation as data, never concatenated into anything the trait treats as an instruction
  channel — the trait boundary itself (`&ReviewerRequest -> ReviewerResponse`) has no side channel
  through which source content could alter which method gets called or with what capability.
  `reviewgraphen-reviewer`'s future real-provider adapter (RG-604, deferred) is responsible for
  implementing `docs/09` §4's SYSTEM/OBLIGATION/CONTEXT/KNOWN-UNKNOWNS/OUTPUT-SCHEMA prompt
  separation when it exists; this ADR fixes the trait shape that makes that separation possible but
  does not itself send anything to a model.
- `AbstentionReason::PromptInjectionSuspected` is a legitimate, expected outcome the fake reviewer
  fixture set must include, so the orchestration path for it (record the execution, do not
  transition the obligation to `Completed`, per §3) is exercised before any real provider exists.
- Independently of a reviewer's own abstention, the structured-output parser (§10) rejects any
  `claims[]` entry whose `requested_evidence`/`target_refs` would name a tool, path, or scope
  outside the envelope's `normalized_included_source_ids` — matching `docs/08` §14's "toolsを
  allow-list化" and `docs/16` §3's "suspicious source instructionを検出したらsecurity obstruction":
  such a rejection is `MalformedOutputReason::UnresolvedSourceId` or `UnknownObligationId`, not a
  fabricated claim.

### 14. Confidence is not authority

Already enforced by `ReviewClaim::propose_ai`/`validate_initial` (`review.rs:536-608`): every AI
claim begins `ClaimDisposition::Proposed` and `ReviewStatus::Unreviewed` regardless of
`candidate_confidence`, and `transition_disposition` never reads `candidate_confidence`. M3 adds
nothing new here — this section exists only to record, for the ADR record, that the parser (§10)
and `ExecutionRecord` (§6) both treat `confidence` purely as descriptive payload: no scheduler,
gate, or aggregate rule introduced by this ADR branches on its value. The same applies to
`RiskDescriptor` (§5a): `impact`/`likelihood` are scheduler-ordering signals, never lifecycle,
disposition, or gate inputs.

### 15. Coverage correction (`docs/11` §3.4)

`docs/11_coverage_and_scheduling.md` §3.4 is corrected in the same commit as this ADR (see the
accompanying edit) to read: "structured resultを得たobligationのみ。有効なabstention、malformed
output、provider failureは`visited`のまま残り、`completed`には含めない" — matching `docs/07` §14,
`docs/09` §9/§16, `docs/11`'s own §12 worked example, `AGENTS.md`'s boundary list, and this ADR's
§3/§7 enforcement. No other section of `docs/11` changes: §12's worked example, §14 (gate status),
and §16 (invariants) were already consistent with this reading.

## Consequences

### Positive

- `ReviewClaim.execution_id` becomes a real, checked reference for the first time, for every claim
  author kind — a claim can no longer exist without a backing, outcome-typed execution record.
- Abstention, malformed output, and provider failure become first-class, retryable, auditable
  outcomes instead of undifferentiated non-events — closing the exact gap `docs/15`'s M1 placeholder
  note already flagged.
- The docs/11-vs-docs/09 contradiction is resolved in the safe direction before any scheduler or
  CI gate is built against the permissive reading, avoiding a later breaking coverage-semantics
  change.
- The `Reviewer` trait and fake reviewer let every negative test in §14 run without a real provider,
  network access, or secrets — consistent with `docs/16`'s untrusted-repository threat model.
- `ReviewContextEnvelope`'s identity is cache-key-derived, so repeated executions against an
  unchanged projection reuse the same Envelope ID and event record, as ADR 0004 requires.
- `ReviewPlan` gives coverage reporting and audit a deterministic, replayable answer to "in what
  order, under what policy and budget, were obligations actually scheduled" — previously only
  described in `docs/11` prose, never recorded.
- Bundling an execution and its claims into one atomic event (§6a) makes "an execution exists with
  no claims" or "a claim exists with no execution" unrepresentable, instead of merely disallowed by
  a separately-checked cross-reference.

### Negative

- Three new `PersistedPayload` variants and a genuine event-schema version bump (`v1` -> `v2`) are
  new surface area to keep deterministic under `reviewgraphen-core`'s existing boundary tests, and
  the v1/v2 dual-behavior in `ReviewAggregate::validate` is a permanent piece of legacy-compat code
  this crate now owns.
- `ExecutionRecord`'s always-required `raw_artifact_hash` means even the fake reviewer's
  fixture-driven path must produce and content-address a byte artifact for every attempt, adding
  bookkeeping to what would otherwise be a pure in-memory test double.
- Requiring `Completed` to carry at least one claim (§6a) means every fixture and future reviewer
  adapter must always produce a structured `issue_absent`-shaped claim for a clean review, instead
  of the cheaper "just return nothing" shape earlier ADR drafts allowed.
- Deferring RG-503/506/507/603/604/607 means M3 cannot yet exercise a real cost/token budget, a
  real provider's abstention behavior, or concurrent execution — those risks are explicitly carried
  forward, not eliminated, and each needs its own follow-on ADR before it can land. `RiskDescriptor`
  (§5a) in particular is an intentionally coarse, provisional bucketing that RG-503 will need to
  revisit, not a calibrated risk model.
- `EnvelopeLoss` being a distinct type from `program::Limitation` (§5) is a second, similarly-shaped
  type in the codebase; a future ADR may need to justify why they still don't converge if a third
  loss-shaped record appears.

## Alternatives considered

### A. Let a valid abstention advance `ObligationLifecycle` to `Completed`

Rejected. This is the status quo `docs/11` §3.4 prose describes, and it directly contradicts
`docs/09` and `docs/07`. It would also make `Coverage::raw` ambiguous between "a reviewer produced
a judgment" and "a reviewer was merely invoked," undermining the exact distinction `AGENTS.md`
requires ("`reviewed`、`evidence_supported`、`verified`、`human_accepted`を同義にしない" — the same
principle applies one level down, between "attempted" and "completed").

### B. Give `ObligationLifecycle` a dedicated `Abstained`/`Failed` variant instead of a separate `ExecutionOutcome` type

Rejected. `ObligationLifecycle` is a coarse, five-state coverage axis shared by every future
milestone (M4-M7); folding execution-attempt detail into it would force every consumer of lifecycle
(coverage, gating, staleness) to learn new states it does not otherwise need, and would not
naturally support multiple retried attempts per obligation (a lifecycle is single-valued per
obligation; execution attempts are a history). Keeping `ExecutionOutcome` on a separate,
one-per-attempt `ExecutionRecord` lets the lifecycle axis stay exactly as coarse as `docs/11` needs
it while the execution history stays fully detailed.

### C. Store raw model output inline in the event payload instead of a content-addressed reference

Rejected. `AGENTS.md` and `docs/15` §6 both require raw output to be a content-addressed artifact,
not embedded canonical state — an inline payload would bloat every event read/replay with
potentially large prose the domain layer never needs to interpret, and would defeat the artifact
retention-policy controls `docs/15` §12 already specifies per artifact kind.

### D. Make `EnvelopeLoss.source_ids` required non-empty, like `Limitation`

Rejected (§5). Some real losses ("full repository not included," "indirect calls beyond depth 3
omitted") describe an unbounded scope, not an enumerable set of IDs; forcing a non-empty set would
require fabricating placeholder IDs that name nothing real, which is a worse failure mode than an
explicitly-allowed empty set.

### E. Skip a dedicated `Reviewer` crate and keep the trait in `reviewgraphen-core`

Rejected. `docs/05`'s crate table already assigns "reviewer adapter traits、structured output
parsing、prompt contract" to a distinct `reviewgraphen-reviewer` crate, separate from core's "IDs、
domain records...". M3 is the first milestone that needs the trait at all, and every future
reviewer implementation (subprocess, real provider) will need dependencies (process spawning, HTTP
clients) core must never carry. Introducing the crate now, while it is still small, avoids a later
disruptive split once real implementations exist.

### F. Bring RG-604 (one real LLM provider adapter) into M3 alongside the fake reviewer

Rejected, per explicit instruction and `docs/18`'s own MVP risk ordering (§7): "minimal projection
が情報を落とす" and "relation/path抽出が不十分" are named as higher-priority risks than provider
count. A real provider adapter also needs its own data-policy ADR (`docs/16` §6: source-upload
policy, retention, region) that this ADR should not bundle in.

### G. Keep `ExecutionRecord` and its claims as separate events, joined only by a checked `execution_id` reference

Rejected (§6a). This is what an earlier draft of this ADR did, and it reproduces the exact
"reference exists but nothing guarantees both sides commit together" shape Context item 2 already
flags as the M1 defect this ADR exists to close — just one layer lower (execution-to-claims instead
of claim-to-execution). Atomic, all-or-nothing application of one `ReviewExecutionRecorded` event
removes the possibility entirely instead of re-checking it after the fact.

### H. Keep `EVENT_SCHEMA` at `v1` and treat the new payload variants as a purely additive vocabulary widening

Rejected (§2). This was this ADR's own original position, and it is wrong: "additive vocabulary"
correctly describes payload *variants* joining a closed enum, but it does not — and cannot — also
describe a genuine *semantic* tightening of what a stream is allowed to claim (execution closure,
§3/§7). A v1-tagged stream that suddenly started enforcing execution closure would silently reject
every pre-M3 event stream it used to accept; a v1-tagged stream that didn't enforce it would make
the tag lie about what was actually checked. Bumping to `v2` makes the schema tag an honest,
checkable claim about which validation contract a stream was produced and replayed under.

## Invariants

1. `ObligationLifecycle::Completed` is reachable only via an `ObligationTransition` event backed by
   an `ExecutionRecord` with `outcome: ExecutionOutcome::Completed` addressing that obligation, and
   only under the v2 contract (§2).
2. A valid abstention, malformed output, or provider failure never advances an obligation past
   `InProgress`/`visited`, and never mutates `ObligationLifecycle` at all.
3. Every `ReviewClaim.execution_id` resolves to a `Completed` `ExecutionRecord` whose
   `parsed_claim_ids` contains that claim's own ID (v2 contract only; §2 states the v1 exception).
4. Every `ExecutionRecord.raw_artifact_hash` is present, regardless of reviewer kind or outcome.
5. Every `ReviewContextEnvelope` traces to exactly one snapshot and one obligation (or tightly
   coupled batch); its `id` never changes for an unchanged cache-key tuple, and any broadened
   projection mints a new ID.
6. `EnvelopeLoss` is non-empty whenever `excluded_sources` is non-empty — equivalently, whenever
   `normalized_included_source_ids` does not cover all of `candidate_source_ids` (§4/§5).
7. `candidate_confidence`/parsed `confidence`, and `RiskDescriptor.impact`/`likelihood`, never
   appear as an input to any lifecycle, disposition, or gate decision introduced by this ADR.
8. A structured claim's every `source_ids` entry resolves within its originating envelope's
   `normalized_included_source_ids`.
9. `AbstentionReason` and `MalformedOutputReason` are each a closed, exhaustively-matched set; a
   new reason requires a schema/version decision, not a silent string addition.
10. `reviewgraphen-reviewer` depends only on `reviewgraphen-core`; `reviewgraphen-core` never
    depends on `reviewgraphen-reviewer`.
11. A `ReviewExecutionRecorded` event applies atomically: its execution record and every one of its
    claims either all become visible in the resulting aggregate, or none do (§6a).
12. A `Completed` `ExecutionRecord`'s co-submitted `claims` is never empty; a non-`Completed`
    `ExecutionRecord`'s co-submitted `claims` (and its own `parsed_claim_ids`) is always empty.
13. Every `ExecutionRecord.plan_id`/`wave_id` resolves within a recorded `ReviewPlanRecorded`, and
    every `ExecutionRecord.obligation_ids` is a subset of that wave's `obligation_ids`.
14. A single event stream never mixes `reviewgraphen.review_event.v1` and `...v2` schema tags across
    its own events.

## Migration and versioning

- `EVENT_SCHEMA` moves from `reviewgraphen.review_event.v1` to `reviewgraphen.review_event.v2` (§2).
  This is a genuine, explicit major-contract change, not an additive widening: a v2 stream enforces
  execution closure (Invariant 3) that a v1 stream never did and never will. `EventEnvelope::
  validate`/`validate_sequence` accept both tags but reject a stream mixing them (Invariant 14).
- **Compatibility**: an existing v1 event stream (the original six payload kinds, plus
  `ObligationTransition`) continues to replay exactly as it always has — unresolved
  `execution_id`s, no execution-closure checks, `docs/15` §7's `unknown`/`unresolved` sentinel path
  in the report adapter. Nothing about a v1 stream's meaning changes. No new v1 stream may be
  created after this ADR ships; `EventLog::new` always mints v2.
- **Migration**: there is no automatic v1-to-v2 upcast. A v1 run that needs M3-era plan/context/
  execution tracking starts a fresh v2 run; it may reference the v1 run's final aggregate state as
  input/audit provenance, but it does not rewrite the v1 stream's own events in place (append-only,
  `docs/15` §1/§17).
- `reviewgraphen.reviewer_output.v1` is a **new** schema file, not a version bump of an existing
  one — no prior schema described reviewer output, so there is nothing to migrate from. (It is not
  itself renamed to `v2`: it has no v1-era predecessor to stay compatible with, unlike the event
  stream schema.)
- `docs/15` §7's M1 placeholder sentinel path (`unknown`/`unresolved` execution metadata,
  `execution_metadata_unavailable` obstruction) remains the correct, and only, behavior for a v1
  stream; a report adapter must not apply it to a v2 stream, which always contains real
  `ReviewPlanRecorded`/`ContextEnvelopeProjected`/`ReviewExecutionRecorded` events.
- `docs/11_coverage_and_scheduling.md` §3.4 is corrected by this same change; no other document
  requires an edit (§15).

## Negative tests

1. Constructing an `ObligationTransition` to `Completed` with no connected `ExecutionRecord`, or
   with only `Abstained`/`Malformed`/`ProviderFailure` records, is rejected by
   `ReviewAggregate::validate` under the v2 contract.
2. A `ReviewClaim` whose `execution_id` names a non-existent `ExecutionRecord` is rejected under v2.
3. A `ReviewClaim` whose `execution_id` names an `ExecutionRecord` with `outcome: Abstained` (or
   `Malformed`/`ProviderFailure`) is rejected under v2.
4. A `ReviewClaim` whose `execution_id` names a `Completed` `ExecutionRecord` that does not list
   that claim's ID in `parsed_claim_ids` is rejected under v2.
5. The fake reviewer's fixture set exercises all 8 `AbstentionReason` values and all 6
   `MalformedOutputReason` values; each produces the correct `ExecutionOutcome` variant and leaves
   the obligation's lifecycle untouched.
6. A structured claim citing a `source_ids` entry outside its envelope's
   `normalized_included_source_ids` is rejected as `MalformedOutputReason::UnresolvedSourceId`,
   never silently dropped from the set and never accepted.
7. A malformed reviewer response (unknown field, missing source refs, invalid polarity enum,
   confidence outside `[0.0, 1.0]`) never produces a `ReviewClaim`, and never produces an
   `issue_absent` claim as a fallback.
8. Two `ExecutionRecord`s for the same obligation/envelope/plan/wave/reviewer/model/prompt/
   tool-policy tuple but different `attempt` values receive distinct IDs; identical `attempt` with
   identical content is idempotent (no collision); identical `attempt` with different content is
   `DomainError::IdCollision`.
9. An `EnvelopeLoss` with an empty `source_ids` is accepted (unlike `Limitation`); an
   `EnvelopeLoss`-eligible omission with zero declared losses is rejected.
10. Replaying a pre-M3 fixture event stream (the existing M1 fixtures, six payload kinds only,
    tagged `reviewgraphen.review_event.v1`) under the M3 build produces byte-identical
    `ReviewAggregate`/`Coverage` output to the pre-M3 build, and is never subjected to execution-
    closure validation.
11. `ReviewPlan` construction from a fixed universe and budget is deterministic across repeated
    runs (byte-identical wave assignment, `risk_breakdown`, and `deferred` map) and never removes a
    deferred obligation from `UniverseDescriptor`.
12. A `Reviewer` implementation cannot cause the trait boundary to invoke any capability outside
    `tool_policy_version`'s declared (empty, in M3) set; a fixture that attempts to name an
    additional tool in `requested_evidence` is rejected at the parser layer (§10/§13), not silently
    accepted.
13. A `ReviewExecutionRecorded` event whose `execution.parsed_claim_ids` does not exactly equal the
    submitted `claims[*].id` set is rejected in full — neither the execution record nor any of the
    claims becomes visible in the resulting aggregate.
14. A `ReviewExecutionRecorded` event with `outcome: Completed` and an empty `claims` vector is
    rejected. A `ReviewExecutionRecorded` event with `outcome: Abstained` (or `Malformed`/
    `ProviderFailure`) and a non-empty `claims` vector is rejected.
15. An `ExecutionRecord` whose `plan_id`/`wave_id` does not resolve within a recorded
    `ReviewPlanRecorded`, or whose `obligation_ids` is not a subset of that wave's `obligation_ids`,
    is rejected.
16. A single imported event sequence containing both a `reviewgraphen.review_event.v1`-tagged and a
    `...v2`-tagged envelope is rejected by `EventEnvelope::validate_sequence` before either is
    applied.

## Crate ownership

| Crate | New M3 responsibility |
| --- | --- |
| `reviewgraphen-core` | `ReviewContextEnvelope`, `EnvelopeLoss`, `SourceArtifactRef`, `ExcludedSourceRef`, `ExcerptRange`, `ReviewPlan`/`ScheduleWave`/`PlanBudget`/`RiskDescriptor`, `ExecutionRecord`, `ExecutionOutcome`, `AbstentionReason`, `MalformedOutputReason`, the three new `PersistedPayload` variants and their v2-only validation extensions in this ADR, and the `v1`/`v2` schema-tag dispatch in `ReviewAggregate::validate`/`EventEnvelope::validate`. Remains the sole crate that constructs/validates a `ReviewAggregate`. |
| `reviewgraphen-reviewer` (new) | `Reviewer` trait, `ReviewerRequest`/`ResolvedSourceInput`/`ReviewerResponse`, deterministic fake reviewer, structured reviewer-output parser (both validation layers), abstention/malformed-output classification, prompt-injection boundary checks. Depends only on `reviewgraphen-core`. |
| `reviewgraphen-ingest` | Unchanged by this ADR. |

## M3 exit criteria

M3 is complete when, in addition to `docs/18` §4's own M3 exit criteria (Envelope carries
obligation/source IDs/included-excluded/unknowns/loss; raw prose never canonical state; malformed
response is a typed failure, not a fabricated claim; the same Envelope is reproducible/auditable;
repository-embedded prompt injection is never executed as an instruction):

1. Every invariant in this ADR's Invariants section has a corresponding passing test.
2. Every test in this ADR's Negative tests section passes.
3. `Coverage::from_aggregate`'s `raw` ("completed") measure, run against a fixture exercising all
   four `ExecutionOutcome` kinds, counts only the `Completed` obligations — proving the docs/11
   correction is not just a documentation change but an enforced property.
4. `docs/11_coverage_and_scheduling.md` §3.4 reads the corrected text and no other section of
   `docs/07`, `docs/09`, or `docs/11` requires a further edit to stay mutually consistent.
5. The deterministic fake reviewer is the only wired `Reviewer` implementation; no real provider,
   subprocess, or parallel coordinator exists yet (§1's deferral list is intact, not silently
   implemented ahead of its own ADR).
6. `reviewgraphen-reviewer` compiles and tests independently of `reviewgraphen-ingest` and of any
   future `reviewgraphen-store` (no accidental dependency introduced).
7. ADR 0014's CAS, `SnapshotSourceBundle`, and `RunGenesisManifest` are available for every v2 run
   this ADR's Envelope builder/plan/execution pipeline produces — this ADR does not require ADR
   0014 to ship as a separate, earlier milestone, but the two are accepted together and neither's
   fixtures may substitute a v1-era shortcut for the other's v2-era contract.
8. A v1 fixture stream (the pre-M3 fixtures) replays under the M3 build with byte-identical
   `ReviewAggregate`/`Coverage` output to the pre-M3 build, and a v2 fixture stream enforces every
   invariant above; the two are exercised by distinct tests, not conflated.
9. ADR 0014 §7's derived-index rebuild projects `ReviewPlanRecorded` into its `review_plans` table
   on the same footing as `ContextEnvelopeProjected`/`ReviewExecutionRecorded` — this ADR's plan
   record is never silently excluded from the rebuildable audit index ADR 0014 defines.

## Revisit triggers

- RG-503 (weighted coverage scheduler) is implemented: revisit whether `RiskDescriptor`'s baseline
  bucket mapping and fixed `likelihood: Medium` (§5a), and `ReviewPlan`'s deterministic ordering
  tiebreak rule (`StableId`, §5b), should become documented, versioned scoring-policy fields instead
  of the current fixed baseline.
- RG-603/RG-604 grant a reviewer its first real tool or provider capability: revisit
  `tool_policy_version`'s fixed-empty M3 policy and the prompt-injection boundary's current
  "no side channel exists" argument, which assumed zero capabilities.
- A second loss-shaped record beyond `EnvelopeLoss` and `program::Limitation` is proposed: revisit
  Alternative D/§5's decision to keep them distinct.
- Incremental review (M6) needs `ReviewContextEnvelope` materiality comparison: revisit deferring
  RG-507 and `projection_hash`'s currently-unused status.
- A third schema-tag generation becomes necessary: revisit whether `EventEnvelope`/`ReviewAggregate`
  should generalize the current pairwise v1/v2 dispatch (§2/§7) into an explicit, extensible
  contract-version table instead of a hand-written two-branch check.
