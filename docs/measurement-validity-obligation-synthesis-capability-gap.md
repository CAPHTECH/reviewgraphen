# Measurement-validity finding: ReviewGraphen's obligation synthesis is capability-gated shut for all real ingestion

- Status: diagnosis, no fix implemented
- Date found: 2026-08-17
- Scope: this is a program-wide finding about ReviewGraphen the implementation,
  not specific to any one benchmark. It is recorded here, independent of any
  experiment directory, because it affects the construct validity of every
  `full_reviewgraphen`/`full_review_graphen`/`g3_proxy` arm this benchmark
  program has ever run through the code path described below.

## Finding

`MvpRulePack::synthesize` (`crates/reviewgraphen-core/src/synthesize.rs`) is
fully implemented and does produce substantive, non-`capability_gap`
obligations (`async.concurrent_reentry`, `payment.idempotency_contract`,
`payment.at_most_once`) — but only when given a `ProgramSpace` that (a) has
at least one artifact with an explicit `attributes.changed: true` fact, and
(b) declares the rule's `required_capabilities` at `CapabilityState::Complete`.

No real ingestion adapter in this repository (`crates/reviewgraphen-ingest/`)
ever produces either of those two things, for any input, in any language.
Every real `full_reviewgraphen`/`g3_proxy` packet this benchmark program has
built therefore receives exactly 5 `reviewgraphen.capability_gap` obligations
— one per `MvpRulePack` rule — and nothing else, regardless of the source
code reviewed. This was directly verified for `fsl` HEAD packets
(`benchmarks/m7-head-local-v1`), `m7-local-factorial-v2` (snapshots 06 and
40), `m7-pilot-v2`'s `g3_proxy` arm, and a byte-verified reconstruction of
`m7-real-v1`'s actual scored `full_review_graphen` snapshot-01 packet (see
`benchmarks/m7-head-local-v1/diagnostics/m7-real-v1-full-obligation-
regeneration/`).

## Root cause, precisely

1. **The "changed" gate.** `is_changed_public_symbol` (synthesize.rs) requires
   `artifact.attributes.changed == true` (directly, or on a containing
   artifact via a `contains` relation) before `node.changed_public_symbol@1`
   — the one `MvpRulePack` rule with no other structural precondition — will
   even attempt to materialize an obligation. Grepped across every file in
   `crates/reviewgraphen-ingest/` and `crates/reviewgraphen-core/`: the
   string `"changed"` as an attribute assignment appears exactly once outside
   test code, and that one occurrence (`crates/reviewgraphen-core/src/
   planning.rs:2007`) is itself inside a `#[test]` function. **No adapter
   computes or sets this attribute on any real artifact.** `crates/
   reviewgraphen-ingest/src/rust.rs::extract` takes a `_snapshot_id` it
   never uses and processes `snapshot.files` uniformly; it has no concept
   of "changed relative to what."
2. **The capability gate.** Independent of (1), every `MvpRulePack` rule
   requires at least one of `concurrency_model`, `direct_calls`, or
   `test_mapping` at `CapabilityState::Complete`
   (`capability_fully_available`, synthesize.rs, accepts only `Complete`).
   `crates/reviewgraphen-ingest/src/rust.rs` never declares
   `concurrency_model` at all (absent from its capability map for any
   input), and declares `direct_calls`/`test_mapping` (and `imports`/
   `module_dependencies`/`state_writes`) **unconditionally `Partial`, by
   explicit documented design** — the adapter's own comment states this
   reflects a genuine, permanent bound (local-syntactic-only resolution,
   no cross-crate or dynamic-dispatch resolution), not a bug to be
   silently outgrown.
3. **A related but distinct capability, `changed_structure`, does exist and
   can reach `Complete`** — `crates/reviewgraphen-ingest/src/git.rs`
   computes a real base→head file-level diff and declares this capability
   `Complete` when no excluded (symlink/submodule/unsupported) entries are
   part of that diff. This is a genuine, working piece of change detection.
   **It is never connected to artifact-level `attributes.changed`.** The
   gap between (1) and (3) is an integration gap specifically — the
   information rust.rs would need already exists at the git-ingest layer,
   but nothing wires it down to individual function/symbol artifacts.
