# ReviewGraphen agent handoff

- Updated: 2026-08-09 (Asia/Tokyo)
- Workspace: `/Users/rizumita/Workspace/reviewgraphen`
- Branch: `main`
- Purpose: operational handoff for the next coding agent. This file is intentionally not part of
  the product contract; remove it after takeover unless the user asks to retain it.

## 1. Read first

Follow `AGENTS.md`. In particular, read these before changing code:

1. `README.md`
2. `docs/00_vision_and_scope.md`
3. `docs/03_conceptual_model.md`
4. `docs/04_highergraphen_mapping.md`
5. `docs/05_system_architecture.md`
6. `docs/adr/0013-m3-plan-context-and-reviewer-contract.md`
7. `docs/adr/0014-snapshot-source-cas-and-durable-replay-admission.md`

Also consult ADR 0004, 0005, 0007, 0009, 0011, and 0012 for context projection, claim/evidence
boundaries, event storage, the Rust harness, ProgramSpace v2, and Cargo admission.

Do not use CaseGraphen for the remaining work. The user explicitly allowed abandoning it after its
JSON/audit workflow consumed too much time. No CaseGraphen Memory Plane state is required.

## 2. User working preferences and constraints

- Implementation should be delegated to Claude CLI using Sonnet with `--effort high`.
- Use independent subagents for contract, security, and implementation audits.
- Keep implementation changes narrow and commit one design/contract decision at a time.
- Manage context proactively; do not reload unrelated docs or repeat already-completed work.
- The ingestion runtime must never discover or invoke mise/rustup/PATH internally.
- Mise is an external harness only. Setup is an explicit human/host step (`mise install`).
- The harness resolves the already-installed real Cargo binary and passes its absolute path as
  `CargoToolAdmission::TrustedExecutable`.
- Do not silently install toolchains or dependencies during tests.

## 3. Current Git state

Relevant completed commits:

| Commit | Result |
| --- | --- |
| `831ac31` | Deterministic M0/M1 review core |
| `a285475` | ProgramSpace v2 capability trace contract |
| `e1a9007` | Bounded Rust/Git M2 ingestion plus mise Cargo-admission harness |
| `bad010f` | Core Clippy constructor lint exceptions, locally documented |
| `ad6b6bc` | Accepted M3 planning/context/reviewer and durable-store ADRs |

At handoff, the following pre-existing working-tree changes belong to the user and must not be
edited, staged, restored, or included in a commit:

```text
 M README.md
 M docs/18_mvp_roadmap.md
 M scripts/ci.sh
 M scripts/validate_bundle.py
?? cases/
?? docs/adr/0010-mvp-case-execution-contract-foundation.md
```

The user subsequently clarified that `cases/` is unnecessary. Treat the entire CaseGraphen/MVP
Case proposal set as excluded local material, not as an implementation input or publish target.
The directly coupled uncommitted changes (`README.md`, `docs/18_mvp_roadmap.md`, ADR 0010,
`scripts/ci.sh`, and `scripts/validate_bundle.py`) are likewise excluded because they reference or
validate that case bundle. They remain local only so this handoff does not destructively discard
someone else's work; do not stage them unless the user gives a new explicit instruction.

No post-`ad6b6bc` implementation was started.

Before each commit, verify the staged set explicitly with:

```sh
git diff --cached --name-only
git diff --cached --check
```

## 4. Phase status

1. Rust development harness: complete.
2. M0/M1 deterministic domain and event core: complete.
3. M2 bounded Git/Rust ingestion and trusted Cargo admission: complete.
4. Core strict-Clippy cleanup: complete.
5. M3 ADR and contract closure: complete and independently audited; no remaining blocker was found.
6. M3 source handoff, event v2, CAS/log/index store: not implemented.
7. M3 deterministic planner/context and fake reviewer/parser: not implemented.
8. M3 workspace verification and final audit: not started.

The next coding unit should be source handoff only, followed by event v2, then the store. Do not try
to implement all of M3 in one Claude session.

## 5. Next implementation units

### Unit A: bounded snapshot-source handoff

Implement this first and commit it independently.

- Add `SnapshotSourceBundle` and `SnapshotSourceEntry` to `reviewgraphen-core` with validated,
  deterministic construction and accessors.
