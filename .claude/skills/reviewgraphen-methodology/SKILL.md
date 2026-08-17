---
name: reviewgraphen-methodology
description: Apply the ReviewGraphen review methodology yourself when asked to review source code the ReviewGraphen way, or when told you are the reviewer in a ReviewGraphen review process. Use this to run an obligation-driven review that keeps claims and evidence separate, instead of an unstructured "find bugs" pass.
---

# ReviewGraphen review methodology

This describes a **methodology**, not a piece of software. You will not call
a ReviewGraphen tool; you will act as the reviewer, and in this instance
also as the obligation-enumeration step ReviewGraphen's own engine would
normally perform for you. Every structural claim below is drawn from this
repository's own design documents and is cited to its source; nothing here
is invented procedure. Sources: `docs/03_conceptual_model.md`,
`docs/06_program_space_ingestion.md`, `docs/07_review_obligation_model.md`,
`docs/08_context_cover_gluing.md`, `docs/09_review_execution_and_agent_protocol.md`,
`docs/10_evidence_and_verification.md`,
`docs/adr/0011-program-space-v2-capability-trace.md`.

## Why this exists (docs/07 §1, docs/03 §16)

The largest cause of missed defects in AI review is that no finite set of
"things to check" ever existed — the reviewer just reads code and writes
whatever occurs to it. ReviewGraphen's answer is to make that set explicit
and finite *before* forming any opinion about whether the code is correct:
enumerate what must be checked (obligations), then check each one and keep
the resulting claim separate from whatever evidence does or doesn't support
it. A finding is not the center of this process — it is one projection that
falls out at the end, after an obligation has been executed, a claim
proposed, evidence bound, and verification attempted (docs/03 §16).

## The pipeline (docs/03 §2-3, §16)

```
Repository / snapshot
  -> ProgramSpace              (accepted structural facts + explicit unknowns)
  -> Review Obligation Universe (the finite set of things to check)
  -> Review Plan                (which obligations, in what order, under what budget)
  -> Context Projection(s)      (bounded, per-obligation view of the ProgramSpace)
  -> Review Execution           (one reviewer processing one obligation + its context)
  -> Review Claim               (a proposed answer: issue_present / issue_absent / ...)
  -> Evidence + Verification    (does anything actually support the claim?)
  -> Coverage / Gluing / Obstructions (what fraction was checked, and where local
                                        conclusions do or don't combine into global ones)
```

You are executing this pipeline yourself, end to end, on the source you are
given. Follow the stages in this order. Do not skip straight to writing
findings.

### Stage 1 — ProgramSpace (docs/06 §1-3)

ProgramSpace ingestion does not mean "I understood the repository." It means
building **accepted structural facts** plus **explicit unknowns** — a
before-you-form-any-opinion inventory, layered so a lower tier still counts
even if a higher one is unavailable to you:

- Tier 0 — repository/change facts: which files, languages, and (if you can
  tell from what you were given) what changed.
- Tier 1 — syntax and symbols: modules, types, functions, methods, fields —
  what is actually declared.
- Higher tiers (calls, data flow, ownership, tests) only if you can actually
  establish them from what you were given — do not assume you can trace a
  call graph, ownership boundary, or test-to-code mapping you have not
  actually read.

Anything you cannot establish is an explicit unknown, not a fact. Do not
silently treat "I didn't check this" as "this is fine."

### Stage 2 — Review Obligation Universe (docs/03 §5, docs/07 §1-4, §12)

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

- **Node** — one function/symbol (e.g. input validation on a public
  function, resource release in a lifecycle method).
- **Relation** — one edge between two things (caller/callee error contract,
  a UI-to-persistence dependency, a model/wire-schema nullability match).
  Relations matter because code can be locally correct at every node while
  still being wrong at the edges between nodes.
- **Subgraph** — a semantically meaningful group, not just a k-hop
  neighborhood (a whole feature, a pipeline, an authentication boundary).
- **Path** — an ordered sequence (untrusted input -> parser -> command
  execution; begin transaction -> writes -> commit/rollback). Order is part
  of the target's identity: a path and its reverse are different
  obligations even over the same edges.
- **Invariant** — a property that must hold globally, not at one location
  (payment happens at most once; disposed objects are never updated
  afterward).
- **Morphism** — what must be preserved across a change (interface
  preservation, an invariant that used to hold and might not anymore, a
  test relation that got lost).

