# Competing minimal vertical slices

## 1. Rust pattern feasibility screen

The rule below is strict: a deterministic extractor may record only what the
chosen frontend actually establishes. A review obligation may ask a semantic
question, but its **applicability and target** must be grounded in accepted
facts, and an LLM answer remains a proposal.

| Pattern | `syn` alone | rustc/rustdoc/MIR | Decision for first slice |
| --- | --- | --- | --- |
| `unsafe` function/block/impl boundary | **Yes, syntax-exact.** `syn` exposes `Signature::unsafety`, `ExprUnsafe`, and `ItemImpl::unsafety`, with spans. It cannot prove whether unsafe operations are sound, and must not claim that. | Not required to locate the syntactic boundary. rustc/Miri may later supply property-specific evidence. | **Retain.** Reify an `unsafe_region` artifact and `contains_unsafe` relation; ask whether stated safety preconditions are sufficient/preserved. |
| `unsafe impl Send/Sync` trait contract | `unsafe impl` and the written trait/type paths are exact; identity of aliases/re-exports and auto-trait semantics are not. | rustc type resolution is required to assert the trait is actually `core::marker::Send/Sync` and to inspect fields/negative impls. | Retain only as a later specialization. The first rule may target generic `unsafe impl` syntax without claiming resolved Send/Sync identity. |
| `.unwrap()`/`.expect()` on `Result` across an error boundary | The method token and enclosing syntactic return type can be seen, but the receiver might be `Option` or a user type; aliases and inferred return types defeat a `Result` assertion. | rustc typeck or a versioned rust-analyzer query can resolve receiver/return types; stable `syn` cannot. | **Reject from `syn` slice.** A weaker “unwrap-shaped method call” rule is too noisy and its label would be misleading. |
| Public API type change / semver break | `syn` can compare written signatures but cannot correctly model re-exports, cfg, macros, type aliases, visibility reachability, or trait coherence. | rustdoc JSON plus a pinned comparator such as `cargo-semver-checks` can deterministically derive a versioned public API diff, subject to toolchain/cfg identity. | **Retain as a competing higher-effort slice.** |
| Trait implementation contract generally | `impl Trait for Type` syntax is visible, but no generic correctness contract follows from its existence. | rustc resolves identities; an accepted profile must still declare the trait-specific property. | Reject as a generic rule; retain only for an explicitly versioned trait profile. |
| Lock acquisition order | Names and method tokens are visible, not receiver types, alias identity, control-flow order across branches/calls, or whether guards remain live. | MIR/typeck plus an allow-listed lock API ontology and conservative interprocedural analysis are needed; wrappers remain unresolved. | Reject from minimal slice. |
| `?` error conversion information loss | `ExprTry` is visible; source/target error types and selected `From` conversion are not. “Information loss” is not a syntax fact. | rustc typeck can identify conversions; whether loss violates a contract still requires a declared property. | Reject from minimal slice. |
| `Drop` order / resource lifecycle | Written field order and explicit `Drop` impl syntax are visible, not fully elaborated drop order under moves/unwind/desugaring. | MIR drop elaboration and a resource-specific invariant are needed. | Reject as a general first rule. |
| `&mut` aliasing | Reference syntax is visible. Safe alias validity is borrow-checker semantics; unsafe raw-pointer aliasing cannot be inferred from syntax. | rustc borrow check/Miri provide bounded evidence, never a complete general alias proof. | Reject as a syntax rule. |

The current extractor already follows this discipline: direct calls are accepted
only for one conservative local syntactic target
(`crates/reviewgraphen-ingest/src/rust.rs:1609-1768`), and method calls are
explicitly unresolved (`crates/reviewgraphen-ingest/src/rust.rs:1941-1951`).

## 2. Candidate A — Unsafe-boundary Relation-to-Report (recommended)

### Scope

Add one generally reachable Rust relation:

```text
callable/module --contains_unsafe--> unsafe_region
```

The accepted fact means only “this unexpanded Rust syntax contains an explicit
unsafe boundary at this span.” The rule
`relation.changed_unsafe_boundary@1` creates a Relation obligation with property
`rust.unsafe_boundary_contract@1` when the relation or enclosing callable is in
the base→head changed region. It asks the reviewer to assess whether the unsafe
boundary's documented preconditions, caller obligations, and maintained
invariants are sufficient. It does **not** assert unsoundness.

