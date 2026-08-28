---
name: reviewgraphen
description: Review source code the ReviewGraphen way — an obligation-driven review that enumerates a finite set of things to check before forming any opinion, then keeps claims, evidence, and verification separate, instead of an unstructured "find bugs" pass. Covers both the methodology you execute yourself and the narrow ReviewGraphen CLI surface that can produce obligations for you. Use when repository- or application-wide review is asked for and missing areas matter, when a review must be auditable rather than a single free-form answer, when relation/path/invariant/boundary review is required, or when asked what has and has not been reviewed.
---

# ReviewGraphen review

An obligation-driven review methodology, plus the narrow CLI surface that
implements part of it. The methodology stands on its own — follow it even when
no ReviewGraphen binary is available.

## Do not use this for

- A compiler, parser, symbol resolver, or static analyzer.
- A security certification or proof that no defect exists.
- A one-file review where an explicit obligation universe adds nothing.
- Autonomous merge approval.
- Arbitrary command execution inside an untrusted repository.
- Spawning reviewer personas without distinct obligations or evidence roles.

## Never collapse these

- Program fact vs review claim.
- Review target recommendation vs review obligation.
- Visited vs completed.
- Completed vs evidence-supported.
- Evidence-supported vs verified.
- Verified vs human-accepted.
- High confidence vs high severity.
- Local validity vs global gluing.

## Why this exists

The largest cause of missed defects in AI review is that no finite set of
"things to check" ever existed — the reviewer just reads code and writes
whatever occurs to it. The answer is to make that set explicit and finite
*before* forming any opinion about whether the code is correct: enumerate what
must be checked (obligations), then check each one and keep the resulting claim
separate from whatever evidence does or doesn't support it. A finding is not the
center of this process — it is one projection that falls out at the end, after
an obligation has been executed, a claim proposed, evidence bound, and
verification attempted.

Do not begin with "review the whole repository." Begin with a bounded snapshot
and construct a review universe.

## The pipeline, and who produces each stage

```
Repository / snapshot          ─┐
  -> ProgramSpace               │ deterministic side: tooling, not the reviewer
  -> Review Obligation Universe │ (parser output, symbol table, accepted
  -> Review Plan               ─┘  dependency facts, rule matching, obligation
                                   ID generation, context source selection)
  -> Context Projection(s)     ─┐
  -> Review Execution           │ the reviewer's work: one obligation, one
  -> Review Claim               │ bounded context, one claim, evidence status
  -> Evidence + Verification    │
  -> Coverage / Gluing         ─┘
```

This split is not a convenience. Obligation IDs must be stable for the same
snapshot, profile, rules, and extractors — the same input must yield the same
obligation IDs. A set enumerated by a language model from prose cannot satisfy
that. A reviewer-enumerated set is therefore a degraded substitute even when it
is the only option, never the target state.

## Choose a mode before you start

### Mode A — obligations supplied (preferred; use whenever possible)

You are given, per unit of work, a set of obligations produced outside your
context: by the ReviewGraphen CLI below, by other deterministic tooling, or by a
separate enumeration pass. **Skip Stages 1 and 2 entirely.** Go to Stage 3.

Building the inputs for Mode A does not require any ReviewGraphen binary. The
tiers below are ordinary tooling output:

- Tier 0 — repository/change facts: `git diff --name-status <base>..<head>`,
  `git log`, file languages.
- Tier 1 — syntax and symbols: any parser or index over the changed files
  (`syn`, tree-sitter, ctags, `cargo metadata`, an LSP dump).
- Higher tiers — calls, ownership, tests: only what the tooling actually
  resolved. Everything else is an explicit unknown carried into the packet.

Obligations are then produced from those facts by a fixed rule list rather than
by generation — e.g. "each changed public function → one node obligation for its
property class", "each changed caller/callee pair → one relation obligation".

If a stronger model builds the packet and a weaker one executes it, put the
enumeration on the stronger model and the execution on the weaker one.

### Mode B — no supplied obligations (fallback)

You were handed raw source and nothing else, and no enumeration step exists
upstream. Then you perform Stages 1 and 2 yourself, knowing that this is a
substitution for a deterministic engine, that the resulting set is not
ID-stable, and that it is the single largest consumer of your budget. Say so in
your output.

