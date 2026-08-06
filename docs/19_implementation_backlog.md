# 19. Implementation Backlog

> Status: Draft v0.1  
> Scope: MVPまでの実装可能なbacklog  
> Priority: `P0` = vertical sliceに必須、`P1` = MVP完成に必要、`P2` = 評価・拡張

## 1. Backlog運用規則

- Task IDはstableに保つ。
- 一つのtaskは一つの検証可能な成果へ対応させる。
- schema、fixture、test、docsを別taskへ分離しすぎない。
- accepted/inferred/verified境界を変更するtaskには必ずnegative testを含める。
- implementation detailが変わってもacceptance criteriaの意味を保つ。
- 完了判定は「コードを書いた」ではなくobservable contractで行う。

## 2. Dependency overview

```text
RG-000 Contracts
   ├─ RG-100 Core model
   ├─ RG-200 Store
   └─ RG-300 Program ingestion
          └─ RG-400 Obligation synthesis
                 └─ RG-500 Context projection
                        └─ RG-600 Review execution
                               └─ RG-700 Evidence verification
                                      ├─ RG-800 Coverage / gate
                                      ├─ RG-900 Gluing
                                      └─ RG-1000 Incremental review
                                             └─ RG-1100 Evaluation

RG-1200 Security / release spans all epics.
```

## 3. RG-000 — Contract baseline

### RG-001 [P0] Canonical ID grammar

Define ID kinds, normalization, namespace, snapshot binding, collision handling.

**Acceptance criteria**

- Equivalent input yields identical IDs.
- Different snapshots cannot accidentally alias snapshot-bound objects.
- IDs can be serialized without provider-specific fields.
- Collision is a typed error, not last-write-wins.

### RG-002 [P0] Canonical JSON rules

Define object key ordering, set-like list ordering, number and timestamp normalization.

**Acceptance criteria**

- Canonical serialization is byte-stable.
- Hash fixture is checked into tests.
- Unknown optional fields do not change existing canonical fields silently.

### RG-003 [P0] Schema fixtures

Implement validation for input, obligation bundle, report.

**Acceptance criteria**

- Positive fixtures validate.
- Missing source IDs, invalid confidence, invalid coverage counts fail.
- Cross-record validation failures are reported separately from JSON Schema failures.

### RG-004 [P0] State transition table

Encode obligation lifecycle, claim disposition, verification outcome.

**Acceptance criteria**

- Illegal transitions fail with typed reason.
- `completed` does not imply `supported` or `verified`.
- stale and superseded history is retained.

## 4. RG-100 — Core domain model

### RG-101 [P0] Artifact/ProgramSpace references

Implement language-neutral cell, relation, context, invariant, source and provenance records.

### RG-102 [P0] ReviewProfile and ReviewRule contracts

Implement version/hash bound profile and rule descriptors.

### RG-103 [P0] ReviewObligation aggregate

Implement target, property, context requirement, evidence requirement, risk, applicability, provenance, version tuple.

### RG-104 [P0] ReviewExecution and ReviewClaim

Implement reviewer identity, envelope reference, raw artifact reference, polarity, limitations and source grounding.

### RG-105 [P0] Evidence and Verification records

Implement evidence kind, bindings, validity contexts, verifier identity and outcome.

### RG-106 [P0] ReviewDecision and Finding projection

Require authority and rationale for acceptance/rejection. Finding must trace to claim.

### RG-107 [P1] Obstruction and CompletionCandidate

Represent process blockers, missing evidence, gluing conflict and repair proposals separately from findings.

### RG-108 [P0] Aggregate validation

**Acceptance criteria for RG-101–108**

- Deserialization uses the same invariants as constructors.
- AI-generated records default to `unreviewed`/`proposed`.
- Verification cannot exist without verifier and evidence/declared no-evidence proof mode.
- Accepted finding cannot exist without decision.
- All public records preserve provenance.

## 5. RG-200 — Local-first store

### RG-201 [P0] Append-only event envelope

