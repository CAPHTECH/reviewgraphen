# Bottleneck analysis: from generic proposal pipeline to useful Rust review

## 1. Causal hierarchy

The shortest diagnosis is: **the framework/control plane is substantially ahead
of the semantic product slice**. The generic pipeline can faithfully execute
what its universe asks, but its built-in universe rarely asks a third party a
general, relation-level Rust correctness question, and its result cannot yet
cross a safe verification/report surface.

### Root A — the built-in semantic denominator is not yet a general Rust rule pack

**Observed symptom:** ordinary Rust can produce one
`async.concurrent_reentry` Node obligation, while Relation/Path/Invariant rules
remain absent or capability gaps.

**Immediate cause:** `MvpRulePack` declares five substantive rules, but four
triggers consume fixture-only vocabulary. `relation.concurrent_reentry@1`
requires `handled_by` plus `concurrency=unbounded_reentry`; the next relation
requires payment/idempotency/external-effect attributes
(`crates/reviewgraphen-core/src/synthesize.rs:617-688`). Path and invariant
materialization are reachable only after those relations
(`crates/reviewgraphen-core/src/synthesize.rs:690-825`). The generic runtime
always selects this pack (`crates/reviewgraphen-runtime/src/generic.rs:599-603`).

**Deeper cause:** the current general Rust fact vocabulary is deliberately
syntax-first. It can prove containment, declaration syntax, changed overlap,
and a conservative subset of local direct calls. It cannot pretend that method
names resolve to types, that a lock name is a particular lock, or that a trait
path denotes a known contract. Direct calls/imports/test mapping/state writes
therefore remain partial (`crates/reviewgraphen-ingest/src/rust.rs:159-193`),
and method dispatch remains explicitly unresolved
(`crates/reviewgraphen-ingest/src/rust.rs:1941-1951`). The error is not that
these limitations are recorded; the missing product decision is a useful rule
whose applicability is exactly supported by facts the extractor can honestly
produce.

**Practical gates unlocked if solved:** versioned non-fixture Relation or
Invariant denominator; measurable utility on real changes; fewer
capability-gap-only runs; a defensible basis for property-specific projection
and verification.

### Root B — projection provenance is real, but subject comprehension is not guaranteed

**Observed symptom:** an envelope can be schema-valid, source-bound, hash-bound,
and still omit the code the obligation is about.

**Immediate cause:** `excerpt` chooses one contiguous interval from the lowest
anchor to the maximum anchor and, when it exceeds the 400-line cap, truncates
forward from the lowest anchor (`crates/reviewgraphen-core/src/context.rs:3179-3252`).
The real probe documented first-400-line windows and even a perfect seed whose
callee anchor displaced the subject (`docs/24_context_projection_feasibility_for_implementation.md:61-98`).

**Deeper cause:** policy identity and loss accounting were designed before an
explicit “subject must survive selection” invariant and before multi-window
source sections. The only public construction entry point is an obligation ID
(`crates/reviewgraphen-core/src/context.rs:1852-1856`); discovery then expands
the obligation's target/context/source references. There is no independent
named-symbol query and no priority class distinguishing subject anchors from
discovered support anchors.

The old `state:*` crash is no longer a current blocker: unparented range-bearing
records now contribute no anchor rather than causing validation failure
(`crates/reviewgraphen-core/src/context.rs:1945-1988`; resolution record at
`docs/24:136-184`). Treating that historical defect as the current bottleneck
would be incorrect.

**Practical gates unlocked if solved:** useful bounded context; deterministic
target coverage; honest per-window loss; stable evidence source closure;
reproducible “unknown/unseen” display.

### Root C — generic observation stops before evidence and verification

**Observed symptom:** generic output always says `evidence_not_executed` and
`human_decision_not_recorded`.

**Immediate cause:** `run_with_driver` ends after model observation, strict
parse, coverage, and an authority ceiling (`crates/reviewgraphen-runtime/src/generic.rs:647-718,1243-1266`).
It constructs no Evidence, EvidenceBinding, Verification, Decision, or Finding.

**Deeper cause:** the implemented verifier was intentionally frozen to two
process-free M4 descriptors. It accepts neither executable nor argv nor cwd and
its only executable semantics is the compiled double-submit harness
(`crates/reviewgraphen-verifier/src/lib.rs:27-145,246-300`). ADR 0021 correctly
requires, but defers, the real boundary: code-owned descriptor, fixed argv,
read-only workspace, workspace-scoped cwd, no network, and hard resource/output
limits (`docs/adr/0021-m4-evidence-bound-verification.md:372-382`). The isolated
**reviewer** bwrap is not a verifier: its repository is intentionally absent and
its tool set empty (`crates/reviewgraphen-reviewer/src/process.rs:584-628,657-726`).

**Practical gates unlocked if solved:** typed `unsupported`/`inconclusive`/pass
outcomes on an ordinary repo; evidence/claim separation in a real workflow;
safe test evidence without reviewer shell authority; future human decision and
freshness checks.

### Root D — the product surface exposes audit JSON, not a review artifact

**Observed symptom:** a third party receives a very large canonical generic run
on stdout, but not a concise PR-facing report.

