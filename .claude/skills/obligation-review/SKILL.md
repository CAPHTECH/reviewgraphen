---
name: obligation-review
description: Run an obligation-driven code review — the ReviewGraphen methodology — when asked to review source code the ReviewGraphen way, or when told you are the reviewer in a ReviewGraphen review process. Enumerate a finite set of review obligations before forming any opinion, then keep claims, evidence, and verification separate, instead of an unstructured "find bugs" pass. This is the current methodology skill; `reviewgraphen-methodology` is a frozen experiment artifact and must not be used for new work.
---

# Obligation-driven review — the ReviewGraphen methodology

This describes a **methodology**, not a piece of software. Every structural
claim below is drawn from this repository's own design documents and is
cited to its source; nothing here is invented procedure. Sources:
`docs/03_conceptual_model.md`, `docs/05_system_architecture.md`,
`docs/06_program_space_ingestion.md`, `docs/07_review_obligation_model.md`,
`docs/08_context_cover_gluing.md`,
`docs/09_review_execution_and_agent_protocol.md`,
`docs/10_evidence_and_verification.md`,
`docs/adr/0011-program-space-v2-capability-trace.md`.

## Relationship to the frozen `reviewgraphen-methodology` skill

`.claude/skills/reviewgraphen-methodology/SKILL.md` is a **frozen experiment
artifact**, not a skill to invoke. It is the declared treatment of
`m7-head-local-v1`'s `qwen_skill` arm
(`benchmarks/m7-head-local-v1/preregistration.json`), it is the only surviving
committed copy of that treatment text, and it is pinned three ways:
`benchmarks/m7-head-local-v1/scripts/build_skill_packets.sh` hard-fails on any
drift in its body (`a43d6984…`), and m9's and m10's pre/post-loop tree
manifests record its whole-file hash (`1ddb0ed1…`) **at that exact path**. It is
therefore never edited, never moved, and never deleted. Use it only to
reproduce those experiments; use **this document for all review work**.

This skill differs from that frozen text in exactly one structural respect,
for a measured reason: **the frozen text made the reviewer perform the
obligation-enumeration step itself, and that is the step that exhausts a local
model's budget.** It said so in its own opening line ("in this instance also as
the obligation-enumeration step ReviewGraphen's own engine would normally
perform for you") and then treated that substitution as the normal case. This
skill treats it as the fallback it is.

Measured in this repository's own benchmark program, same model
(`qwen3.8:27b-mlx`), same 65,536-token output cap:

| Configuration | Output tokens | Elapsed | Result |
| --- | --- | --- | --- |
| Free-form review, no obligation structure | 65,535 / 65,536 | 5,146 s | **0 bytes**, `reasoning_runaway` |
| Reviewer enumerates its own obligations | 64,421 / 65,536 | 3,051 s | valid (reasoning 60,128 / final 1,802) |
| Same, second unit | 65,535 / 65,536 | 3,070 s | **0 bytes**, `empty_final_after_process_completion` |
| **Obligations supplied; reviewer only processes them** | **15,159 / 65,536** | **806 s** | valid |

Sources: `benchmarks/m7-local-factorial-v3/STAGE1_STATUS_SUMMARY.md:394`,
`benchmarks/m7-head-local-v1/OUTPUT_CAP_STOP_AND_PROBE.md:27-28`.

Two conclusions follow, and they drive this document's structure:

1. **Dropping the record does not make the review cheaper.** The unstructured
   arm consumed the entire budget and produced nothing. The record is not the
   overhead; it is what keeps generation bounded.
2. **Producing the record is what costs, not emitting it.** In the completed
   runs the final content was 1,802 and 453 tokens. The expensive part is the
   reasoning needed to *derive* the ProgramSpace inventory and the obligation
   set — and that derivation is a deterministic engine's job (`docs/05` §6-7),
   not a reviewer's.

## Why this exists (docs/07 §1, docs/03 §16)

The largest cause of missed defects in AI review is that no finite set of
"things to check" ever existed — the reviewer just reads code and writes
whatever occurs to it. ReviewGraphen's answer is to make that set explicit and
finite *before* forming any opinion about whether the code is correct:
enumerate what must be checked (obligations), then check each one and keep the
resulting claim separate from whatever evidence does or doesn't support it. A
finding is not the center of this process — it is one projection that falls out
at the end, after an obligation has been executed, a claim proposed, evidence
bound, and verification attempted (docs/03 §16).

## The pipeline, and who produces each stage (docs/05 §6-7, docs/03 §2-3)

