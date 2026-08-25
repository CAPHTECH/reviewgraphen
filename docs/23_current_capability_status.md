# 23. Current Capability Status

> Status: canonical, dated 2026-08-18. This is the single document a
> reader should check for "what can ReviewGraphen do right now" — every
> other document that discusses the obligation-synthesis capability gap
> predates the fix described here and is being pointed at this document
> via a dated addendum, not rewritten (see §5).
>
> Verification note: every claim below was independently checked against
> the actual commits, code, and test suite in this repository as of
> commit `7f8716d` — not copied from any commit message or from the
> operator's own description of the work. Where a claim could not be
> independently reproduced, that is stated explicitly in §4 rather than
> presented as verified. `crates/` was read but not modified while
> producing this document.

## 1. The one-paragraph answer

As of today, `MvpRulePack::synthesize` produces a genuine, non-placeholder
obligation on real, unmodified Rust source for the first time since this
defect was diagnosed (`docs/measurement-validity-obligation-synthesis-capability-gap.md`,
2026-08-17): a changed public function or method with syntactically
established concurrency evidence (declared `async`, an `.await`, a spawn
call, a channel construction, or a named concurrency primitive) now
synthesizes an `async.concurrent_reentry` obligation at `applicable`,
verified by this repository's own passing test suite. This is one rule
of five, newly reachable through two independent, deliberately narrow
fixes on the ingest side — nothing in the synthesis layer or in any
rule's own logic was changed to make this happen. The other four rules
remain scoped to the hand-authored double-submit payment fixture and do
not fire on arbitrary real code (§3). A separate, unrelated durability
defect discovered while making this change is now understood and partly
fixed, with one piece explicitly deferred (§4.3).

## 2. What changed, and why it works — verified independently

### 2.1 The two root causes, and how each was closed

`docs/measurement-validity-obligation-synthesis-capability-gap.md`
identified two independent reasons every real-repository run produced
only `reviewgraphen.capability_gap` obligations: `is_changed_public_symbol`
required a `changed` attribute no ingest adapter ever set, and every
rule required a `complete` capability (`concurrency_model` for rule 1)
that `rust.rs` never declared.

**`changed` now reaches synthesis** (commit `7b40d48`). The change-family
artifact (the one record whose identity is already base-relative — it
exists only because a diff reported the path changed) now carries
`changed: true` and a `contains` edge to every accepted record whose
source region the change touched. Obligation synthesis reaches `changed`
by walking containment down from the change, never from a symbol's own
attributes — read directly in `crates/reviewgraphen-ingest/src/lib.rs`.

**`concurrency_model` now reaches `Complete`** (same commit). A new
`ConcurrencyScan` in `crates/reviewgraphen-ingest/src/rust.rs` reads,
off the same unexpanded `syn` tree the `ast` capability already covers:
whether a function/method is declared `async`, which lines it
syntactically `.await`s, which call paths are spawn- or
channel-constructing, and which concurrency-primitive type names occur
in its signature or body. **Verified by reading the code directly**:
`concurrency_model` is declared complete under the exact same loop
condition as `ast` and `containment` (`crates/reviewgraphen-ingest/src/rust.rs`,
the capability-declaration loop over `["ast", "concurrency_model", "containment"]`)
— i.e. whenever a file parses. It is bounded to syntax alone: it does
not resolve types, traits, or cross-crate references, and the file's
own doc comment on `ConcurrencyMarkers` states this explicitly. The five
permanently-`partial` resolution-bounded capabilities (`direct_calls`,
`imports`, `module_dependencies`, `test_mapping`, `state_writes`) are
untouched — this fix added a new, genuinely syntax-complete capability;
it did not loosen what the already-partial ones claim.

### 2.2 The rule itself was miscalibrated, then corrected, then versioned

Closing the two root causes made `node.changed_public_symbol@1` fire for
the first time on real input — and exposed that its trigger
(`kind == function && changed && public`) asserted a concurrency
property with no concurrency evidence at all. The commit that closed the
root causes reports (self-measured by the other agent, not independently
reproduced by this document — see §4.1) that on a real `axum` commit
range this produced 7 obligations, 5 over plainly synchronous functions.

