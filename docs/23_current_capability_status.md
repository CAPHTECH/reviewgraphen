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