```
Repository / snapshot          ─┐
  -> ProgramSpace               │ deterministic side: tooling, not the reviewer
  -> Review Obligation Universe │ (docs/05 §6 "Deterministic side": parser output,
  -> Review Plan                │  symbol table, accepted dependency facts, rule
                               ─┘  matching, obligation ID generation, context
                                   source selection)
  -> Context Projection(s)     ─┐
  -> Review Execution           │ the reviewer's work: one obligation, one
  -> Review Claim               │ bounded context, one claim, evidence status
  -> Evidence + Verification    │
  -> Coverage / Gluing         ─┘
```

This split is not a convenience. `docs/03` §15.9 requires that obligation IDs
be **stable** for the same snapshot/profile/rules/extractors, and `docs/05` §14
lists "same input → same obligation IDs" as a required architecture test. A set
a language model enumerated from prose cannot satisfy either. A
reviewer-enumerated set is therefore a degraded substitute even when it is the
only option — never the target state.

## Choose a mode before you start

### Mode A — obligations supplied (preferred; use whenever possible)

You are given, per unit of work, a set of obligations produced outside your
context: by ReviewGraphen's rule pack, by deterministic tooling, or by a
separate enumeration pass. **Skip Stages 1 and 2 entirely.** Go to Stage 3.

Building the inputs for Mode A does not require the ReviewGraphen binary. The
tiers below are ordinary tooling output:

- Tier 0 — repository/change facts: `git diff --name-status <base>..<head>`,
  `git log`, file languages.
- Tier 1 — syntax and symbols: any parser or index over the changed files
  (`syn`, tree-sitter, ctags, `cargo metadata`, an LSP dump).
- Higher tiers — calls, ownership, tests: only what the tooling actually
  resolved. Everything else is an explicit unknown carried into the packet.

Obligations can then be generated from those facts by a fixed rule list rather
than by generation — e.g. "each changed public function → one node obligation
for its property class", "each changed caller/callee pair → one relation
obligation". This is what `docs/07` §5 "Deterministic generation" describes.

If a stronger model is available to build the packet and a weaker one will
execute it, put the enumeration on the stronger model and the execution on the
weaker one. The measured 4.3× budget difference above is between exactly these
two arrangements.

### Mode B — no supplied obligations (fallback)

You were handed raw source and nothing else, and no enumeration step exists
upstream. Then you perform Stages 1 and 2 yourself, knowing that this is a
substitution for a deterministic engine, that the resulting set is not
ID-stable, and that it is the single largest consumer of your budget. Say so in
your output.

## Stage 1 — ProgramSpace (Mode B only; docs/06 §1-3)

ProgramSpace ingestion does not mean "I understood the repository." It means
building **accepted structural facts** plus **explicit unknowns** — a
before-you-form-any-opinion inventory, layered so a lower tier still counts even
if a higher one is unavailable to you. Use the same Tier 0 / Tier 1 / higher-tier
split listed under Mode A.

Anything you cannot establish is an explicit unknown, not a fact. Do not
silently treat "I didn't check this" as "this is fine."

**Keep this inventory internal.** Emit only what a later stage cites. A
narrated walkthrough of every file you looked at is generation cost with no
consumer; in the measured failures it is where the budget went.

## Stage 2 — Review Obligation Universe (Mode B only; docs/03 §5, docs/07 §1-4, §12)

Before reading for defects, enumerate a finite set of **ReviewObligations**.
Each obligation is:

| Field | Meaning (docs/03 §5) |
| --- | --- |
| target | what is being checked — see target kinds below |
| property | a specific, named thing to verify — not a vague question |
| context requirement | what neighborhood/information this needs |
| evidence requirement | what would actually support a claim about it |
| risk | impact / exposure / likelihood, as a first estimate |
| provenance | why you generated this obligation |

**Target kinds** (docs/03 §5.1, docs/07 §3):

- **Node** — one function/symbol (e.g. input validation on a public function,
  resource release in a lifecycle method).
- **Relation** — one edge between two things (caller/callee error contract, a
  UI-to-persistence dependency, a model/wire-schema nullability match).
  Relations matter because code can be locally correct at every node while
  still being wrong at the edges between nodes.
- **Subgraph** — a semantically meaningful group, not just a k-hop
  neighborhood (a whole feature, a pipeline, an authentication boundary).
- **Path** — an ordered sequence (untrusted input -> parser -> command
  execution; begin transaction -> writes -> commit/rollback). Order is part of
  the target's identity: a path and its reverse are different obligations even
  over the same edges.
- **Invariant** — a property that must hold globally, not at one location
  (payment happens at most once; disposed objects are never updated
  afterward).
- **Morphism** — what must be preserved across a change (interface
  preservation, an invariant that used to hold and might not anymore, a test
  relation that got lost).

**Property vocabulary** — draw properties from this initial rule-pack listing
(docs/07 §12) rather than inventing ad hoc phrasing; extend it with a
versioned-style name (`domain.specific_property`) only when nothing here fits:

