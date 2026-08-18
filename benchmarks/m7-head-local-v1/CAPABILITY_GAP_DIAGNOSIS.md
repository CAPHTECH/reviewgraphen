# Diagnosis: ReviewGraphen's obligation synthesis is structurally degenerate for real Rust code

Status: diagnosis only, requested by the operator before any fix decision.
No code is changed by this document. All claims below are cited to exact
file/line locations or directly-read artifacts; nothing here is inferred
without a citation.

## Question 1 — does origin_rule fire while required_capabilities stay undeclared?

**Confirmed, exactly as the operator's hypothesis stated.**
`MvpRulePack::synthesize` (`crates/reviewgraphen-core/src/synthesize.rs`)
runs two passes. The first pass (lines 548-797) walks the actual program
facts (artifacts, relations) and emits substantive obligations when a
rule's structural precondition matches. The second pass (lines 804-851,
comment at 799-803: "A capability gap is not an exclusion... Emit it even
when concrete facts exist") runs **unconditionally, once per rule
descriptor**, independent of whether the first pass found anything, and
checks `capability_fully_available` (line 1089) for every one of that
rule's `required_capabilities`. If any capability required by that rule is
not `CapabilityState::Complete`, a `reviewgraphen.capability_gap` fallback
obligation is emitted for that rule, with `applicability_status: "unknown"`
and `applicability_reasons` naming which capability was missing and
why (`capability_gap_reason`, lines 1107-1123, distinguishing
`capability_partial` / `capability_missing` / `capability_unknown` /
`capability_undeclared`).

Verified directly against `head-local-08`'s built packet
(`agent_input/obligations.json`): all 5 obligations are
`property_id: "reviewgraphen.capability_gap"`, `applicability_status:
"unknown"`, and each carries `origin_rule:<rule-id>` in
`applicability_reasons` alongside the specific missing-capability tags —
e.g. `["capability_undeclared:concurrency_model",
"origin_rule:node.changed_public_symbol@1"]`. This is a direct,
mechanical read of the artifact, not an inference.

## Question 2 — where are these capabilities declared, and why doesn't fsl have them?

Read directly from `crates/reviewgraphen-ingest/src/rust.rs` (the only
Rust-source ingest adapter; lines 113-134), for **every** Rust snapshot
this adapter ever processes, regardless of content:

- `ast`, `containment`: `Complete` if the file parses, `Partial` if it
  doesn't. fsl's files parse (the observed reason never lists `ast`), so
  this pair is not the blocker.
- `direct_calls`, `imports`, `module_dependencies`, `test_mapping`,
  `state_writes`: **always `Partial`, unconditionally, by explicit
  design** — the code comment (lines 136-141) states these five are
  "unconditionally `partial`... even a snapshot with no unresolved
  instance is still bounded to unique local syntactic targets," and that
  this is retained "deterministically instead of only emitting it when a
  specific unresolved call/import/write is observed." This is a
  deliberate, permanent policy of this adapter: it will never report
  `Complete` for these five, for any Rust input, because its call/import/
  test-mapping resolution is genuinely bounded to local syntactic
  resolution (no cross-crate or dynamic-dispatch resolution) and the
  adapter authors chose to always disclose that bound rather than claim
  completeness on inputs that happen not to exercise it.
- `concurrency_model`: **never inserted into the capabilities map at
  all**, anywhere in `rust.rs`. Grepped across every file in
  `crates/reviewgraphen-ingest/`: no adapter ever declares this
  capability. It is not fsl-specific — no capability of this name is
  implemented by any ingest adapter in this codebase, for any input.

**This is not an fsl-specific gap.** Every rule in `MvpRulePack` requires
either `concurrency_model` (never declared by any adapter) or one of the
five permanently-`Partial` capabilities (`direct_calls`/`test_mapping`).
Given `capability_fully_available` accepts only `Complete` (line
1089-1098, with an explicit doc comment explaining `partial` must not be
silently treated as available), **no rule in `MvpRulePack` can ever
produce a substantive obligation from real Rust ingestion, for any
codebase** — this is a structural property of the current adapter/rule-
pack pairing, not a per-input or per-project condition.

What declaring `concurrency_model` would require: implementing an actual
concurrency-fact extractor in the Rust ingest adapter (e.g., detecting
`async`/`await`, thread-spawn, lock-acquisition, or actor/handler dispatch
patterns) and inserting a `CapabilityState` for it — this does not exist
today in any form, complete or partial.