## The ReviewGraphen CLI

Build it first; `$RG` below is the resulting binary.

```bash
cargo build --release --locked -p reviewgraphen-cli   # from the repository root
RG="$PWD/target/release/reviewgraphen"
```

The implemented command surface is exactly:

```text
reviewgraphen review --request <request.json> --artifacts <fresh-dir> [--diagnostics <fresh-file>]
reviewgraphen schema list
reviewgraphen schema print <schema-id>
reviewgraphen schema validate <json-file>
```

There are no `snapshot`, `ingest`, `obligations`, `plan`, `run`, `verify`,
`glue`, `coverage`, `report`, `gate`, or `context` subcommands. **Do not invent
or claim execution of them**, and never translate a stage of the pipeline above
into a command that is not on this list. `--help` prints the single usage line
and exits 2; there are no per-subcommand help pages.

`review` accepts the closed generic-review request v2, v3, or v4 contracts. The
implemented slice is narrow: `rust.production.v1`, the
`relation.changed_public_callee@1` rule over an accepted direct `calls`
relation, and `rust.callee_contract_review@1`. V3 selects
`context.subject_windows@3`, recording caller/callee subjects, relation IDs,
bounded source windows, denominator commitments, unknowns, and declared loss.
This is not a repository-wide call graph, proof that a contract changed, or
proof that a bug exists.

V4 is a separate mixed production family: D remains bound to
`context.subject_windows@3`, while `node.public_function_contract@1` is bound
only to `context.subject_windows@4`. Its Node arm covers accepted exact public
free functions with a module `contains` witness, never `direct_calls`, and its
single-layer coverage must not be described as a call-enumeration gap.

Run from the admitted repository root. The request binds immutable base and
target revisions, profile/rule identity, ingest bounds, plan bounds, observer,
verifier descriptor, and context policy. Both the artifact directory and the
optional diagnostics file must be fresh, workspace-scoped, relative paths; an
absolute artifact directory is rejected as path traversal.

```bash
cd /path/to/admitted/repository
"$RG" review \
  --request /absolute/path/to/reviewgraphen.generic_review_request.v3.json \
  --artifacts artifacts \
  --diagnostics diagnostics.json
```

A successful v3 run writes:

```text
artifacts/artifact-manifest.v1.json
artifacts/audit.run.v3.json
artifacts/human-report.manifest.v2.json
artifacts/human-report.md
artifacts/records/<execution>.deterministic-observer-output.v1.json
artifacts/records/<execution>.provider-free-reviewer-packet.v1.json
diagnostics.json
```

The reviewer packet is the Mode A input. The audit
(`reviewgraphen.generic_review_run.v3`) is non-authority: its claims,
observations, coverage, and limitations never become accepted facts, Evidence,
Verification, or human acceptance. Validate it with
`"$RG" schema validate artifacts/audit.run.v3.json`; use `schema list` before
`schema print` or `schema validate`.

A `context` subcommand exists only on the unmerged m21 branch (commit
`2c972dbb`), producing `reviewgraphen.context_packet.v1` from
`reviewgraphen.context_request.v1` via `context.task_subject_windows@1`. To use
it, check that commit out into a separate worktree and build its own binary —
never substitute it for the main one, never present it as a main capability, and
never cross-decode its request or packet as generic-review v3.

## Stage 1 — ProgramSpace (Mode B only)

ProgramSpace ingestion does not mean "I understood the repository." It means
building **accepted structural facts** plus **explicit unknowns** — a
before-you-form-any-opinion inventory, layered so a lower tier still counts even
if a higher one is unavailable to you. Use the same Tier 0 / Tier 1 / higher-tier
split listed under Mode A.

Prefer compiler, LSP, parser, or analyzer facts. An LLM-inferred relation may be
proposed as a candidate but is never an accepted fact. Always inspect the
extraction report: a missing call-graph capability is not evidence that no risky
path exists.

Anything you cannot establish is an explicit unknown, not a fact. Do not
silently treat "I didn't check this" as "this is fine."