```
baseline: changed-symbol, public-api, error-propagation, test-relation
async: concurrent-reentry, cancellation, lifecycle-after-dispose
security: auth-reachability, untrusted-input-flow, secret-exposure
architecture: context-boundary, forbidden-dependency
persistence: transaction, idempotency, cache-invalidation
```

Do not enumerate an unbounded number of obligations. Prioritize by what
actually changed (if you know), public/external-facing surface, and risk. It is
legitimate, and expected, for most of what you read to not become an obligation
at all.

**Obligation budget.** ReviewGraphen's design gives the Review Plan stage a
budget concept — enumeration is meant to be budget-aware, not unbounded
(docs/07 §13 "Obligation explosion", listing "budget-aware deferral" and
"low-risk obligation sampling"; docs/09 §2's `ReviewPlan` carries an explicit
`budget:` field; docs/11 §7 frames obligation selection as a budget-constrained
optimization). None of those documents states a concrete number.

**Default cap in Mode B: 8 obligations, total, across the whole input.** This is
an execution constraint, not part of the methodology, and it is empirical: at
this cap a 27B local model completed one unit at 64,421/65,536 output tokens
and exhausted the budget on the next
(`benchmarks/m7-head-local-v1/OUTPUT_CAP_STOP_AND_PROBE.md:27-28`;
`QWEN_SKILL_ARM_AMENDMENT.md:96` records it as experiment-side). **8 is at the
edge of feasibility, not a safe default** — for a model at or below that scale,
prefer Mode A, or lower the cap. In Mode A the cap does not apply: the supplied
set governs, and you may process it one obligation per request.

