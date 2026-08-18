---
name: reviewgraphen-implementation-methodology
description: Apply the ReviewGraphen methodology to a code *change* rather than to a review. Use when you are asked to implement a specified change the ReviewGraphen way — enumerate what the change must satisfy and must preserve before writing any code, then keep the resulting claims separate from the evidence that a compiler or a test runner actually produced.
---

# ReviewGraphen implementation methodology

This describes a **methodology**, not a piece of software. You will not call
a ReviewGraphen tool. You will act as the implementer, and also as the
obligation-enumeration step ReviewGraphen's engine would normally perform.

Every structural claim below is drawn from this repository's own design
documents and is cited to its source; nothing here is invented procedure.
Sources: `docs/03_conceptual_model.md` (§5, §5.1-5.2, §8, §11, §13, §15.1,
§16), `docs/06_program_space_ingestion.md` (§1-3),
`docs/07_review_obligation_model.md` (§3, §3.6, §4, §12, §13),
`docs/08_context_cover_gluing.md`,
`docs/09_review_execution_and_agent_protocol.md` (§2-9, §16),
`docs/10_evidence_and_verification.md` (§1-7, §12, §15, §16),
`docs/adr/0011-program-space-v2-capability-trace.md`.

The review form of this methodology stops at a Claim, because a reviewer
reading source has no compiler and no test runner and therefore can never
reach Verification (docs/10 §5). **Implementation is the case where that
limit does not apply.** A change is compiled and its tests are run, so
`executable evidence` — "unit/integration/e2e test result", "compiler
diagnostic", "reproduction script result" (docs/10 §2.2) — is actually
obtainable. This methodology exists to make sure that difference is used
honestly: to reach real Verification where a procedure was really run, and
to keep saying `claimed` everywhere else.

## Why this exists (docs/07 §1, docs/03 §16)

The same failure that makes AI review miss defects makes AI implementation
break things: no finite set of "what this change must satisfy" ever existed,
so the model writes plausible code and then reasons about whether it looks
right. ReviewGraphen's answer is to make that set explicit and finite
*before* writing any code: enumerate the obligations, then satisfy each one,
then keep the resulting claim separate from whatever evidence does or
doesn't support it.

## The pipeline (docs/03 §2-3, §13, §16)

A change from snapshot \(S_0\) to snapshot \(S_1\) is a **morphism**
\(m: S_0 \to S_1\) (docs/03 §13). This methodology runs the same pipeline the
review methodology runs, over that morphism:

```
Pre-change source (S0)
  -> ProgramSpace              (accepted structural facts + explicit unknowns)
  -> Change Obligation set     (the finite set of things the change must satisfy
                                and must preserve)
  -> Context Projection(s)     (bounded, per-obligation view of the source)
  -> The edit                  (the smallest change that discharges them)
  -> Claim per obligation      (satisfied / not satisfied / inconclusive / ...)
  -> Evidence + Verification   (what a compiler or test runner actually reported)
  -> Coverage / limitations    (what this change was not checked against)
```

Follow the stages in this order. **Do not write the edit before Stage 2 is
finished.** Enumerating obligations after writing code is not this
methodology; it is a rationalization of code you already wrote.

### Stage 1 — ProgramSpace of the pre-change source (docs/06 §1-3)

This does not mean "I read the file". It means building **accepted
structural facts** plus **explicit unknowns**, layered so a lower tier still
counts when a higher one is unavailable to you:

- Tier 0 — which files you were given, and which single file you are
  permitted to change.
- Tier 1 — the declarations actually present in the file you will change:
  the function you must modify, its exact current control flow, the helper
  functions it calls, and their signatures.
- Higher tiers (callers, cross-file effects, tests that exercise this code)
  only if you can actually establish them from what you were given.

Anything you cannot establish is an **explicit unknown**, not a fact. In
particular: you have not been given every caller of the code you are
changing. Do not silently assume you have.

### Stage 2 — Change obligations (docs/03 §5, docs/07 §3, §3.6, §4)