**Property vocabulary** — draw properties from this initial rule-pack
listing (docs/07 §12) rather than inventing ad hoc phrasing; extend it with
a versioned-style name (`domain.specific_property`) only when nothing here
fits:

```
baseline: changed-symbol, public-api, error-propagation, test-relation
async: concurrent-reentry, cancellation, lifecycle-after-dispose
security: auth-reachability, untrusted-input-flow, secret-exposure
architecture: context-boundary, forbidden-dependency
persistence: transaction, idempotency, cache-invalidation
```

Do not enumerate an unbounded number of obligations. Prioritize by what
actually changed (if you know), public/external-facing surface, and risk.
It is legitimate, and expected, for most of what you read to not become an
obligation at all.

**Obligation budget.** ReviewGraphen's own design gives the Review Plan
stage a budget concept — obligation enumeration is meant to be
budget-aware, not unbounded (docs/07 §13 "Obligation explosion", listing
"budget-aware deferral" and "low-risk obligation sampling" among its
countermeasures; docs/09 §2's `ReviewPlan` example carries an explicit
`budget:` field; docs/11 §7 frames obligation selection itself as a
budget-constrained optimization). None of those documents states a
concrete number — the number below is **not** drawn from them.

**Hard cap for this execution: enumerate at most 8 obligations in Stage 2,
total, across the whole input.** This number is an experiment-side
execution constraint, not part of the ReviewGraphen methodology itself,
and is set here for this specific run for a documented, empirical reason:
in this same benchmark program, this model given an unstructured
free-form review task consumed 65,535 of its 65,536-token output budget
on reasoning and produced zero final content (`m7-local-factorial-v3`
snapshot-06 B1: 5,146.5s; `m7-head-local-v1` head-local-00: 6,548.8s —
both the identical failure mode, `empty_final_after_process_completion`).
The one comparable run that completed within budget used only 15,159 of
65,536 output tokens — but for a fixed 5-obligation set the model did not
have to generate itself. Enumerating obligations yourself, per-obligation
reasoning, source citation, and explicit evidence-status reasoning is
strictly more generation work per obligation than that completed run
required, and unstructured generation is exactly what exhausted the
budget in the failed run. 8 is chosen as a small, conservative multiple of
the 5-obligation case that completed in budget, not a derived or
guaranteed-safe number — it is a judgment call under real uncertainty
about this model's per-obligation cost, made because guessing too high
risks repeating the exact failure this cap exists to avoid, and guessing
too low only costs coverage, not budget.

If you notice partway through that you are consuming a large share of
your available output while still enumerating or processing obligations,
stop enumerating new ones and finalize your output with what you have
completed (docs/09 §9's `budget_exhausted` abstention reason; docs/07 §13's
"budget-aware deferral" — apply that concept here even though neither
document gives a specific trigger point for it). Do not keep going in the
hope of finishing everything.

### Stage 3 — Context Projection (docs/03 §6, docs/08)

For each obligation, use only the source you can actually see that is
relevant to it — not the whole input indiscriminately. Note explicitly what
you are leaving out (files you were not given, call targets you could not
resolve, cross-file effects you cannot see). A local conclusion about one
context does not automatically become a global conclusion — if two parts of
your review would need to agree with each other to support a broader claim
and you cannot actually check that they do, say so rather than assuming it
(docs/03 §12, docs/08).

### Stage 4 — Review Execution and Claim (docs/09 §3-4, §7-9, docs/03 §8)

For each obligation, produce exactly one **ReviewClaim** with one of these
polarities (docs/03 §8):

- `issue_present` — you believe a defect or rule violation exists.
- `issue_absent` — you checked and did not find a problem, **within the
  checked scope only**. This is not "safe" and not a general guarantee —
  state what you actually checked and its bound.
- `inconclusive` — you don't have enough to decide either way.
- `not_applicable` — the obligation's own precondition doesn't hold here.
- `conflict` — this claim doesn't sit well with another claim or fact you
  have; say so rather than silently picking one.

Every claim must cite **source IDs** — the specific file path, symbol, or
line range it is grounded in (docs/09 §8). A claim with no source citation
is an unsupported proposal, not a finding — it must not be reported as if
it were checked.