Commit `c642641` narrowed the trigger to require the same
extractor-declared concurrency evidence `concurrency_model` now
produces — **verified by reading `crates/reviewgraphen-core/src/synthesize.rs`
directly**: the rule's condition now includes
`has_local_concurrency_evidence(&artifact.attributes)` alongside the
original `changed`/`public` checks. This commit deliberately kept the
rule at `@1`, reasoning that no stored obligation existed under the old
trigger for the version-bump protection to matter. Commit `d122046`
reversed that call once a separate durability blocker was fixed
(§4.3.1) and minted `@2` — **verified**: `synthesize.rs` names the rule
`"node.changed_public_symbol@2"` at the call site building this
obligation, and `crates/reviewgraphen-core/src/m6.rs` retains `@1`
resolution alongside `@2` for reading pre-existing stored history.

Commit `7f8716d` gave each of the five rules its own rationale string,
replacing one payment-scenario sentence all five previously shared.
`node.changed_public_symbol@2`'s is now: *"a changed public symbol with
extractor-established concurrency may be re-entered while its own
earlier invocation is still in flight."*

### 2.3 Independent verification performed for this document

- `mise run test-ingest`: **40/40 pass**, including
  `a_changed_public_async_function_synthesizes_a_substantive_obligation`
  — read directly: this test runs the real `ingest` → `MvpRulePack::synthesize`
  pipeline against a fixture repository and asserts (a) `changed` reaches
  the target function through containment, (b) synthesis produces at
  least one obligation whose `property_id` is not `reviewgraphen.capability_gap`,
  and (c) that obligation is `async.concurrent_reentry`, targeting the
  function, at `applicability_status: "applicable"` — not `unknown`.
- `cargo test -p reviewgraphen-core`: **422/422 unit tests, 65/65 doctests
  pass.**
- `cargo test -p reviewgraphen-store`: 202/202 pass single-threaded
  (210.95s); under the default parallel run, 2 of 202 fail
  (`validated_v4_handle_reuses_one_snapshot_and_refuses_drift_tamper_and_wrong_journal`,
  `simulated_crash_after_temp_sync_never_publishes_and_is_gc_recoverable`)
  and both pass individually in isolation — consistent with
  test-parallelism flakiness in fault-injection tests unrelated to the
  files this fix arc touched, not a regression. Not root-caused further;
  out of this document's scope.
- `cargo test -p reviewgraphen-reviewer/-runtime/-report/-verifier`: all
  pass (31, 32, 38, 4 respectively, plus doctests).
- `cargo clippy --workspace --all-targets`: clean, no warnings.

This independently confirms the pipeline behavior the fix commits claim,
using this repository's own test suite rather than trusting any commit
message's prose.

## 3. What is still unreachable — verified by reading, not assumed unchanged

**Rules 2 through 5 remain scoped to the double-submit payment fixture
and do not fire on arbitrary real Rust code.** Verified by reading each
rule's trigger condition in `crates/reviewgraphen-core/src/synthesize.rs`
directly (not by re-reading the original diagnosis and assuming it still
holds):

| Rule | property_id | Trigger condition | Reachable from real Rust ingest? |
| --- | --- | --- | --- |
| `node.changed_public_symbol@2` | `async.concurrent_reentry` | changed, public function with syntactic concurrency evidence | **Yes** — the fix in §2 |
| `relation.concurrent_reentry@1` | `async.concurrent_reentry` | a `handled_by` relation with `attributes["concurrency"] == "unbounded_reentry"` | No — this attribute is not produced by `rust.rs`'s `ConcurrencyScan` or anywhere else in the ingest adapter; only the hand-authored double-submit fixture JSON sets it |
| `relation.changed_call_contract@1` | `payment.idempotency_contract` | a `calls` relation with `idempotency_key_forwarded == false` reaching a target with `external_side_effect == true` | No — neither attribute is produced by real ingest; both are payment-domain-specific semantic facts only the fixture declares |
| `path.external_side_effect@1` | `payment.at_most_once` | traces `calls` relations from an artifact bound to the variable `submit` | No — keyed to the double-submit fixture's own naming, not a general pattern |
| `invariant.payment_at_most_once@1` | `payment.at_most_once` | payment-scenario invariant matching, same trace | No — same as above |

This matches the original diagnosis's statement that "rules 2 through 5
are payment/double-submit-scenario-specific" — **still true today**,
confirmed by reading the current code, not inferred from the fact that
it was true before.

## 4. What remains incomplete, deferred, or unverified by this document