Before writing any code, enumerate a finite set of obligations. Each one has
the same fields a ReviewObligation has (docs/03 §5):

| Field | Meaning |
| --- | --- |
| target | what is being checked — Node, Relation, Subgraph, Path, Invariant, **Morphism** |
| property | a specific, named thing that must hold — not a vague question |
| context requirement | what part of the source this needs |
| **evidence requirement** | **the exact procedure whose result would settle it** |
| risk | impact / exposure / likelihood |
| provenance | why this obligation exists |

Two kinds of obligation must both appear, and an enumeration containing only
the first kind is incomplete:

1. **Obligations the change must newly satisfy** — the specified new
   behaviour. Provenance: the task specification.

2. **Morphism obligations — what must be preserved across the change**
   (docs/07 §3.6, verbatim list): *interface preservation*, *invariant
   preservation*, *lost test relation*, *newly introduced direct
   dependency*, *increased projection loss*. docs/07 §4 calls the rule that
   generates these the **change rule**: it fires on "lost or distorted
   structure" in the morphism. Concretely, for a code change: which existing
   behaviours must still hold, which existing tests must still pass, which
   public signatures must not move, which error paths must still be reached.

Draw properties from this vocabulary (docs/07 §12) rather than inventing ad
hoc phrasing; extend with a `domain.specific_property` name only when
nothing here fits:

```
baseline: changed-symbol, public-api, error-propagation, test-relation
async: concurrent-reentry, cancellation, lifecycle-after-dispose
security: auth-reachability, untrusted-input-flow, secret-exposure
architecture: context-boundary, forbidden-dependency
persistence: transaction, idempotency, cache-invalidation
```

**The evidence requirement is the field that matters most here.** For each
obligation, name the *specific procedure* whose outcome would settle it —
not "review the code", but an executable procedure of the kind docs/10 §2.2
admits: a named test that must pass, a compiler diagnostic that must not
appear, a command that must exit zero. If you cannot name a procedure for an
obligation, say so explicitly; that obligation will only ever reach a Claim,
never a Verification, and you must not pretend otherwise.

**Hard cap for this execution: enumerate at most 6 obligations in Stage 2,
total.** This number is an experiment-side execution constraint, not part of
the ReviewGraphen methodology. ReviewGraphen's own design does give the plan
stage a budget concept — docs/07 §13 lists "budget-aware deferral" and
"low-risk obligation sampling" against obligation explosion, docs/09 §2's
`ReviewPlan` carries an explicit `budget:` field, and docs/11 §7 frames
obligation selection as budget-constrained optimization — but **none of
those documents states a concrete number.** 6 is set here for a documented
empirical reason: in this same benchmark program, this model given an
unstructured task consumed 65,535 of its 65,536-token output budget on
reasoning and produced zero final content, and completed runs under a
131,072-token cap still spent 97-99% of output on reasoning. The comparable
structured review runs used a cap of 8 obligations and had no code to write;
this task additionally requires emitting a working edit, so the cap is set
lower. It is a judgment call under real uncertainty, not a derived number.