Event ID, sequence, run, actor, timestamp, schema, payload hash.

### RG-202 [P0] Content-addressed artifact store

Atomic write, hash verification, media type, sensitivity classification.

### RG-203 [P0] Derived SQLite index

Index objects, references, lifecycle, coverage inputs and source dependencies.

### RG-204 [P0] Replay and rebuild

Delete index and reconstruct it from events/artifacts.

### RG-205 [P1] Transaction and locking

Stable commit order for parallel executions; no interleaved partial aggregate.

### RG-206 [P1] Export/import bundle

Manifest, schema, checksums, redaction and projection loss.

**Acceptance criteria**

- Event replay produces equivalent query state.
- Artifact corruption is detected.
- A failed execution leaves a valid incomplete run.
- SQLite is never the sole durable record.

## 6. RG-300 — ProgramSpace ingestion

### RG-301 [P0] Manual JSON adapter

Lift the checked-in reference ProgramSpace input without parsing source code.

### RG-302 [P0] Git snapshot adapter

Record repository, base/head, tree hashes, changed files, dirty state and exclusions.

### RG-303 [P1] Rust AST adapter

Use `syn` for files, modules, functions, types and source locations.

### RG-304 [P1] Cargo metadata adapter

Packages, targets, dependencies, tests and feature configuration.

### RG-305 [P1] Direct call extractor

Resolve statically evident calls; mark unresolved dynamic/macro cases.

### RG-306 [P1] Test mapping adapter

Map test names/files and explicit target references where available.

### RG-307 [P0] Extraction completeness

Report parse coverage, relation resolution, unsupported constructs and excluded regions.

### RG-308 [P1] ProgramSpace consistency validator

Reject dangling accepted relations and duplicate source identities.

**Acceptance criteria**

- Unsupported syntax becomes an obstruction/limitation, not an empty graph.
- Accepted facts are deterministic for fixed tool versions.
- Extractor provenance includes adapter version and configuration.

## 7. RG-400 — Obligation synthesis

### RG-401 [P0] Rule pack loader

Load normalized, versioned rules from profile files.

### RG-402 [P0] Applicability matcher

Match target kinds, changes, contexts and capability requirements.

### RG-403 [P0] Deterministic obligation ID generation

Use semantic target key, property, profile/rule and snapshot scope.

### RG-404 [P0] Node obligation rules

Changed public symbol and local correctness baseline.

### RG-405 [P0] Relation obligation rules

Call contract, async boundary, ownership and test relation.

### RG-406 [P1] Path obligation rules

Side-effect path and duplicate event path over bounded graph traversal.

### RG-407 [P1] Invariant obligation rules

At-most-once payment and projection loss declaration.

### RG-408 [P0] Universe descriptor

Record denominator, exclusions, rule/extractor hashes and capability limitations.

### RG-409 [P1] Supplemental AI obligation candidates

Keep semantic proposals outside accepted universe until reviewed or policy-admitted.

**Acceptance criteria**

- Same universe inputs produce same obligation set and order.
- Rule applicability failure is explainable.
- Missing extractor capability cannot silently suppress a required obligation.
- AI supplemental candidates do not inflate accepted generation coverage.

## 8. RG-500 — Planning and context projection

### RG-501 [P0] Risk descriptor

Impact, exposure, structural reach, uncertainty and policy priority as separate factors.

### RG-502 [P0] Baseline scheduler

Deterministic priority queue with dependency ordering and budget.

### RG-503 [P1] Weighted coverage scheduler

Select obligations under token/cost/execution budget.

### RG-504 [P0] ReviewContextEnvelope builder

Target source, direct neighborhood, paths, invariants, tests, evidence, unknowns.

### RG-505 [P0] Information loss declaration

Included/excluded source IDs, reason, count and recoverability.

### RG-506 [P1] Context size estimator

Estimate tokens/bytes before provider invocation.

### RG-507 [P1] Envelope materiality comparison

Detect whether a changed projection requires reviewer rerun.

**Acceptance criteria**