- Keep `reviewgraphen_ingest::ingest(&IngestRequest) -> IngestResult` unchanged.
- Add:

  ```rust
  pub fn ingest_with_sources(
      request: &IngestRequest,
      max_total_source_bytes: u64,
  ) -> Result<IngestWithSourcesResult, IngestError>;
  ```

- Route both public functions through one private ingestion pipeline.
- For `ingest_with_sources`, thread the aggregate limit through `git::load_snapshot`'s existing
  file-read loop. Count actual bytes and stop before the next `git show` after the limit is exceeded.
- Reuse the one already-read blob for parsing and bundling; never read a blob twice.
- Allow zero-byte tracked regular files.
- Require source entries to match the accepted ProgramSpace file-artifact set exactly.
- Current M2 `content_hash` is SHA-256 of the bytes, not Git blob SHA-1.
- Add boundary/determinism tests: zero bytes, exact limit, over limit with no partial result,
  missing/extra/duplicate artifact, path/hash mismatch, stable order, same-input byte equality, and
  proof that the bundle and parser consumed the same read.

### Unit B: event v2 and typed genesis/source records

Implement and commit separately from Unit A.

- Introduce `EventContractVersion::{V1, V2}` and store it in `EventLog`.
- New logs mint v2; imported v1 remains read-only. One stream cannot mix schema tags.
- Pass the selected schema into `event_id`/`envelope_hash`; do not replace the global constant in a
  way that invalidates legacy v1 hashes.
- V1 genesis hash remains SHA-256 over the old canonical `ReviewAggregate` serialization.
- V2 genesis hash is SHA-256 over canonical `RunGenesisSnapshot` bytes.
- Implement the versioned `RunGenesisSnapshot` decoder/validator and `EventLog::new_v2` equivalence
  check described in ADR 0014.
- Add the three ADR 0014 payloads: `RunGenesisManifest`, `ArtifactRegistered`, and
  `SnapshotSourcesRecorded`.
- Add private-constructor `ValidatedEventView`, typed `DecodedPayload`, and
  `OfflineProjectionState`. Chain/shape validity is not aggregate acceptance.
- Add aggregate maps for genesis, registrations, and snapshot sources.
- Persist source `registration_id`, `cas_hash`, and `line_count`; validate context excerpts without
  reading CAS bytes.
- Preserve authority boundaries. `EvidenceRecorded` is gated by `EvidenceAdmission`; store code may
  not mint any admission.

### Unit C: durable store

Create `reviewgraphen-store` as a new workspace crate depending on core only.

- Exact SQLite dependency required by ADR 0014:

  ```toml
  rusqlite = { version = "=0.40.1", features = ["bundled"] }
  ```

- Choose safe wrapper dependencies for FD-relative/no-follow opens, locks, and atomic no-replace.
  Workspace lint forbids unsafe code, so do not add local `unsafe` blocks. Research and pin current
  primary-source-supported versions before adding new crates (likely a safe `rustix`/capability
  wrapper; no choice has been committed yet).
- Implement `StoreRoot`, exact `CasHash`, bounded streaming CAS, atomic no-replace publication,
  owner/mode checks, lease-aware temp GC, shared-reader/exclusive-transaction log locking, durable
  rollback, typed corruption, and atomic recovery intent/completion files.
- SQLite is derived state: seed v2 baseline tables from the validated genesis CAS artifact, project
  event rows through `OfflineProjectionState`, mark authority rows unreconciled, and compare
  canonical `IndexSnapshot` query state rather than SQLite bytes.
- Hold the log shared lock from tail comparison through a current-state query to close the append
  TOCTOU.
- V1 supports event-metadata-only rebuild; full domain rebuild requires a fresh v2 import.

### Unit D: deterministic planning/context/reviewer

Implement after the storage contracts exist.

- Core: `ReviewPlan`, waves/budget/risk descriptor, full `ReviewContextEnvelope`, source refs,
  exclusions/losses, execution records, outcomes, and atomic execution+claims event.
- Envelope candidate set for M3 is every accepted `file:*` artifact in StableId order.
- Envelope ID and projection hash bind the same full canonical projection body, including
  obligation IDs, candidate/included/excluded sources, excerpts, reasons, unknowns, assumptions,
  and losses.
- Execution records carry both `raw_artifact_registration_id` and `raw_artifact_hash`.
- Reviewer crate depends on core only. Use `ReviewerRequest` with verified full artifact bytes and
  expose only selected excerpts; return `ReviewerResponse { raw_artifact, outcome }`.