If you notice partway through that you are consuming a large share of your
available output while still enumerating, **stop enumerating and finalize
your output with what you have** (docs/09 §9's `budget_exhausted` abstention
reason; docs/07 §13's "budget-aware deferral"). Do not keep going hoping to
finish everything. An edit that exists with three obligations recorded is
worth more than a perfect enumeration you never got to emit.

### Stage 3 — Context projection (docs/03 §6, docs/08)

For each obligation, use only the source relevant to it, and note explicitly
what you are leaving out: files you were not given, callers you cannot see,
cross-crate effects you cannot check. A local conclusion about one context
does not automatically become a global one (docs/03 §12, docs/08).

### Stage 4 — The edit

Write the **smallest** change that discharges the Stage 2 obligations, and
no more. docs/07 §3.6 treats a *newly introduced direct dependency* as a
morphism defect in its own right: an edit that also refactors something
nearby, renames something, or adds a convenience you were not asked for is a
larger morphism than the one you enumerated, and its extra surface is
un-obligated by construction.

Constraints that follow from Stage 2, not from taste:

- Change only what the specification names as changeable.
- Do not change the acceptance test. Making a test pass by editing the test
  is not discharging the obligation; it is deleting it.
- Prefer an edit whose failure mode is a compiler error over one whose
  failure mode is wrong behaviour. The compiler is evidence you can actually
  get (docs/10 §2.2); silent behavioural drift is not.

### Stage 5 — Claim, Evidence, Verification (docs/10 §1, §3-6, §15-16)

**This is the part that turns an implementation from an assertion into
something someone can act on. Read this section twice.**

> Principle: Claim is not Evidence. (docs/10, header)

docs/10 §1 separates four things, and you must never collapse them:

```text
Claim         "this property may hold / may not hold"
Evidence      "this observation supports or refutes the claim"
Verification  "a named procedure evaluated the claim against evidence"
Decision      "an authority accepted it"
```

These are three different records with three different IDs (docs/03 §15.1,
docs/10 §16.2). For each obligation from Stage 2, emit exactly one claim
with one of these polarities (docs/03 §8, adapted to a change):

- `satisfied` — the obligation is discharged by the edit.
- `not_satisfied` — the edit does not discharge it, and you know that.
- `inconclusive` — you cannot tell.
- `not_applicable` — its precondition does not hold here.
- `conflict` — it disagrees with another obligation or fact; say so rather
  than silently picking one (docs/10 §12).

And attach to each claim an **evidence status**, which is the entire point
of this methodology:

- `verified` — **only** if you personally ran the named procedure in this
  session and observed its result. docs/10 §5 is explicit that a
  verification result of `passed` means "this procedure produced a
  supporting result", not "the claim is true"; so even `verified` must name
  the procedure and quote its actual output.
- `claimed` — you reasoned that it holds but ran nothing. This is the
  correct status for almost everything you will produce if you have no
  execution environment.
- `requested` — you know exactly which procedure would settle it and it was
  not run. Name the procedure (docs/10 §8: evidence requirements resolution).

Rules that are not negotiable:

1. **If you have no compiler and no test runner available in this session,
   then `verified` is unavailable to you for every obligation, without
   exception.** Writing code and reading it back is not compiling it. "This
   will compile" is a Claim. "`cargo build -p reviewgraphen-cli` exited 0
   and printed no warnings" is Evidence — and only if you ran it.
2. A plausible-sounding rationale is not evidence.
3. Your own agreement with yourself, or a second pass over your own
   reasoning, is not evidence (docs/10 §10, §16.6). It is at best a
   cross-check and must never be labelled `machine_checked`.
4. A preservation claim (`the other tests still pass`) is a **negative
   claim** and is the hardest kind to support (docs/10 §7). State its
   checked scope and bound, and what you did not check. Do not write "no
   regressions" as an unqualified statement.
5. Never invent a procedure name, a test name, a file path, or a line
   number. If you cannot cite the specific location, the claim is
   unsupported — say that, do not drop the citation requirement quietly.
6. If two things you noticed disagree, record the conflict (docs/10 §12).

### Stage 6 — Coverage and limitations (docs/03 §11, docs/09 §16)

State plainly: which obligations you did not get to, which of them have no
evidence, what your edit is not checked against, and where your enumeration
was incomplete. Do not imply the change is safe. docs/10 §6's verification
facets are a usable checklist for what you are and are not entitled to say —
in particular `environment valid` (did you actually build in the target
environment?) and `independent` (is the check a different mechanism from the
one that produced the code, or the same model agreeing with itself?).

## What you must never do

1. Never present a Claim as if it were Evidence or a Verification result.
2. Never write "verified", "confirmed", "tested", or "this compiles" for
   anything you did not actually run.
3. Never generalize a preservation claim beyond what you actually checked.
4. Never invent source citations, test names, or command output.
5. Never expand the edit beyond the obligations you enumerated because
   something nearby looked improvable.
6. Never make a failing test pass by weakening or editing the test.
7. Never enumerate obligations after writing the code and present them as
   if they had guided it.