4. **The rule pack itself is a single hand-built scenario, not a general
   Rust rule set.** All 5 `MvpRulePack` rule contracts share the identical
   literal `rationale` string ("External payment side effect is reachable
   from a repeatable UI event."). 4 of the 5 rules additionally require
   relation kinds/attributes (`handled_by`, `idempotency_key_forwarded`,
   `external_side_effect`, `covers`, `constrains` with specific attribute
   values) that appear nowhere in `crates/reviewgraphen-ingest/` — grepped
   and confirmed absent outside `#[cfg(test)]` code and
   `examples/double-submit-payment/program-space.json`, a fully
   hand-authored fixture (10 artifacts, capabilities manually set to
   `complete`/`partial` as needed, `changed: true` set by hand on one
   artifact) that exists specifically to exercise `MvpRulePack::synthesize`
   end to end in tests. This fixture is the *only* input in this
   repository, as of this finding, that reliably drives `MvpRulePack` past
   `capability_gap`.

**Confirmed: the obligation-synthesis code itself works correctly and is
tested** — extensively, via the fixture above, across
`reviewgraphen-core`, `reviewgraphen-runtime`, `reviewgraphen-reviewer`,
and other crates' test suites. The defect is entirely on the ingestion
side: no adapter has been built (or extended) to supply the facts
`MvpRulePack` needs, for any language, on any real repository. This is not
`fsl`-specific and not this benchmark's fault; `fsl` simply exposed a
pre-existing, universal gap the first time this benchmark program pointed
`full_reviewgraphen` at a real, previously-unexamined codebase and someone
(the operator) read the resulting `obligations.json` directly rather than
trusting the packaging around it.

## Design intent, per ADR 0011

`docs/adr/0011-program-space-v2-capability-trace.md` documents the
capability-gap mechanism's own design rationale directly: the point of
distinguishing `capability_undeclared` from `capability_partial`/
`capability_missing`/`capability_unknown` reasons is precisely so "a
reviewer reading the coverage denominator cannot tell 'the adapter tried
and failed' from 'no adapter ever declared this capability'" is no longer
true — i.e., the capability-gap reporting mechanism is intentional,
mature, documented design, working as intended. What ADR 0011 does not
address, and what this finding adds, is that *no adapter has ever been
extended to close any of these gaps for a real codebase* — the trace
mechanism correctly reports an absence that has simply never been filled.

## Answers to the operator's three questions

**Is obligation generation implemented as designed, or stuck at
capability_gap only?** Both, precisely: the synthesis/rule-evaluation
layer (`MvpRulePack::synthesize` and its capability-gap fallback) is
implemented as designed and is not the defect. Every real ingestion
adapter is, for the purposes of driving this rule pack, stuck at
capability_gap only — permanently, by construction, not by omission that
merely hasn't been reached yet.

**What would it take to get substantive obligations?** Two independent
pieces of ingest-side work, neither large in isolation:
- Wire `git.rs`'s already-`Complete`-capable `changed_structure` diff
  facts down into `attributes.changed` on the artifacts rust.rs (or any
  adapter) produces. This alone would let `node.changed_public_symbol@1`
  materialize obligations (still capability-gapped on `concurrency_model`
  and `ast`, but no longer silently absent at the "changed" gate).
- Implement and declare a `concurrency_model` capability in `rust.rs` at
  `Complete` for at least some real patterns (e.g. `async`/`await`,
  thread/lock use, actor/handler dispatch) — this and the `changed`-wiring
  above together would make rule 1 (`async.concurrent_reentry`) reachable
  on real code.
- Rules 2-5 remain unreachable regardless, because their relation-kind/
  attribute triggers are specific to the hand-built payment/double-submit
  scenario and are not implemented as general Rust facts anywhere. Making
  `MvpRulePack` (or a new rule pack) produce general-purpose review
  obligations for arbitrary Rust code is a substantially larger
  undertaking than the two items above — writing new rules against real,
  general program-space facts, not just unblocking existing ones.

**Effort estimate:** The `changed`-wiring integration and a first
`concurrency_model` declaration are each bounded, single-adapter changes
(read `git.rs`'s existing diff output, propagate a boolean; add one new
capability declaration and a real detection heuristic in `rust.rs`) —
plausibly small enough to be worth doing and re-measuring. Making
`full_reviewgraphen` produce *general* substantive review obligations for
arbitrary Rust code (rather than the one narrow, already-reachable
concurrent-reentry property) requires a new, non-fixture-specific rule
pack — a materially larger effort, outside the scope of this diagnosis to
size further. No fix is implemented here, per the operator's instruction;
this is a report for the operator's decision.

## Corrective addenda

Per the operator's instruction, the affected experiment records are not
rewritten. Each gets a short, dated addendum pointing here, added without
touching any existing content:

- `benchmarks/m7-pilot-v2/README.md` (or nearest record)
- `benchmarks/m7-real-v1/results/full-reviewgraphen-replicate-2/REPORT.md`
- `benchmarks/m7-local-factorial-v2/` (relevant record)

## Addendum, 2026-08-18: rule 1 is now reachable — see docs/23

**Everything above this addendum was true when written (2026-08-17) and
is left unedited, per the append-only discipline stated throughout this
document.** As of 2026-08-18, the two root causes this document
diagnoses — `is_changed_public_symbol` never seeing `changed`, and no
adapter declaring `concurrency_model` complete — are closed for
`node.changed_public_symbol` specifically, which now produces genuine
`async.concurrent_reentry` obligations on real Rust code with syntactic
concurrency evidence. This document's own prediction, quoted from the
paragraph above the corrective-addenda list, was correct: fixing this
"would let rule 1 produce real per-function obligations about concurrent
re-entry... It would not fix rules 2-5," and rules 2 through 5 remain
exactly where this document found them.

Full detail, verified independently against the actual commits and this
repository's own test suite, not copied from any commit message: see
`docs/23_current_capability_status.md`. That document is the current,
canonical answer to "what can ReviewGraphen do"; this diagnosis remains
the accurate historical record of the defect it found and its root
cause.