### 4.1 What this document did not independently reproduce

The fix commits report measuring effect on a real `axum` commit range
(a throwaway clone, nothing retained in this repository): before the
fix, 5 `capability_gap` obligations and nothing else; after wiring
`changed`/`concurrency_model` but before narrowing the trigger, 7
`async.concurrent_reentry` obligations (5 over synchronous functions);
after narrowing, 1 `async.concurrent_reentry` obligation (a genuinely
concurrent handler) alongside 4 `capability_gap`. **This document did
not reproduce the `axum` run itself** — verification here relied on this
repository's own fixture-based test (§2.3), which exercises the
identical code path with the identical assertions, rather than an
external clone. The `axum`-scale numbers are reported by the commits,
not independently confirmed by this document.

### 4.2 The generic rule pack

Rules 2 through 5 (§3) are not a general capability, and closing them
is not part of what shipped today — this fix arc closed the two root
causes for rule 1 specifically and left the other four exactly where
they were. No work observed in this repository today generalizes rules
2 through 5 beyond the double-submit scenario.

### 4.3 Durability findings surfaced while making this change

Fixing rule 1's reachability exposed a separate, unrelated defect,
documented in full in
`docs/durability-finding-run-genesis-resynthesis-couples-stored-history-to-current-rule-pack.md`:
every decode of a persisted run genesis re-ran synthesis against its own
`ProgramSpace` and required byte equality with the stored result, so a
genesis written by any other rule pack version failed to load at all —
inside `JournalIdentity::new`, before any caller could inspect it. Four
instances of "who wrote this record" checks embedded inside structural
validation were found and closed (commits `ae805f4`, `8906e65`,
`0cb0568`, plus the read/write boundary split in `be8c24f`) — **verified
by reading `docs/durability-finding-...md`'s own instance table and
cross-checking each cited commit's diff directly.**

**4.3.1 What's fixed:** authorship is now established in exactly one
place (`RunGenesisSnapshot::reproduction`), returning one of three typed
verdicts — `Reproduced`, `NotReproducible { recorded_pack, running_pack }`,
or `NotSynthesizable { profile, reason }` — instead of a hard decode
error. Read-only callers can open a run the current rule pack doesn't
reproduce; anything that extends a run (appends events, mints new
obligations) still refuses unless `Reproduced`, held by construction
(the sole `JournalIdentity::new` construction site) rather than by
enumerating extension points.

**4.3.2 What's explicitly not fixed, recorded as such, verified still
absent:** `decode_index_v5_genesis` (`crates/reviewgraphen-store/src/journal.rs`)
is the one genesis-decode path that does not go through core's
`from_canonical_bytes*` seams, so it never checks canonical-byte
equality — forged obligation bodies are still refused there, but
noncanonical encodings are not. Recorded in `docs/durability-finding-...md`
(commit `31078be`) as a known, separate, unfixed defect, not silently
left out.

**4.3.3 The verdict is not yet surfaced anywhere a human would see it.**
Grepped `crates/reviewgraphen-report/` for `NotReproducible` and
`NotSynthesizable`: **zero occurrences.** The typed verdicts exist at
the core/store layer only; nothing in the report or CLI surfaces
currently reads or displays them. `docs/durability-finding-...md` §
"How the write boundary is actually held (option C, step 2)" and its
sibling commits describe this as staged work ("step 1", "step 2" of a
three-part scope); no commit or document found states step 3 (verdict
propagation into a human-visible surface) as done, and the code confirms
it is not.

### 4.4 Not evaluated by this document

The hostile-corpus fixture (`terminal-v5-gluing-genesis.json` and its
chained artifacts) was deliberately not regenerated (recorded in
`docs/durability-finding-...md` § "Not done, deliberately") — this
document did not re-evaluate that decision. Whether any m7-* benchmark
experiment should be re-run under the fixed synthesis path is not
addressed here; those experiments' own records stand as historical
measurements of the pre-fix system (§5) and re-running them, if desired,
is a separate decision.

## 5. Relationship to prior documents — append-only, framing only

This document **does not rewrite, correct, or invalidate** any prior
diagnosis. Per this repository's own discipline, nothing already written
is edited for content. What changes is framing: prior documents describe
a defect that was true when written and is no longer true today for the
one rule this fix addressed. Each of the following now carries a dated
addendum pointing here, added after this document, not instead of
anything in them:

