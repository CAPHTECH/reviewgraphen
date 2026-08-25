# Implementation decomposition for terra/high

Target: Candidate A, **Unsafe-boundary Relation-to-Report**. These are proposed
units only; no implementation was performed in this audit.

## 1. File ownership and sequencing rule

Each path below has exactly one owner. No two units edit the same file. Later
integration units consume earlier public APIs rather than patching their files.
If implementation discovers that a listed owner cannot complete without editing
another unit's file, stop and re-cut the plan before parallel work; do not make a
cross-owner “small fix.” New filenames are proposals and should be frozen by U0.

Every unit preserves Program fact / Review claim / Evidence / Verification /
Decision separation. “Passing test,” model confidence, structured output, and
report generation never imply acceptance.

Verification abbreviations used below:

- **F** `cargo fmt --all -- --check`
- **C** `cargo clippy --workspace --all-targets -- -D warnings`
- **U** focused unit/integration tests
- **S** `python3 scripts/validate_bundle.py` plus new schema examples/mutations
- **D** same-input stable-ID/canonical-byte determinism test
- **N** negative fixture: parse/unknown/stale/timeout/tamper/limit as applicable

## 2. Work units

### U0 — Freeze ADR and public contract map

- **Purpose:** decide exact fact/relation/property/profile/schema versions,
  authority ceiling, verifier threat model, migration, and supersession before
  public code changes.
- **Changes only:**
  `docs/adr/0038-unsafe-boundary-generic-review-slice.md` (new).
- **Must not change:** all `crates/`, `schemas/`, existing ADRs, examples,
  fixtures, benchmark records.
- **Contract/ADR:** the ADR itself must normatively define:
  `explicit_unsafe_syntax` fact meaning; `unsafe_boundaries` capability;
  `contains_unsafe` relation; `relation.changed_unsafe_boundary@1`;
  `rust.unsafe_boundary_contract@1`; context policy v2; deterministic observer;
  verifier descriptor/mount/argv/env/limit contract; generic request/run/report
  schema versions; v1 read compatibility; no automatic promotion.
- **Required verification:** Markdown/link check; review against ADR 0003/0004/
  0005/0016/0021/0029/0030 and AGENTS boundaries.
- **Done when:** every new stable ID/hash input, source role, loss record,
  capability downgrade, typed failure, and schema migration is closed; unresolved
  choices appear as explicit rejection/deferral rather than prose ambiguity.
- **Dependency / parallel:** first and alone. All later units depend on Accepted
  U0.

### U1 — Extract exact unsafe syntax facts

- **Purpose:** deterministically reify explicit unsafe function/block/impl spans
  and `contains_unsafe` relations from unexpanded `syn` syntax.
- **Changes only:**
  `crates/reviewgraphen-ingest/src/rust.rs`,
  `crates/reviewgraphen-ingest/tests/unsafe_boundary.rs` (new).
- **Must not change:** Core synthesis/context, reviewer, verifier, runtime, CLI,
  schemas, docs, existing m2 fixtures.
- **Contract/ADR:** U0 fact/relation shape; extractor version bump and migration;
  capability complete only for the admitted syntax tree, partial on parse
  failure; macro-generated unsafe remains unknown/obstructed.
- **Required verification:** **F C U D N**. Positive cases: unsafe fn, block,
  impl, nested block, method, changed/nonchanged. Negative cases: safe code,
  macro token containing `unsafe`, parse failure, same span/order mutation,
  unrelated method name, ID collision. Repeat ingest bytes/IDs exactly.
- **Done when:** no fact says “unsound,” source paths/spans/hashes are bound,
  changed overlap reaches the correct relation without mutating symbol-owned
  base facts, and all capability limitations have source-backed obstructions.
- **Dependency / parallel:** after U0; parallel with U3/U4/U5.

### U2 — Materialize the general Relation obligation

- **Purpose:** add one rule over accepted unsafe relations and preserve the full
  universe denominator/capability-gap semantics.
- **Changes only:**
  `crates/reviewgraphen-core/src/synthesize.rs`,
  `crates/reviewgraphen-core/tests/unsafe_relation_obligation.rs` (new).
- **Must not change:** ingest, context, runtime, schemas, fixture
  `double-submit-payment`, historical rule meanings/versions.