If you notice partway through that you are consuming a large share of your
available output while still enumerating or processing obligations, stop
enumerating new ones and finalize your output with what you have completed
(docs/09 §9's `budget_exhausted` abstention reason; docs/07 §13's "budget-aware
deferral"). Do not keep going in the hope of finishing everything.

## Stage 3 — Context Projection (docs/03 §6, docs/08)

For each obligation, use only the source relevant to it — not the whole input
indiscriminately. Note explicitly what you are leaving out (files you were not
given, call targets you could not resolve, cross-file effects you cannot see).
A local conclusion about one context does not automatically become a global
conclusion — if two parts of your review would need to agree with each other to
support a broader claim and you cannot actually check that they do, say so
rather than assuming it (docs/03 §12, docs/08).

## Stage 4 — Review Execution and Claim (docs/09 §3-4, §7-9, docs/03 §8)

For each obligation, produce exactly one **ReviewClaim** with one of these
polarities (docs/03 §8):

- `issue_present` — you believe a defect or rule violation exists.
- `issue_absent` — you checked and did not find a problem, **within the checked
  scope only**. This is not "safe" and not a general guarantee — state what you
  actually checked and its bound.
- `inconclusive` — you don't have enough to decide either way.
- `not_applicable` — the obligation's own precondition doesn't hold here.
- `conflict` — this claim doesn't sit well with another claim or fact you have;
  say so rather than silently picking one.

Every claim must cite **source IDs** — the specific file path, symbol, or line
range it is grounded in (docs/09 §8). A claim with no source citation is an
unsupported proposal, not a finding — it must not be reported as if it were
checked.

**Abstain rather than force a claim** when (docs/09 §9): the context is
insufficient, a symbol is unresolved, required evidence isn't available to you,
you don't actually understand the property being asked, sources conflict, or
you've hit your own budget. An abstention is not an error and is not the same as
"no issue" — record it as what it is.

## Stage 5 — Evidence, Verification, and the discipline that matters most (docs/10)

**This is the part that turns a review from an opinion into something someone
can act on. Read this section twice.**

> Principle: Claim is not Evidence. (docs/10, header)

A **Claim** is "this problem might exist" or "I checked and didn't find one."
**Evidence** is an actual observation — a test result, a compiler diagnostic, a
resolved call edge, a counterexample — that supports or refutes a claim.
**Verification** is the record of a specific procedure having been applied to
check a claim against evidence. These are three different things with three
different IDs (docs/03 §15.1, docs/10 §16.2), and you must never collapse them:

- Writing a plausible-sounding rationale is **not** evidence.
- Another model (including you, asked twice) agreeing with a claim is **not**
  evidence — it is, at best, a cross-check, and must never be labeled
  `machine_checked` or treated as proof (docs/10 §10, §16.6). "Two reviewers
  agree" is not the same fact as "a test failed." Independent agreement can
  raise priority; it never substitutes for verifying the actual program
  property.
- You, reading code and reasoning about it, are producing a **Claim**, not a
  **Verification** — unless you actually ran something. If you have no test
  runner, compiler, or execution environment in this context, say so. Do not
  write "confirmed" or "verified" for anything you have not actually run or
  mechanically checked.
- If you believe a claim needs a specific kind of evidence to be trusted (a
  reproduction test, a type check, a counterexample), say what that evidence
  would be (`requested_evidence`) instead of asserting the claim is settled.
- A negative claim (`issue_absent`) is especially easy to overstate. State its
  checked scope, its bound, and what you did **not** check (unresolved
  dispatch, paths outside what you were given, etc.) — do not write "no issue"
  as an unqualified, general statement (docs/10 §7).
- If two things you noticed disagree with each other, do not silently resolve it
  by picking the more plausible one — record the conflict (docs/10 §12).

## Stage 6 — Coverage and honesty about scope (docs/03 §11, docs/09 §16)

You are not reviewing "the codebase." You are executing a bounded set of
obligations against a bounded context. State plainly what you did not get to,
what you could not resolve, and where your obligation set was incomplete. In
Mode B, state that the obligation set was reviewer-enumerated and therefore not
ID-stable (docs/03 §15.9) — coverage over such a set is descriptive, not a
versioned-universe coverage figure (docs/03 §15.3).

Persona-switching ("as a senior engineer... as a security expert...") does not
increase coverage and is not part of this methodology (docs/09 §5-6) — a
genuinely different angle requires a different evidence source or an
independent failure mode, not a different voice over the same information.

## What you must never do

1. Never present a Claim as if it were Evidence or a Verification result.
2. Never write "verified" or "confirmed" without an actual check you performed
   (a citation to a resolved fact is not the same as running a test).
3. Never generalize an `issue_absent` claim beyond the property and context you
   actually checked.
4. Never invent source citations. If you cannot point to the specific location,
   the claim is unsupported — say so, do not omit the citation requirement
   silently.
5. Never expand scope beyond your obligations because something else looked
   interesting — either make it a new obligation explicitly, or leave it out.
6. Never treat model self-agreement (including your own reasoning sounding
   confident) as evidence.
7. Never present a Mode B reviewer-enumerated obligation set as if it were an
   engine-generated, ID-stable universe.

## Output mapping

Emit the record; do not narrate the derivation. Everything below is final
content, which is cheap — in the measured completed runs it was 1,802 and 453
tokens. Reasoning that produced it is not part of the output.

**Mode A — per-obligation response.** One obligation in, one object out. Keep it
minimal unless the caller supplied a schema:

```json
{
  "obligation_id": "<the id you were given>",
  "disposition": "issue_present | issue_absent | inconclusive | not_applicable | abstained",
  "locations": ["path/to/file.rs:120-138"],
  "rationale": "<the claim, and its evidence status>",
  "requested_evidence": "<what would settle it, if unverified>",
  "severity": "<only for issue_present>"
}
```

**Mode B — `reviewgraphen.benchmark.candidate_output.v1`**, if that is the
schema you were given, unmodified:

- For each obligation you enumerated in Stage 2, emit one entry in
  `obligation_results`. Assign your own stable `packet_id` (`obligation-1`,
  `obligation-2`, ...) since you enumerated it yourself. Set `disposition` to
  the Stage 4 claim polarity (`issue_present`, `issue_absent`, `inconclusive`,
  `not_applicable`; `abstained` for an obligation you abstained on). Only
  `issue_present` obligations list `finding_local_ids`.
- For each `issue_present` claim, emit one entry in `findings` with source
  citations as `locations`, a `mechanism_tags` value matching the kind of
  problem, a `rationale` stating the claim **and** its evidence status (e.g.
  "unverified — would need a reproduction test showing X"), and a `severity`
  reflecting your risk estimate.
- Do not emit a finding for `issue_absent`, `inconclusive`, or
  `not_applicable` — those are recorded only via `obligation_results`.

## Local-inference execution notes

These are measured conditions for small local models, not methodology. Ignore
them when the reviewer is a large hosted model.

- **One obligation per request.** Mode A's per-obligation form exists so that
  eight obligations are eight small requests, not one large one. This closes
  the window in which runaway reasoning can consume the whole budget.
- **Disable the thinking channel.** With thinking on, this program's local model
  emitted 130 KB of prose into `content` and hit its 32k ceiling on every arm;
  `enable_thinking=false` with `response_format=json_object` was required for
  any arm to complete (`docs/23_current_capability_status.md` §6.5).
- **Keep the output contract thin.** In the 2026-08-26 exploratory 10-pair run,
  **0 of 10 pairs scored** because every output failed a closed `observations`
  form, while the same raw outputs judged without the mechanical gate scored
  A 5 / B 4 / tie 1. That is contract conformance failing, not review quality
  (`docs/23` §6.5). Every required field costs conformance probability; require
  only fields a consumer reads.
- **Do not respond to budget pressure by dropping the obligation structure.**
  That is the free-form arm, and it is the configuration that produced 0 bytes
  after 86 minutes. Respond by moving enumeration upstream (Mode A), lowering
  the cap, or splitting into more requests.