- Full repository is never the implicit default Envelope.
- Every Envelope traces to an obligation and snapshot.
- Omitted source regions are visible in audit view.
- Scheduler score can be decomposed and inspected.

## 9. RG-600 — Reviewer execution

### RG-601 [P0] ReviewerAdapter trait

Input Envelope; output structured claims, abstention, usage and raw artifact reference.

### RG-602 [P0] Deterministic fake reviewer

Fixture-driven outcomes for integration tests.

### RG-603 [P1] Command/process reviewer adapter

Provider-neutral subprocess contract without arbitrary shell interpolation.

### RG-604 [P1] One LLM provider adapter

Explicit model/config, source upload policy and structured output.

### RG-605 [P0] Claim parser and validator

Reject missing source grounding, unknown obligation IDs and invalid polarity.

### RG-606 [P0] Abstention taxonomy

Insufficient context, unresolved symbol, unsupported property, policy blocked, tool failed.

### RG-607 [P1] Parallel execution coordinator

Stable result commit order and bounded concurrency.

### RG-608 [P1] Prompt-injection boundary tests

Repository text cannot alter system protocol or allowed tools.

**Acceptance criteria**

- Parse failure does not fabricate an empty `issue_absent` claim.
- Provider failure leaves obligation incomplete/retryable.
- Usage and raw response hashes are auditable.
- Reviewer never writes to source repository.

## 10. RG-700 — Evidence and verification

### RG-701 [P0] Evidence ingestion

Source fact, test result, static result, runtime trace, human observation.

### RG-702 [P0] Evidence binding

Supports, refutes, qualifies, reproduces, contradicts, supersedes.

### RG-703 [P0] VerifierAdapter trait

Claim/property scoped input, typed outcome, evidence outputs and limitations.

### RG-704 [P1] Allow-list test verifier

No arbitrary command; fixed executable/config profile.

### RG-705 [P1] Static path/fact verifier

Check path existence, relation presence and source grounding.

### RG-706 [P0] Admission policy

Map verification and decision states to reportable finding status.

### RG-707 [P1] Human decision import

Accept/reject/exception with authority, scope and expiry.

**Acceptance criteria**

- Evidence validity is context/snapshot scoped.
- Unsupported verifier returns domain result, not false pass.
- Human exception cannot erase the underlying claim or obstruction.
- Accepted finding has complete trace chain.

## 11. RG-800 — Coverage, report and gate

### RG-801 [P0] Multi-dimensional coverage

Extraction, generation, visited, completed, evidence-supported, verified, fresh verified.

### RG-802 [P0] Raw and risk-weighted counts

Preserve numerator, denominator and excluded weight.

### RG-803 [P0] Report envelope

Scenario, result, coverage and ProjectionViewSet.

### RG-804 [P0] Human review projection

Findings, unknowns, blockers, decisions and limitations.

### RG-805 [P0] AI/audit projection

Stable IDs, references, versions, event range and information loss.

### RG-806 [P0] Policy gate

`pass / blocked / incomplete`; only gate maps domain state to CI exit code.

### RG-807 [P1] Markdown renderer

Renderer over report, never an independent source of truth.

**Acceptance criteria**

- Percentage always has denominator.
- Stale records are excluded from fresh coverage.
- Incomplete extraction cannot produce unconditional pass.
- Projection source IDs have no dangling references.

## 12. RG-900 — Context cover and gluing

### RG-901 [P1] Review context interpretation

Map Context, Cover, Section, Restriction and overlap to HigherGraphen structures.

### RG-902 [P1] Local section builder

Represent local contract, assumptions, claim and evidence environment.

### RG-903 [P1] Restriction compatibility

Compare values/claims on overlap.

### RG-904 [P1] Gluing result and obstruction

Success, candidate, failure, conflicting witnesses and required resolution.

### RG-905 [P1] Global claim admission

Require declared cover and compatible local sections.

### RG-906 [P1] Double-submit fixture

UI guard assumption vs payment idempotency assumption conflict.