- **Contract/ADR:** U0 rule/property/target kind; rule-set/profile version; no
  mutation of `node.changed_public_symbol@2`; stable ID includes snapshot,
  profile, rule, extractor, relation target and source closure.
- **Required verification:** **F C U D N**. Cover changed vs unchanged, complete
  vs partial/missing capability, exclusion, parse/macro unknown, duplicate/nested
  relations, order permutation, self-diff control, and fixture backward
  compatibility. Capability gap remains an obligation, not an exclusion.
- **Done when:** a hand-built ProgramSpace with the U1 fact creates exactly the
  new applicable Relation obligation; absent/partial facts never synthesize a
  false applicable target; all old canonical fixtures either remain byte-stable
  under v1 or migrate under the U0-declared major/profile revision.
- **Dependency / parallel:** after U1 fact shape is frozen. Can run while U3/U4/
  U5 finish; no shared files.

### U3 — Subject-first multi-window context policy

- **Purpose:** guarantee that every admitted relation endpoint/unsafe region
  subject is present or explicitly excluded/unknown, then allocate remaining
  budget to support anchors as deterministic disjoint windows.
- **Changes only:**
  `crates/reviewgraphen-core/src/context.rs`,
  `crates/reviewgraphen-core/tests/context_subject_windows.rs` (new).
- **Must not change:** synthesizer, ingest, runtime, source registration, old
  policy semantics in place.
- **Contract/ADR:** additive `context.subject_windows@2`; window identity/order,
  merge rule, per-file/window caps, source-ID representation, recovery refs,
  meaningful loss when a subject cannot fit. Do not silently change baseline@1.
- **Required verification:** **F C U D N**. Reproduce the >400-line displaced-
  subject case from `docs/24`; scattered anchors, overlapping/adjacent windows,
  giant line, file/byte/window cap +1, unparented range artifact, unresolved
  relation endpoint, reordered ProgramSpace, source/CAS drift. Property test
  requires every target to be included or named in excluded/unknown/loss.
- **Done when:** same aggregate/obligation/policy yields identical windows and
  projection hash; no lower-priority support anchor can evict a subject without
  a typed record; v1 policy remains readable/replayable.
- **Dependency / parallel:** after U0; parallel with U1/U4/U5.

### U4 — Closed workspace verifier descriptor

- **Purpose:** provide one generic, code-owned offline Cargo-test descriptor and
  typed `unsupported`, `inconclusive`, `failed`, `passed`, `timeout`, and stale
  observations without exposing arbitrary shell.
- **Changes only:**
  `crates/reviewgraphen-verifier/Cargo.toml`,
  `crates/reviewgraphen-verifier/src/lib.rs`,
  `crates/reviewgraphen-verifier/tests/workspace_descriptor.rs` (new).
- **Must not change:** reviewer process sandbox, runtime/store authority,
  generic request/run schemas, M4 descriptor IDs/procedures.
- **Contract/ADR:** U0 descriptor registry. Executable and toolchain identity are
  trusted code/config inputs; argv is one fixed vector, never a shell string;
  cwd is canonical and inside the admitted workspace; repo read-only;
  `CARGO_TARGET_DIR` is a fresh bounded writable mount; network/env/process/time/
  memory/output limits are closed and recorded. Result is evidence material, not
  an accepted claim.
- **Required verification:** **F C U D N**. Reject unknown descriptor,
  executable/argv/env/cwd/path traversal/symlink/network attempts, workspace
  writes, output/CPU/wall/process cap +1, non-UTF8, missing lock/dependency,
  stale snapshot/tool identity, wrong property/subjects. Prove reviewer prose
  and `requested_evidence` cannot select command data.
- **Done when:** all hostile inputs refuse before process start or return a typed
  non-passing result; an admitted temporary Rust workspace produces a
  hash-bound result; no API can append evidence/verification/decision itself.
- **Dependency / parallel:** after U0; parallel with U1/U3/U5.

### U5 — Generic deterministic observer

- **Purpose:** add a repo-independent provider-free observer that emits one
  canonical typed abstention for any valid single-obligation request; preserve
  existing real adapters unchanged.