**Immediate cause:** CLI review serializes `GenericReviewRun` directly
(`crates/reviewgraphen-cli/src/lib.rs:77-89`). The generic run contains useful
audit fields but no report projection (`crates/reviewgraphen-runtime/src/generic.rs:126-139`).
The existing report crate starts from a Store journal, verified index, and CAS
authority roots (`crates/reviewgraphen-report/src/lib.rs:116-184`), not from a
generic non-authority proposal run.

**Deeper cause:** ADR 0030 correctly refused to forge V5 authority from model
output (`docs/adr/0030-generic-review-orchestration.md:107-126`), but no separate
non-authority human projection was added. “Do not launder proposals” became a
stop at JSON rather than a report whose headings explicitly say proposed,
unverified, abstained, excluded, and unknown.

**Practical gates unlocked if solved:** one-command PR consumption; short
proposed-finding list; visible unsupported/unknown/loss; audit JSON retained as
the source projection rather than replaced by prose.

### Root E — no provider-free fresh generic execution path

**Observed symptom:** after cloning, a user must configure a model credential or
already possess one replay record per scheduled obligation.

**Immediate cause:** the request schema has Codex, Claude, unsupported
app-server, and replay only
(`schemas/reviewgraphen.generic_review_request.v1.schema.json:60-66,72-121`).
The `FakeReviewer` is a fixture-key lookup, not a generic CLI mode
(`crates/reviewgraphen-reviewer/src/lib.rs:2645-2714`). The runtime's successful
ordinary-Git test injects a private `StructuredDriver`
(`crates/reviewgraphen-runtime/src/generic.rs:1445-1477`), which a user cannot
select.

**Deeper cause:** the removal of `review --fixture` was correct—the old command
could falsely demonstrate generality (`docs/adr/0029-generic-only-review-command.md:7-36`).
The missing replacement is a generic deterministic observer whose behavior is
repo-independent (for example, always abstain with a fixed typed reason), not a
new named scenario fixture.

**Practical gates unlocked if solved:** clone-and-run documentation; CI smoke
path; deterministic canonical/replay fixture generated from any ordinary repo;
provider adapter tested as a substitutable boundary rather than a prerequisite.

### Root F — efficacy evidence does not support a general product claim

**Observed symptom:** many mechanics are well tested, while there is no current
population estimate that the generic product reduces missed Rust defects.

**Immediate causes:**

- Historical M7 full/proxy arms used a pre-fix capability-gap-only denominator;
  current canonical docs explicitly say none were recomputed
  (`docs/23_current_capability_status.md:232-253`).
- M13 proved compact projection recognition in four one-replicate cells, not
  review utility (`benchmarks/m13-projection-recognition-factorial-v1/RESULT.md:3-30`).
- M14 showed that a complete 9.3 KB projection can cause objective/operation
  failure (`benchmarks/m14-projection-payload-boundary-v1/RESULT.md:17-30`).
- M15 did not complete a report in either condition
  (`benchmarks/m15-qwen-intelligent-reviewgraphen-v1/RESULT.md:1-41`).
- M16 completed one configuration and produced one issue-worthy judgment from
  three proposals, with one replicate (`benchmarks/m16-qwen-chained-reviewgraphen-v1/RESULT.md:7-37,52-69`).
- M17 made the previously failing 4-bit/low cell complete through an external
  checkpoint runtime, but the effect belongs to that runtime, not CaseGraphen
  scheduling, and still has one replicate
  (`benchmarks/m17-casegraphen-controlled-review-v1/RESULT.md:29-49,60-80`).

**Deeper cause:** mechanism validation, interface compliance, model finding
quality, and population defect recall have repeatedly been different estimands.
They must not be collapsed into “ReviewGraphen works.”

**Practical gates unlocked if solved:** an honest go/no-go criterion; evidence
that the semantic slice is worth maintaining; a bound on false-positive cost;
and a reproducible measure of what the system did not inspect.

## 2. Bottleneck order

| Priority | Bottleneck | Why it precedes the next one |
| ---: | --- | --- |
| 1 | General, deterministically grounded Relation/Invariant rule | Without a useful denominator, improving execution/reporting only presents the wrong questions more cleanly. |
| 2 | Subject-preserving multi-window projection | A good rule with missing target bytes cannot yield grounded claims; model tuning cannot repair absent input. |
| 3 | Provider-free generic observer + short non-authority report | This creates a reproducible user workflow and makes failures/unknowns visible before adding authority. |
| 4 | Closed workspace verifier | This is security- and semantics-sensitive; it should verify a frozen property/evidence contract, not precede it. |
| 5 | Durable human decision/promotion | Only after evidence/verification closure is demonstrated. It must remain a distinct Store operation, never a side effect of report rendering. |
| 6 | Preregistered efficacy trial | Freeze before implementation, enumerate/run only after the slice and its negative tests are fixed. |

## 3. What is not a root-cause fix

- Raising the 400-line limit does not provide subject priority or disjoint
  windows.
- Marking `direct_calls` complete because one repository happened to resolve
  locally would erase the declared denominator limitation.
- Asking the LLM to infer lock types, method targets, `Result` receivers, or
  trait identity would replace a resolver with prose and violate the project
  boundary.
- Reusing a benchmark JSON object extractor as the product parser would cross a
  consumer boundary; the product parser is already stricter and canonical.
- Adding more agents/personas would not create independent evidence, a missing
  obligation, or a verifier.
- Emitting a V5-looking report from generic proposals would launder authority;
  the needed first report is explicitly non-authority.