- `docs/measurement-validity-obligation-synthesis-capability-gap.md`
- `benchmarks/m7-head-local-v1/CAPABILITY_GAP_DIAGNOSIS.md`
- `benchmarks/m7-real-v1/results/full-reviewgraphen-replicate-2/REPORT.md`
- `benchmarks/m7-pilot-v2/README.md`
- `benchmarks/m7-local-factorial-v2/README.md`

Every number and disposition in those documents (capability_gap-only
output, the specific experiments it invalidated, the root-cause
attribution) stands as an accurate historical record of the system as it
existed when measured. None of the m7-* benchmark results were
re-computed, and none of this document's claims are retroactively
applied to them.

## 6. 2026-08-25 addendum — ADR 0038 changed-public-callee slice

This dated addendum is append-only. It does not revise the 2026-08-18
diagnosis above. The claims in this section were checked against the working
tree's code and product-binary runs recorded through 2026-08-25; no commit
message or another operator's report was used as evidence. A command that did
not reproduce successfully is recorded as such, and the associated capability
is not called implemented here.

### 6.1 Narrow, independently reproduced capabilities

The current ADR 0038 path is restricted to `rust.production.v1` and to an
accepted `calls` relation whose resolution is `syntactic_unique`, with an
accepted caller, an exact accepted public function callee, and changed
containment for that callee. It synthesizes the review question
`relation.changed_public_callee@1` / `rust.callee_contract_review@1`; this is
not a statement that a callee contract changed or that a bug exists. The
trigger and property are visible in
`crates/reviewgraphen-core/src/synthesize.rs:1128-1238`. Calls outside that
accepted set, including methods, imports, macros, dynamic dispatch, ambiguous
targets, and other unresolved forms, are not silently counted as complete.
`direct_calls` remains partial, with explicit occurrence obstructions and a
global limitation.

`context.subject_windows@2` is a bounded projection policy with a fixed
policy hash (`crates/reviewgraphen-core/src/context.rs:247-262`). It carries
source/window IDs, a projection hash, and typed subject loss rather than
pretending that a whole source file was supplied. The independent tests below
covered its golden policy bytes, deterministic windows, and inclusive/+1 line,
byte, and total-byte limits.

The generic v2 audit has a fixed non-authority ceiling:
`classification = non_authority`, `trusted_pass = false`, and
`result_status = incomplete`
(`crates/reviewgraphen-runtime/src/generic.rs:2731-2765`). Its deterministic
observer is `deterministic.abstain@1`; it produces a structured abstention
rather than treating prose as canonical state. Provider-backed observer kinds
are present in the request schema but the current v2 executor rejects them as
unsupported (`generic.rs:2054-2062`). Therefore no real model-adapter run has
been reproduced.

### 6.2 Practical-usefulness gates — current evidence