- **Changes only:**
  `crates/reviewgraphen-reviewer/src/lib.rs`,
  `crates/reviewgraphen-reviewer/src/deterministic.rs` (new),
  `crates/reviewgraphen-reviewer/tests/deterministic_observer.rs` (new).
- **Must not change:** `src/process.rs`, generic runtime/request schema, fake M4
  fixture responses, Core/Store/report.
- **Contract/ADR:** new observer ID/prompt/tool-policy versions; no fixture key,
  source-content branching, I/O, model, tool, evidence, or authority capability.
- **Required verification:** **F C U D N**. Arbitrary valid obligation IDs/source
  bytes lead to the same closed abstention class with caller-fixed execution ID;
  invalid closure/oversize refuses; repeat raw bytes/hash exactly; cannot emit a
  claim, evidence, or accepted state.
- **Done when:** it is a public bounded API suitable for runtime selection and
  is demonstrably not `FakeReviewer`'s named-fixture lookup.
- **Dependency / parallel:** after U0; parallel with U1/U3/U4.

### U6 — Generic run v2 orchestration and audit schema

- **Purpose:** compose U2/U3/U4/U5 with existing live/replay adapters and emit
  one canonical non-authority audit record including typed verification attempts
  and report-source data.
- **Changes only:**
  `crates/reviewgraphen-runtime/Cargo.toml`,
  `crates/reviewgraphen-runtime/src/generic.rs`,
  `schemas/reviewgraphen.generic_review_request.v2.schema.json` (new),
  `schemas/reviewgraphen.generic_review_run.v2.schema.json` (new),
  `schemas/examples/generic-review-request-v2.json` (new),
  `schemas/examples/generic-review-run-v2.json` (new).
- **Must not change:** ingest/Core/reviewer/verifier implementations, Store
  events, report/CLI, v1 schema files, old fixtures.
- **Contract/ADR:** U0 v2 DTO; request selects rule/profile and observer only
  from closed enums; verifier selection is profile-owned; executions, claims,
  evidence candidates, verification results, staleness, and absent human
  decisions are distinct arrays/records; authority remains fixed false.
- **Required verification:** **F C U S D N**. Ordinary temporary Git changes with
  unsafe relation and no-unsafe control; deterministic observer fresh run and
  byte-identical replay; structured live-record injection and abstention;
  unsupported/nonzero/timeout/stale verifier; malformed model; deferred
  obligation; target/loss closure; schema mutation of every category; forged
  `trusted_pass`/accepted/denominator/source rejected.
- **Done when:** a provider-free ordinary Git base/head yields complete schema-
  valid audit bytes outside the repo; a replay rebuild is identical; no generic
  path appends durable Store authority or converts verifier success to human
  acceptance.
- **Dependency / parallel:** after U2/U3/U4/U5. Sole integration owner; not
  parallel with its prerequisites.

### U7 — Short non-authority Markdown projection

- **Purpose:** render a PR-usable, bounded Markdown view from a validated generic
  run while retaining a hash/reference to the canonical audit JSON.
- **Changes only:**
  `crates/reviewgraphen-report/Cargo.toml`,
  `crates/reviewgraphen-report/src/lib.rs`,
  `crates/reviewgraphen-report/src/generic_non_authority.rs` (new),
  `crates/reviewgraphen-report/tests/generic_non_authority.rs` (new),
  `schemas/reviewgraphen.generic_review_human_report.v1.schema.json` (new if a
  JSON report manifest is required by U0).
- **Must not change:** authority-bearing V2–V5 report modules/schemas, runtime,
  CLI, Store/index/CAS code.
- **Contract/ADR:** projection-only: headings for proposed claims, abstentions,
  verification status, excluded/unknown/unresolved/loss, denominator coverage,
  authority warning, audit hash. Source IDs are mandatory; omitted audit fields
  have meaningful loss/recovery refs. Markdown cannot be imported as state.
- **Required verification:** **F C U S D N**. Snapshot tests for zero claim,
  proposal, abstention, malformed/provider failure, unsupported/inconclusive/
  stale verifier, deferred obligation, large bounded lists, Unicode/control
  escaping, tampered audit hash, forged authority. Repeat bytes exactly.
- **Done when:** report is short under the frozen byte/row bound, cites exact
  source IDs, never says accepted/verified when only reviewed/evidence-supported,
  and points to the canonical audit record for every omitted detail.