**Abstain rather than force a claim** when (docs/09 §9): the context is
insufficient, a symbol is unresolved, required evidence isn't available to
you, you don't actually understand the property being asked, sources
conflict, or you've hit your own budget. An abstention is not an error and
is not the same as "no issue" — record it as what it is.

### Stage 5 — Evidence, Verification, and the discipline that matters most (docs/10)

**This is the part that turns a review from an opinion into something
someone can act on. Read this section twice.**

> Principle: Claim is not Evidence. (docs/10, header)

A **Claim** is "this problem might exist" or "I checked and didn't find
one." **Evidence** is an actual observation — a test result, a compiler
diagnostic, a resolved call edge, a counterexample — that supports or
refutes a claim. **Verification** is the record of a specific procedure
having been applied to check a claim against evidence. These are three
different things with three different IDs (docs/03 §15.1, docs/10 §16.2),
and you must never collapse them:

- Writing a plausible-sounding rationale is **not** evidence.
- Another model (including you, asked twice) agreeing with a claim is
  **not** evidence — it is, at best, a cross-check, and must never be
  labeled `machine_checked` or treated as proof (docs/10 §10, §16.6).
  "Two reviewers agree" is not the same fact as "a test failed."
  Independent agreement can raise priority; it never substitutes for
  verifying the actual program property.
- You, reading code and reasoning about it, are producing a **Claim**, not
  a **Verification** — you have no test runner, no compiler, no execution
  environment here. Say so. Do not write "confirmed" or "verified" for
  anything you have not actually run or mechanically checked.
- If you believe a claim needs a specific kind of evidence to be trusted
  (a reproduction test, a type check, a counterexample), say what that
  evidence would be (`requested_evidence`) instead of asserting the claim
  is already settled.
- A negative claim (`issue_absent`) is especially easy to overstate. State
  its checked scope, its bound, and what you did **not** check (unresolved
  dispatch, paths outside what you were given, etc.) — do not write
  "no issue" as an unqualified, general statement (docs/10 §7).
- If two things you noticed disagree with each other, do not silently
  resolve it by picking the more plausible one — record the conflict
  (docs/10 §12).

### Stage 6 — Coverage and honesty about scope (docs/03 §11, docs/09 §16)

You are not reviewing "the codebase." You are executing a bounded set of
obligations against a bounded context. State plainly what you did not get
to, what you could not resolve, and where your obligation set was
incomplete. Do not imply full coverage. Persona-switching ("as a senior
engineer... as a security expert...") does not increase coverage and is not
part of this methodology (docs/09 §5-6) — if you want a genuinely different
angle, that requires an actually different evidence source or independent
failure mode, not a different voice using the same information.

## What you must never do

1. Never present a Claim as if it were Evidence or a Verification result.
2. Never write "verified" or "confirmed" without an actual check you
   performed (a citation to a resolved fact is not the same as running a
   test).
3. Never generalize an `issue_absent` claim beyond the property and context
   you actually checked.
4. Never invent source citations. If you cannot point to the specific
   location, the claim is unsupported — say so, do not omit the citation
   requirement silently.
5. Never expand scope beyond the obligations you enumerated because
   something else looked interesting — either make it a new obligation
   explicitly, or leave it out.
6. Never treat model self-agreement (including your own reasoning sounding
   confident) as evidence.

## Output mapping

Report your obligations and claims using the schema you were given
(`reviewgraphen.benchmark.candidate_output.v1`), unmodified:

- For each obligation you enumerated in Stage 2, emit one entry in
  `obligation_results`. Assign it your own stable `packet_id` (e.g.
  `obligation-1`, `obligation-2`, ...) since you enumerated it yourself
  rather than receiving it from a pre-built packet. Set `disposition` to
  the claim polarity from Stage 4 (`issue_present`, `issue_absent`,
  `inconclusive`, or `not_applicable`; use `abstained` for an obligation you
  abstained on per Stage 4). Only `issue_present` obligations should list
  `finding_local_ids`.
- For each `issue_present` claim, emit one entry in `findings` with your
  source citations as `locations`, a `mechanism_tags` value that best
  matches what kind of problem you found, your `rationale` stating the
  claim **and** its evidence status (e.g. "unverified — would need a
  reproduction test showing X"), and a `severity` reflecting your risk
  estimate.
- Do not emit a finding for `issue_absent`, `inconclusive`, or
  `not_applicable` obligations — those are recorded only via
  `obligation_results`.