Carry that obligation through:

1. subject-prioritized multi-window context;
2. deterministic “always abstain” generic reviewer mode and existing real
   Codex/Claude modes;
3. strict proposal/abstention parsing;
4. a closed verifier registry with `unsupported` as the default, plus one
   code-owned offline Cargo-test descriptor whose argv/environment/cwd/mounts
   cannot come from model prose;
5. canonical audit JSON and a derived short Markdown report explicitly labeled
   non-authority/proposed/unverified.

The first slice does not add automatic acceptance. A later human decision must
bind the claim/evidence/verification IDs through the existing authority seam.

### Contracts and ADR

- New capability `unsafe_boundaries`, complete iff all admitted Rust syntax was
  parsed under extractor `rust_syn.v2`; versioned artifact/relation shape and
  stable-ID inputs.
- New rule/profile revision and obligation property; rule must emit explicit
  capability gaps on parse/macro limits rather than widen `ast` claims.
- Context policy v2: subject anchors first, multiple non-overlapping windows per
  file, deterministic merge/order/caps, window-level source IDs/loss.
- Generic request/run v2 (or additive new schemas): deterministic observer,
  typed verification attempt/result, Markdown projection hash, fixed
  non-authority ceiling.
- Verifier descriptor v2: exact executable identity, fixed argv template,
  read-only repo, workspace-scoped cwd, separate writable `CARGO_TARGET_DIR`,
  no network, closed env, CPU/wall/RSS/process/output limits. Model may request
  a descriptor ID but never command text.
- A new ADR is mandatory before code because this changes public types/schema,
  projection identity, tool authority, and the rule-set denominator. It must
  state migration/major-version behavior and explicitly supersede no M4
  authority rule.

### Estimate

**8–10 terra/high work units**, including ADR/schema, ingest fact, rule,
projection, deterministic observer, verifier sandbox, generic report, CLI/docs,
and negative/determinism fixtures. This is a small product slice, not a
single-file heuristic.

### Measurable utility

- Non-fixture Relation-obligation prevalence per changed Rust commit.
- Known-root recall delta against free-form review under equal budget.
- Proposed false-positive count on matched fixed/control changes.
- Completion and judge-positive findings per token/hour.
- Exact reproducibility of denominator, unsafe relation IDs, context
  include/exclude/unknown/loss, and report/audit hashes.

### Failure risks

- Unsafe-boundary changes may be too sparse to power a population study; the
  preregistration therefore stops if the hidden census cannot reach the frozen
  sample size.
- Reviewers may emit generic “unsafe is risky” claims. The output schema and
  judge rubric must require a concrete violated precondition/invariant, exact
  source IDs, and an observable failure consequence.
- Cargo tests often do not verify memory-safety properties. Passing them is
  evidence, not verification of the unsafe claim; unsupported/inconclusive must
  remain common and visible.

### Boundary risks and controls

- **Fact/claim laundering:** name the fact `explicit_unsafe_syntax`, not
  `unsafe_bug`; claims remain proposed.
- **Macro/type overclaim:** macro-generated unsafe and alias/trait identity stay
  unknown; no “complete semantic unsafe” capability.
- **Shell authority:** only a code-owned descriptor can run; raw claim prose and
  `requested_evidence` cannot become argv/path/env.
- **Report authority:** Markdown is a projection of canonical audit JSON and
  cannot mint Evidence, Verification, Decision, Finding, or `trusted_pass`.

## 3. Candidate B — Generic workflow/verification first on the existing async Node rule

### Scope

Do not add a semantic rule. Add subject-preserving context, deterministic
abstention mode, fixed offline Cargo verifier, audit JSON + non-authority
Markdown, and a checked-in ordinary async Git quickstart. Use the already
reachable `node.changed_public_symbol@2` obligation.

### Contracts and ADR

Generic request/run v2, context policy v2, verifier descriptor v2, and
non-authority report schema. A new ADR is required for the verifier authority
and schema changes. No ProgramSpace/rule contract changes.