**Keep this inventory internal.** Emit only what a later stage cites. A narrated
walkthrough of every file you looked at is generation cost with no consumer.

## Stage 2 — Review Obligation Universe (Mode B only)

Before reading for defects, enumerate a finite set of **ReviewObligations**.
Each obligation is:

| Field | Meaning |
| --- | --- |
| target | what is being checked — see target kinds below |
| property | a specific, named thing to verify — not a vague question |
| context requirement | what neighborhood/information this needs |
| evidence requirement | what would actually support a claim about it |
| risk | impact / exposure / likelihood, as a first estimate |
| provenance | why you generated this obligation |

**Target kinds:**

- **Node** — one function/symbol (e.g. input validation on a public function,
  resource release in a lifecycle method).
- **Relation** — one edge between two things (caller/callee error contract, a
  UI-to-persistence dependency, a model/wire-schema nullability match).
  Relations matter because code can be locally correct at every node while
  still being wrong at the edges between nodes.
- **Subgraph** — a semantically meaningful group, not just a k-hop neighborhood
  (a whole feature, a pipeline, an authentication boundary).
- **Path** — an ordered sequence (untrusted input -> parser -> command
  execution; begin transaction -> writes -> commit/rollback). Order is part of
  the target's identity: a path and its reverse are different obligations even
  over the same edges.
- **Invariant** — a property that must hold globally, not at one location
  (payment happens at most once; disposed objects are never updated afterward).
- **Morphism** — what must be preserved across a change (interface
  preservation, an invariant that used to hold and might not anymore, a test
  relation that got lost).

**Property vocabulary** — draw properties from this listing rather than
inventing ad hoc phrasing; extend it with a versioned-style name
(`domain.specific_property`) only when nothing here fits:

```
baseline: changed-symbol, public-api, error-propagation, test-relation
async: concurrent-reentry, cancellation, lifecycle-after-dispose
security: auth-reachability, untrusted-input-flow, secret-exposure
architecture: context-boundary, forbidden-dependency
persistence: transaction, idempotency, cache-invalidation
```

Do not enumerate an unbounded number of obligations. Prioritize by what actually
changed (if you know), public/external-facing surface, and risk. It is
legitimate, and expected, for most of what you read to not become an obligation
at all.

**Default cap in Mode B: 8 obligations, total, across the whole input.** This is
an execution constraint, not part of the methodology. Treat 8 as an upper bound
rather than a target: a small local model can exhaust its whole output budget at
this cap, so prefer Mode A, or set a lower cap. In Mode A the cap does not
apply — the supplied set governs, and you may process it one obligation per
request.

If you notice partway through that you are consuming a large share of your
available output while still enumerating or processing obligations, stop
enumerating new ones and finalize your output with what you have completed.
Record the reason as `budget_exhausted`. Do not keep going in the hope of
finishing everything.

## Stage 3 — Context Projection

For each obligation, use only the source relevant to it — not the whole input
indiscriminately. Note explicitly what you are leaving out (files you were not
given, call targets you could not resolve, cross-file effects you cannot see).
A local conclusion about one context does not automatically become a global
conclusion — if two parts of your review would need to agree with each other to
support a broader claim and you cannot actually check that they do, say so
rather than assuming it.

## Stage 4 — Review Execution and Claim

For each obligation, produce exactly one **ReviewClaim** with one of these
polarities:

- `issue_present` — you believe a defect or rule violation exists.
- `issue_absent` — you checked and did not find a problem, **within the checked
  scope only**. This is not "safe" and not a general guarantee — state what you
  actually checked and its bound.
- `inconclusive` — you don't have enough to decide either way.
- `not_applicable` — the obligation's own precondition doesn't hold here.
- `conflict` — this claim doesn't sit well with another claim or fact you have;
  say so rather than silently picking one.

Every claim must cite **source IDs** — the specific file path, symbol, or line
range it is grounded in. A claim with no source citation is an unsupported
proposal, not a finding — it must not be reported as if it were checked.

**Abstain rather than force a claim** when: the context is insufficient, a
symbol is unresolved, required evidence isn't available to you, you don't
actually understand the property being asked, sources conflict, or you've hit
your own budget. An abstention is not an error and is not the same as "no
issue" — record it as what it is.