- Implement `ReviewerDescriptor`, deterministic fake reviewer, structured parser, abstention and
  malformed-output taxonomies, and prompt-injection boundary tests.
- No real provider, subprocess reviewer, parallel coordinator, or weighted scheduler in M3.

## 6. Critical invariants to keep visible

- Program facts, review claims, and evidence remain separate records.
- Confidence never promotes a claim to accepted/verified/human-accepted.
- `reviewed`, `evidence_supported`, `verified`, and `human_accepted` are distinct.
- Coverage always records snapshot/profile/rule/extractor versions and an explicit denominator.
- Projection declares source IDs and meaningful information loss.
- Stale evidence cannot sign off a current snapshot.
- Local review success cannot imply global safety without gluing checks.
- LLM output is never a parser/symbol-resolver substitute or raw canonical state.
- Reviewer tools do not get arbitrary shell access.
- A valid abstention advances only `visited`, never `completed`.
- `ReviewExecutionRecorded` applies execution and claims atomically.
- Store chain validity alone does not mean semantic/authority reconciliation.

## 7. Harness and verification commands

The project toolchain is pinned to Rust 1.95.0. Do not run `mise install` automatically. The host
must perform that setup explicitly if it has not already been done.

Use this environment for every harness invocation:

```sh
export MISE_TRUSTED_CONFIG_PATHS=/Users/rizumita/Workspace/reviewgraphen/mise.toml
export MISE_AUTO_INSTALL=false
export MISE_EXEC_AUTO_INSTALL=false
export MISE_TASK_RUN_AUTO_INSTALL=false
export MISE_NOT_FOUND_AUTO_INSTALL=false
export MISE_OFFLINE=true
```

Ingestion's end-to-end trusted-Cargo tests:

```sh
mise run test-ingest
```

Workspace gates (run through the installed mise toolchain, with no auto-install):

```sh
mise exec rust@1.95.0 -- cargo fmt --all -- --check
mise exec rust@1.95.0 -- cargo check --workspace --all-targets
mise exec rust@1.95.0 -- cargo clippy --workspace --all-targets --all-features -- -D warnings
mise exec rust@1.95.0 -- cargo test --workspace --all-targets
```

Last known verification before the M3 ADR commit:

- Core unit tests: 2 passed.
- Core integration tests: 94 passed.
- Ingest suite via `mise run test-ingest`: 105 passed.
- Formatter, check, and strict Clippy passed after commit `bad010f`.

Re-run the relevant subset after every implementation unit and the full set before each milestone
commit.

## 8. Claude CLI operational note

Claude CLI itself is installed (`2.1.226` at handoff) and a minimal Sonnet/high request succeeded.
Use the exact mise config file in `MISE_TRUSTED_CONFIG_PATHS`; using only the directory caused a
Claude session hook to report the config as untrusted.

Recommended non-interactive shape:

```sh
MISE_TRUSTED_CONFIG_PATHS=/Users/rizumita/Workspace/reviewgraphen/mise.toml \
MISE_AUTO_INSTALL=false MISE_OFFLINE=true \
claude --model sonnet --effort high \
  --permission-mode acceptEdits \
  --allowedTools 'Read,Edit,Write,Grep,Glob' \
  --no-session-persistence --output-format json \
  -p '<one bounded implementation unit>'
```

Large prompts caused long silent sessions and, once, a delayed edit after the wrapper appeared
stalled. Keep each session to one unit/file family, inspect file mtimes and `git diff`, and ensure no
old Claude session is still editing before launching another. Do not let concurrent implementers
touch the same files. Claude should not commit; the primary agent must inspect, test, independently
audit, stage exact paths, and commit.

## 9. Last completed audit

Two independent subagents audited ADR 0013/0014. Their final result was PASS with no implementation
blocker after these clarifications:

- V1/V2 genesis hash formulas and repository identity cross-check.
- Typed genesis reconstruction and semantic offline projection.
- Source registration/CAS/line-count trace.
- Raw reviewer artifact registration and sensitivity/source binding.
- Atomic recovery records and durable append rollback.
- Aggregate source bound, index query lock TOCTOU, exact projection identity, deterministic candidate
  set, and concrete `ReviewerDescriptor`.

Do not reopen those design questions unless implementation evidence contradicts the accepted ADR.