### Estimate

**5–7 terra/high work units**.

### Measurable utility

Clone-to-report completion, deterministic fake/replay identity, sandbox
negative tests, target inclusion, and human report usability can all be measured
without a model. A small model trial can measure whether the repaired projection
helps the existing async question.

### Failure risks

It can produce a polished workflow whose built-in semantic denominator still
has only one broad Node rule. It therefore **does not pass the user's
Relation/Path/Invariant gate** and cannot establish general Rust review utility.

### Boundary risks

Lower semantic risk than A, but higher product-framing risk: documentation may
again make one demonstration look like generic detection. The example must be
an ordinary temporary Git repository, not a named hidden fixture, and must show
non-authority/incomplete status.

### When to choose instead

Choose B only if the immediate priority is to retire verifier/CLI security risk
before selecting a semantic wedge, and explicitly call the release an
infrastructure preview rather than the requested practical state.

## 4. Candidate C — Public-API compatibility Invariant slice

### Scope

Pin rustdoc JSON/toolchain/cfg identity and a deterministic public-API comparator
for base and head. Materialize accepted API-surface facts plus an invariant
scope representing “the declared compatibility policy must hold.” Generate
`invariant.public_api_compatibility@1` obligations for changed public API items.
Comparator output is an external deterministic evidence artifact; it does not
automatically accept the reviewer claim. Carry the result through the same
projection/fake/real/report workflow as A.

### Contracts and ADR

- Versioned rustdoc extraction adapter, toolchain target/features/cfg identity,
  public reachability and comparison schemas.
- Invariant source and accepted-policy authority: semver level and exclusions
  must come from a versioned profile, never from the model.
- Tool descriptor for pinned rustdoc/comparator, with base/head source hashes
  and stale invalidation.
- Generic request/run/report additions as in A.
- New ADR for external deterministic fact/evidence distinction, toolchain
  reproducibility, unsupported targets, and schema migration.

### Estimate

**11–14 terra/high work units**.

### Measurable utility

Mechanical breaking-change recall, unsupported-cfg rate, false positives
against comparator-confirmed controls, and reviewer value on explaining impact
are stronger and more objective than generic issue judgment. This slice is
likely more frequent in library repositories than changed unsafe boundaries.

### Failure risks

- rustdoc JSON/toolchain stability, feature matrices, target cfg, macros,
  re-exports, and workspace dependency resolution materially expand the
  denominator.
- A comparator result can be a strong world anchor, but treating it as accepted
  policy truth without a profile would collapse fact/evidence/decision.
- Clone reproducibility may require pinned tool downloads and caches, contrary
  to a provider-free/offline quickstart unless artifacts are vendored or
  preflighted.

### Boundary risks

The main risk is calling “different public API” a “breaking bug” without an
accepted compatibility policy. Keep API facts, comparator evidence, review
claim, and human compatibility decision separate.

### When to choose instead

Choose C if the product is primarily for library maintainers, a pinned rustdoc
toolchain is acceptable, and the user prefers a mechanically anchored invariant
over a smaller syntax-only wedge.

## 5. Recommendation

Choose **Candidate A: Unsafe-boundary Relation-to-Report**.

It is the smallest candidate that simultaneously:

- clears the missing non-fixture Relation gate;
- uses a fact `syn` can establish without name/type resolution;
- exercises projection around a concrete changed subject;
- forces the generic verifier and report boundaries to become honest and
  usable; and
- can be evaluated against independent test/Miri/issue anchors without ever
  treating the model or judge as truth.

It is not “complete” when one happy-path unsafe fixture passes. Completion
requires at least: ordinary repositories with and without unsafe boundaries;
parse/macro/oversize/target-truncation cases; verifier unsupported/timeout/
nonzero/stale cases; identical fake/replay bytes; real-adapter structured and
abstention paths; and the frozen holdout procedure in
`04_evaluation_preregistration_draft.md`.

Candidate C is the preferred alternative if unsafe-boundary prevalence fails
the preregistered hidden census or library API compatibility is the chosen
product market. Candidate B is a sequencing fallback, not a substitute for the
semantic gate.