## Stage 5 — Evidence and Verification

**This is the part that turns a review from an opinion into something someone
can act on. Read this section twice.**

> Claim is not Evidence.

A **Claim** is "this problem might exist" or "I checked and didn't find one."
**Evidence** is an actual observation — a test result, a compiler diagnostic, a
resolved call edge, a counterexample — that supports or refutes a claim.
**Verification** is the record of a specific procedure having been applied to
check a claim against evidence. These are three different things with three
different IDs, and you must never collapse them:

- Writing a plausible-sounding rationale is **not** evidence.
- Another model (including you, asked twice) agreeing with a claim is **not**
  evidence — it is, at best, a cross-check, and must never be labeled
  `machine_checked` or treated as proof. "Two reviewers agree" is not the same
  fact as "a test failed." Independent agreement can raise priority; it never
  substitutes for verifying the actual program property.
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
  as an unqualified, general statement.
- If two things you noticed disagree with each other, do not silently resolve it
  by picking the more plausible one — record the conflict.

## Stage 6 — Coverage and honesty about scope

You are not reviewing "the codebase." You are executing a bounded set of
obligations against a bounded context. State plainly what you did not get to,
what you could not resolve, and where your obligation set was incomplete. In
Mode B, state that the obligation set was reviewer-enumerated and therefore not
ID-stable — coverage over such a set is descriptive, not a versioned-universe
coverage figure.

Persona-switching ("as a senior engineer... as a security expert...") does not
increase coverage and is not part of this methodology — a genuinely different
angle requires a different evidence source or an independent failure mode, not a
different voice over the same information.

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
8. Never claim a CLI subcommand that is not on the list above.

## Output

Emit the record; do not narrate the derivation. Only the structures below are
output. The reasoning that produced them is not.

If the caller supplied an output schema, follow it unmodified. Otherwise, in
Mode A answer one obligation per response:

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

In Mode B, emit one `obligation_results` entry per obligation you enumerated,
with your own stable `packet_id` (`obligation-1`, `obligation-2`, ...) and the
Stage 4 claim polarity as `disposition` (`abstained` for one you abstained on).
Emit a `findings` entry only for `issue_present` obligations, carrying your
source citations as `locations`, a `rationale` stating the claim **and** its
evidence status (e.g. "unverified — would need a reproduction test showing X"),
and a `severity` reflecting your risk estimate. `issue_absent`, `inconclusive`,
and `not_applicable` are recorded only via `obligation_results`.

A summarizing response should state: snapshot/profile/universe; extraction
limitations; which target and property layers were reviewed; critical and high
claims with their disposition; evidence and verification outcome; abstentions
and unverifiable claims; gluing obstructions; coverage; projection loss; and the
next required decision or observation.

## Safety rules

- Never allow arbitrary shell by default.
- Never write to the reviewed repository during review.
- Never upload full source unless policy explicitly allows it.
- Redact secrets before model invocation and declare the resulting loss.
- Restrict filesystem access to the workspace root.
- Treat generated code, vendored code, and documentation through explicit
  profile rules rather than silent exclusion.
- Preserve rejected claims and decisions for audit.
- Do not expose raw proprietary source in a research export.

## Local-inference execution notes

Execution conditions for small local models. Ignore them when the reviewer is a
large hosted model.

- **One obligation per request.** Mode A's per-obligation form exists so that
  eight obligations are eight small requests, not one large one. This closes the
  window in which runaway reasoning can consume the whole budget.
- **Disable the thinking channel.** Set `enable_thinking=false` with
  `response_format=json_object`. With thinking on, a model of this size will
  spend its ceiling on prose and return no final content.
- **Keep the output contract thin.** A closed output form can reject an entire
  run on conformance alone, independently of review quality. Every required
  field costs conformance probability; require only fields a consumer reads.
- **Do not respond to budget pressure by dropping the obligation structure.**
  Unstructured review is the configuration most likely to consume the whole
  budget and return nothing. Respond by moving enumeration upstream (Mode A),
  lowering the cap, or splitting into more requests.