- **Dependency / parallel:** after U6. It consumes U6 without editing it.

### U8 — One CLI workflow and checked-in ordinary quickstart

- **Purpose:** expose one command that atomically creates audit JSON, Markdown,
  packets, process/verifier artifacts in a fresh output root for deterministic,
  Codex, Claude, or replay mode.
- **Changes only:**
  `crates/reviewgraphen-cli/Cargo.toml`,
  `crates/reviewgraphen-cli/src/lib.rs`,
  `crates/reviewgraphen-cli/src/main.rs`,
  `crates/reviewgraphen-cli/tests/generic_quickstart.rs` (new),
  `examples/generic-rust-review/README.md` (new),
  `examples/generic-rust-review/request.json` (new),
  `examples/generic-rust-review/repository/` (new ordinary two-commit source
  recipe/fixture as U0 permits; no hidden product branch).
- **Must not change:** runtime/report/verifier internals, fixed double-submit
  example, schemas, docs/ADR, benchmark records.
- **Contract/ADR:** exact command/exit codes/output filenames; fresh output root;
  write-temp/fsync/rename or typed partial-artifact semantics; stdout/stderr
  contract; deterministic mode is generic and not selected by repo contents or
  fixture name.
- **Required verification:** **F C U S D N**. Clone-style deterministic command,
  second-run existing-output refusal, output path inside repo/symlink/traversal
  refusal, no-unsafe control, provider failure, replay mismatch, report/audit
  hash match, fixed-fixture command still rejected. CLI integration compares
  exact canonical bytes on two fresh roots.
- **Done when:** documented commands work without provider credentials and
  produce both artifacts; the same request can switch to a real adapter without
  changing the ingest/rule/context/report path.
- **Dependency / parallel:** after U6/U7. Sole CLI/example owner.

### U9 — Documentation, reference scenario, and DoD reconciliation

- **Purpose:** make clone reproduction and current capability limits accurate;
  update public status only after U8 evidence exists.
- **Changes only:**
  `README.md`, `docs/13_cli_contract.md`, `docs/14_report_and_schema_contract.md`,
  `docs/16_security_and_trust_boundary.md`,
  `docs/23_current_capability_status.md`, `schemas/README.md`.
- **Must not change:** all Rust/schema implementations and examples, ADR
  decision text, benchmark/preregistration results.
- **Contract/ADR:** document exact versioned request/run/report files, fake vs
  real mode, authority ceiling, unsafe syntax limitation, verifier sandbox,
  reproduction commands, expected hashes/exit codes, and known unsupported
  cases. Historical benchmark results remain append-only and are not
  retroactively reinterpreted.
- **Required verification:** **F C U S D N** via the exact README command in a
  clean throwaway clone/worktree; link checker; compare documented output IDs and
  hashes to generated fixtures; negative quickstart case.
- **Done when:** a third party with pinned Rust/tool prerequisites can follow the
  docs to audit JSON + Markdown without credentials; current status distinguishes
  code-reconfirmed, externally reproduced, and not evaluated claims.
- **Dependency / parallel:** last, after U8. No parallel code edits.

## 3. Dependency waves

```text
Wave 0: U0
Wave 1: U1  U3  U4  U5       (parallel; disjoint files)
Wave 2: U2                    (after U1; may overlap in time with unfinished U3/U4/U5)
Wave 3: U6                    (after U2/U3/U4/U5)
Wave 4: U7
Wave 5: U8
Wave 6: U9
```

Each wave ends with `git diff --check`, F/C as relevant, focused tests, and a
read-only check that no unrelated dirty/untracked file changed. A final
workspace test/schema pass is necessary but does not replace each unit's focused
negative and determinism evidence.

## 4. Final completion gate

The implementation is not complete merely because the quickstart produces a
finding. Completion requires all ten units, formatter/clippy/unit/schema/bundle
validation, stable-ID and canonical replay equality, ordinary no-unsafe and
unsafe repositories, stale/unknown/unverifiable/gluing-not-claimed cases,
subject/loss source trace, fixed verifier attack refusals, report/audit hash
closure, docs reproduction, and zero automatic promotion of a model proposal.
The population efficacy claim remains separate and is governed only by the
frozen preregistration.