| Gate | Status | Evidence checked through 2026-08-25 |
| --- | --- | --- |
| 1. Local repo + base/head → deterministic ingest → versioned universe | **Partial** | The product-binary v3 run completed on the positive D pair `a8b6b24d…` → `8569a226…` with exit 0 in 212s. Its accepted-file denominator was 4,816, its reached-file denominator was 1, and it carried 2 D obligations. Two executions produced byte-identical audit and manifest files; enabling `--diagnostics` did not change those canonical bytes. The path remains limited to the Rust production profile and D rule, not general-language ingest. |
| 2. Non-fixture Relation obligation | **Achieved, narrow** | `cargo test -p reviewgraphen-core --test changed_public_callee_rule` passed 24/24; `cargo test -p reviewgraphen-ingest --test changed_public_callee_facts` passed 19/19. These include trigger negatives, endpoint rejection, split capability traces, and deterministic canonical contracts. |
| 3. Bounded projection with source IDs/loss/hash | **Achieved, narrow** | `context.subject_windows@3` completed the positive D pair with the 4,816 accepted-file / 1 reached-file commitments above. Canonical bytes contain counts and digests, not the accepted/reached file-ID sets or per-reason lost-anchor ID sets: bytes-only validation establishes structural and wire-visible closure, not denominator correctness. Before sealing, the Runtime path uses trusted `ContextValidationBasisV3`, bound to the immutable snapshot, to rebuild and validate the denominator sets, counts, digests, and partitions. The five premise-locking test families passed across Core, Runtime, and CLI. |
| 4. Structured claim or abstention; prose not canonical | **Partial** | The deterministic structured abstention and non-authority audit are implemented for request/run v3. Actual model output has never been executed; the structured-claim path therefore remains unconfirmed. |
| 5. Claim / evidence / verification / human decision are separate | **Partial** | v2 is explicitly prevented from minting Evidence, Verification, Decision, Finding, accepted claim, or Store admission (`generic.rs:1349-1352`). This confirms its non-promotion boundary, not an end-to-end human-decision workflow for the D slice. |
| 6. Allow-listed, workspace-scoped verification | **Unmet (deferred)** | `workspace.cargo_test@1` returns only typed `unsupported`; it starts no process and resolves no executable. `cargo test -p reviewgraphen-verifier --test deferred_workspace_seam` passed 35/35. This is a security deferral, not an implemented verifier. |
| 7. Short report + audit JSON in one CLI workflow | **Achieved, narrow** | At commit `f68e764`, two independent `git clone --no-local` clones followed the README: locked build succeeded, the CLI emitted the human report and audit with exit 0 in 238s and 235s, `verify.py` exited 0, and each second invocation exited 20. This is limited to the Rust production profile, D rule, and this quickstart. |
| 8. Provider-free deterministic quickstart + real model-adapter route | **Partial** | Provider-free v3 construction completed deterministically as described above. Real Codex/Claude/App Server observer execution is still rejected as unsupported; model evaluation count is 0. |
| 9. Reproducible from clone following documentation | **Achieved, narrow** | The two independent clean clones produced byte-identical audits, also identical to the working-tree audit, while following the README and pinned expected hashes. This establishes reproducibility only for the Rust production profile, D rule, and this quickstart. |

### 6.3 Limits and evaluation status

- Mutation sampling exercises representative seams only. It is not an
  exhaustive score over every branch or every checked call site.
- Carrier-size limits are regression checks against carrier-layout growth.
  They are not a formal maximum-stack proof for every toolchain or platform.
- The legacy byte oracle fixes checked-in fixtures and a fixed live-ingest
  sample. It is not a universal observational-equivalence proof for arbitrary
  repository inputs.

No model-based evaluation has ever been run: the model evaluation count is 0.
Stage 0 (the model-free measurement) has started through the accepted driver,
but no Stage 0 result has yet been observed. No comparison with a free-form
baseline exists. This repository therefore makes no claim that ReviewGraphen
is better than free-form review. M20 has completed its fourth atomic re-freeze at SHA-256
`6b71b4b63f550bd4896d13658f4ce295b8113e68eec3ce788414ed2fca99bbd7`.
The `95b75a62…fa73`, `eb207a22…e1a7e`, `19014d24…c1e6`, and
`f3af4c7b…adcb8` freezes remain as superseded history. The manifest binds each
of four generated artifacts by its individual SHA-256.
Specification and implementation now agree on the 3-input execution hash and
15-key manifest; executable-drift checks SC01/SC02 were added. Vectors passed
74/74 and the mutation sweep passed 3,990/3,990 with `SCORE_AFFECTING=0`.
Stage 0 results, model results, and corpus outcomes remain unobserved, and the
model evaluation count is 0. A freeze, model pin, product-binary completion,
or the source-level tests in §6.2 is not evidence of utility. The
practical-usefulness gate therefore remains **fail**: gate #6 is intentionally
deferred and there are zero model evaluations.

Wave 18 independent review concluded **BLOCKING 0 — accepted** for the Stage 0
driver, including its parallel-execution contract and pre-corpus freeze gate.

The v2 policy remains a supported, typed failure boundary for this pair: the
product binary exited 20 after 204s at the 4,097th candidate, reporting the
4,096 limit. The former six-hour nontermination is therefore replaced by a
typed failure. The product has no deadline, assigns no exit 21, and the
withdrawn 10-second watchdog is only a test-harness condition.

### 6.4 Latest observed verification

The 2026-08-25 quiescent-state verification recorded Core 600, Runtime 50,
Report 87, and CLI 7, all with exit 0; workspace clippy and formatting also
exited 0. The product-binary v3 run exited 0 in 209s, `verify.py` exited 0, and
the audit was byte-identical to the first run; a second invocation exited 20.
The two-clone quickstart check remains pending a commit.