**Acceptance criteria**

- Missing overlap evidence yields unknown, not success.
- Conflict retains both local source traces.
- Gluing obstruction can block only scoped global claims/gates.

## 13. RG-1000 — Incremental review

### RG-1001 [P1] Change morphism builder

Map preserved, modified, added, removed and unresolved facts.

### RG-1002 [P1] Obligation correspondence

Semantic key mapping including split/merge/supersede.

### RG-1003 [P1] Source dependency graph

Claim/evidence/verification/coverage dependencies.

### RG-1004 [P1] Direct staleness

Hash/version/source change.

### RG-1005 [P1] Property-sensitive indirect staleness

Dependency cone and property impact rules.

### RG-1006 [P1] Preservation verifier

Reuse accepted only with explicit preservation evidence.

### RG-1007 [P1] Partial rerun planner

Reviewer, verifier and gluing recomputation sets.

### RG-1008 [P1] Freshness report

Reason distribution and reusable verified weight.

**Acceptance criteria**

- Unknown mapping never becomes preserved by default.
- Rule/profile/policy changes invalidate affected records.
- Fresh gate uses current universe and current evidence.

## 14. RG-1100 — Evaluation

### RG-1101 [P2] Baseline runner

Diff-only, free-form, AST traversal and graph retrieval conditions.

### RG-1102 [P2] ReviewGraphen condition runner

G1–G6 feature flags with same model/budget.

### RG-1103 [P2] Benchmark adapters

Code Review Bench and available SWE-PRBench artifacts subject to license/availability validation.

### RG-1104 [P2] Mutation corpus

Layer-tagged injected faults and expected obligation class.

### RG-1105 [P2] Finding matcher

Exact, semantic candidate and expert adjudication queue.

### RG-1106 [P2] Metrics and statistics

Paired metrics, bootstrap intervals, variance and efficiency.

### RG-1107 [P2] Reproducibility export

Profiles, hashes, manifests, outputs, cost and limitations.

### RG-1108 [P2] Negative-result ledger

Projection loss, obligation miss, verifier rejection, stale miss.

## 15. RG-1200 — Security, quality and release

### RG-1201 [P0] Workspace path confinement

Reject symlink escape and out-of-root access.

### RG-1202 [P0] Secret detection/redaction boundary

Prevent accidental provider upload and report exposure.

### RG-1203 [P0] No arbitrary shell policy

Typed tool invocation and allow-list.

### RG-1204 [P1] Resource limits

Source bytes, context bytes, process CPU/memory/time and concurrency.

### RG-1205 [P0] Determinism tests

IDs, synthesis, projection source selection, coverage and reports.

### RG-1206 [P0] Property-based invariant tests

State transitions, numerator bounds, no dangling refs, no stale-fresh overlap.

### RG-1207 [P1] Fuzz schema/deserialization

Untrusted report/input boundary.

### RG-1208 [P1] Supply-chain and dependency policy

Pinned lockfile, advisory scanning and release provenance.

### RG-1209 [P1] Release contract checker

Schema fixtures, docs links, CLI help and migration matrix.

## 16. First vertical slice

最初に実装する順序は次です。

1. RG-001–004。
2. RG-101–108。
3. RG-201–204。
4. RG-301。
5. RG-401–408のうちreference rules。
6. RG-501–505。
7. RG-602、605、606。
8. RG-701–706のfixture verifier。
9. RG-801–806。
10. RG-906。

この時点では実ソース解析も実LLMも不要です。まず、checked-in ProgramSpaceからReview Graph全体が成立することを確認します。その後にRust/Git/LLMを接続します。

## 17. Backlogから除外するもの

次はMVP backlogへ入れません。

- visual graph explorer。
- IDE inline annotations。
- hosted billing。
- organization dashboard。
- autonomous patch generation。
- reviewer persona marketplace。
- general-purpose graph database migration。
- every-language CPG implementation。

必要性が検証された時点で、別のproduct/ADRとして追加します。