## Question 3 — would declaring capabilities produce substantive obligations, and which?

**Partially yes, for one rule; no, for the other four**, verified from
`rule_contract` (`synthesize.rs` lines 1401-1465) and a repository-wide
grep for the relation kinds/attributes the other four rules pattern-match
on.

`MvpRulePack` defines exactly 5 rules. Every one of their `rationale`
fields is the **identical literal string**: `"External payment side
effect is reachable from a repeatable UI event."` This is not five
general code-review properties; it is one hand-built scenario (a payment
double-submit / idempotency scenario — consistent with property IDs
`async.concurrent_reentry` and `payment.idempotency_contract`, and with
`crate::m5::DOUBLE_SUBMIT_PROFILE_ID` referenced in the same function),
expressed as 5 rule variants:

1. `node.changed_public_symbol@1` (property `async.concurrent_reentry`):
   fires on **any** changed public function (line 550,
   `is_changed_public_symbol` + `public` attribute — no payment-specific
   gate at all). Requires `ast` (already `Complete`) and
   `concurrency_model` (never declared). **If `concurrency_model` were
   implemented, this rule would produce one substantive obligation per
   changed public function on real code, fsl included** — a real,
   reachable review property (though narrowly "does this function admit
   concurrent re-entry," not general defect review).
2. `relation.concurrent_reentry@1`: requires a `handled_by` relation with
   attribute `concurrency == "unbounded_reentry"`.
3. `relation.changed_call_contract@1`: requires a `calls` relation with
   `idempotency_key_forwarded == false` reaching an artifact attributed
   `external_side_effect`.
4. `path.external_side_effect@1` and 5. `invariant.payment_at_most_once@1`:
   require the same `handled_by`/`calls`/`covers`/`constrains` relation
   graph as above.

Grepped across every file in `crates/reviewgraphen-ingest/` (the real
extraction adapters): the strings `unbounded_reentry`,
`idempotency_key_forwarded`, `external_side_effect`, and the relation kind
`"handled_by"` **never appear**. They exist only in
`crates/reviewgraphen-core/src/synthesize.rs` (which checks for them) and
inside `crates/reviewgraphen-store/src/journal.rs`'s `#[cfg(test)]` module
(confirmed: the two occurrences at lines 18917/18938 fall inside the `mod
tests` block opened at line 12050) — i.e., these relation kinds and
attributes are **only ever produced by hand-written test fixtures**, never
by real ingestion of any codebase. **Rules 2-5 are structurally
unreachable on any real-ingested input, independent of capability
declarations**, because nothing in the real pipeline ever produces the
facts they pattern-match on.

## Question 4 — is empty `context_ids` a consequence of capability_gap or a separate defect?

**A deliberate, by-design consequence — not a separate defect.** The
capability-gap fallback's `ObligationSpec` (`synthesize.rs` line 836)
hardcodes `context_ids: Vec::new()`. Compare the substantive rule 1's
`ObligationSpec` (line 574): `context_ids:
contexts_for_node(program, &artifact.id)`, a real function call that
resolves context bundles for that specific artifact. A capability-gap
obligation targets `target_kind: "subgraph"` /
`target_refs: [snapshot_id]` — the whole snapshot, not a specific location
— so it has no single location to attach a context to; leaving
`context_ids` empty for this obligation type is consistent with what the
obligation is actually asserting ("this rule could not run at all"), not
a bug that dropped context linkage that should have existed. The
`contexts/` bundles present in the packet were built independently (from
`prepare_real_full_review`'s own context-construction pass) and are simply
never referenced by any obligation, because no non-capability-gap
obligation exists to reference them.

## Question 5 — does this affect prior experiments, and specifically the pilot-vs-real hypothesis?

Directly verified (read, not inferred):

| Experiment | Arm checked | Obligations | Result |
| --- | --- | --- | --- |
| `m7-pilot-v2` | `g3_proxy` (`p7m1-g3`) — pilot has no `full_reviewgraphen` arm at all, only `b1`/`g3` | 5 | **100% `capability_gap`**, same 5 rule origins, same reason tags |
| `m7-local-factorial-v2` | `full_reviewgraphen`, snapshot-06 (already built in `/tmp`) | 5 | **100% `capability_gap`** |
| `m7-local-factorial-v2` | `full_reviewgraphen`, snapshot-40 (operator's own direct check) | 5 | **100% `capability_gap`** |
| `m7-head-local-v1` | `full`, `head-local-08` (this experiment) | 5 | **100% `capability_gap`** |

`m7-real-v1`, `m7-real-v2`, and `m7-local-factorial-v1`/`v3`'s prepared
`agent_input`/`obligations.json` artifacts are **not present in this
repository** (not committed, or only ever materialized to `/tmp` and since
cleaned up) — I did not fabricate a result for them. However, all four
experiments call the same `MvpRulePack::synthesize` through the same
`rust.rs` ingest adapter documented above, which is content-independent
for this failure mode (it depends only on which capabilities the adapter
ever declares, never on what the source code contains), so the same
degeneration is expected there with high confidence, but is stated here as
expected-from-the-shared-code-path, not directly observed.

**The operator's pilot-vs-real hypothesis is refuted by direct evidence.**
`m7-pilot-v2`'s `g3_proxy` arm — the richer of its two arms, and the one
whose synthetic corpus the operator recalled scoring 6/6 against B1's 3/6
— is **exactly as capability-gap-degenerate as every real-ingestion full
arm checked above**. Pilot's higher score was not driven by substantive
ReviewGraphen obligations; `g3_proxy`'s obligations are the same 5
capability-gap placeholders. Whatever gave `g3_proxy` its higher score in
that experiment came from something else in the packet (the raw
`program-space-facts.json`, additional context/source visibility, or
chance), not from obligation-driven review guidance. This must not be
described as "ReviewGraphen's obligations helped in pilot but not in
real" — the obligations were equally absent in both.

## Answers to the operator's three questions

**Is ReviewGraphen currently able to generate substantive review
obligations for fsl?** Essentially no, through the current adapter/rule-
pack pairing. 4 of `MvpRulePack`'s 5 rules are structurally unreachable on
any real-ingested codebase (their trigger relations/attributes exist only
in test fixtures). The 5th (`node.changed_public_symbol@1`) is reachable
in principle — it would fire on every changed public function in fsl or
any other Rust project — but is currently blocked purely by one
unimplemented capability (`concurrency_model`), and even if unblocked it
would produce only one narrow property (`async.concurrent_reentry`), not
general-purpose defect review.

**What's missing, and is it configuration, unimplemented capability, or
fsl-specific?** Not configuration, and not fsl-specific. It is: (a) one
genuinely unimplemented capability (`concurrency_model`) that no adapter
in this codebase declares for any input; (b) a deliberate, by-design
permanent cap on five other capabilities (`direct_calls`, `test_mapping`,
etc.) that the adapter's own documentation explains is a real bound on
what local-syntactic-only Rust extraction can claim, not an oversight; and
(c) a rule pack (`MvpRulePack`) that is a single hand-built payment/
double-submit demo scenario (evidenced by all 5 rules sharing one literal
rationale string), not a general Rust code-review rule set — most of its
rules were never intended to fire outside that one fixture scenario.

**If fixable, what would it take?** Implementing `concurrency_model`
extraction in `rust.rs` (a bounded, real engineering task — detecting
async/await, thread/lock, or handler-dispatch patterns) would let rule 1
produce real per-function obligations about concurrent re-entry, on fsl
and on any other Rust project reviewed this way. It would not fix rules
2-5, and it would not make ReviewGraphen review fsl (or any project)
generally — only that one narrow property. Making `full_reviewgraphen`
produce general-purpose substantive review obligations for arbitrary Rust
code would require writing a new, non-fixture-specific rule pack, which is
a substantially larger undertaking than declaring one capability, and is
outside the scope of a diagnosis.

No fix is proposed or implemented by this document, per the operator's
instruction. This is a report for the operator's decision.

## Addendum, 2026-08-18: the fix this document sized was implemented — see docs/23

**Everything above this addendum was true when written and is left
unedited.** This document's own "If fixable, what would it take?"
section predicted precisely what was implemented on 2026-08-18:
`concurrency_model` extraction was added to `rust.rs`, rule 1
(`node.changed_public_symbol`) now produces real per-function
`async.concurrent_reentry` obligations on real Rust code with
concurrency evidence, and — exactly as predicted — rules 2 through 5
were not fixed and remain scoped to the double-submit payment fixture.
This diagnosis's finding that ReviewGraphen (as of 2026-08-17) could not
generate substantive review obligations for fsl was correct **for that
date**; it is not the current state of the system.

Full detail, independently verified against the actual commits and this
repository's own test suite: see `docs/23_current_capability_status.md`,
the current canonical answer to "what can ReviewGraphen do." This
diagnosis remains the accurate historical record of what m7-head-local-v1
actually measured and why.
