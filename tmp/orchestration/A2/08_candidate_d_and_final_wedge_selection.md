# Candidate D audit and final wedge selection

Audit basis: repository revision `1a2fefb10c1bffb088f8377e102d391086e094a9`,
2026-08-23. No implementation or network access was performed.

## 1. Candidate D, narrowed to an implementable first rule

The broad challenge candidate includes both “callers of a changed callee” and
“callees called by a changed caller.” The first wedge should be narrower:

> For each accepted local `calls` relation whose **callee is an exact `pub`
> function changed by the base→head comparison**, create a Relation obligation
> asking whether callers' assumptions about preconditions, postconditions,
> error paths, and return-value meaning remain valid.

The target is the accepted relation ID, not a guessed call. The change and
public filters reduce fan-out; the changed-caller-only branch is deferred
because changing a caller body does not itself establish a change to the
callee's contract and tends to select every edited callsite.

This property is a review question, not an accepted fact that a contract broke.
The name must say `changed_public_callee_contract_review` (or equivalent), not
`breaking_call` or `contract_violation`.

## 2. What the current facts can and cannot establish

### 2.1 Accepted calls

The Rust adapter accepts a free-function `ExprCall` only when the callee is a
path and resolves to exactly one conservative local syntactic target. It refuses
shadowed unqualified names, glob/unresolved scopes, ambiguous or absent local
targets, and semantic/UFCS cases (`crates/reviewgraphen-ingest/src/rust.rs:1609-1719`).
An accepted relation is `kind="calls"`, with source caller, target callee,
`resolution="syntactic_unique"`, call line, source path, and extractor identity
(`rust.rs:1720-1744`). Method calls remain
`DynamicDispatchUnresolved` (`rust.rs:1941-1951`). This is a useful, accepted
subset, never a complete Rust call graph.

### 2.2 Changed endpoint

Git ingest creates a base-relative change artifact with `changed=true` and exact
changed lines (`crates/reviewgraphen-ingest/src/lib.rs:543-604`). It adds
`changed_by` and reverse `contains` edges for every accepted artifact whose
location intersects those lines (`ingest/src/lib.rs:605-658`), using the exact
range test at `ingest/src/lib.rs:921-930`. Thus an accepted function endpoint can
be deterministically classified as changed without mutating the function's
base-invariant canonical body.

The loop ranges over artifact drafts, not relation drafts
(`ingest/src/lib.rs:618-657`). A `calls` relation records a line but has no
location record (`rust.rs:1724-1744`). Therefore current facts can say “callee
function changed” or “caller function changed”; they do **not** currently say
that the callsite relation itself changed. A rule could compare its recorded
line to the change artifact's `changed_lines`, but that is new synthesis logic,
not an already materialized `changed_by` fact.

### 2.3 Public and signature/error filters

Top-level function artifacts store exact unqualified-public syntax as
`public=true` only for `Visibility::Public(_)`
(`rust.rs:636-662,901-910`). This is usable now, with the honest caveat that it
does not prove external reachability through module visibility/re-exports and
does not include `pub(crate)`.

Function artifacts retain span, label, general attributes, and the **file**
content hash (`rust.rs:644-661`). They do not retain a normalized signature,
return type, error type, ABI, generics, where-clause, or base/head API-diff fact.
Consequently:

| Filter | Honest with current accepted facts? | Conclusion |
| --- | --- | --- |
| callee function changed | Yes | use |
| callee exact `pub` syntax | Yes, syntactic only | use for first wedge |
| relation is accepted syntactic-unique local call | Yes | mandatory |
| callsite line changed | Derivable only with new synthesis comparison | defer unless ADR adds and tests it |
| written signature changed | No separate fact | do not claim/filter without new extraction |
| error or return type changed | No | do not claim/filter |
| method/trait/cross-crate caller | No | keep unresolved/unknown |
| actual semantic contract changed | No deterministic fact can prove it | reviewer question only |

This makes “changed exact-public callee + accepted direct call” the strongest
low-cost noise filter. A later signature-diff specialization would require an
extractor/versioned base-head representation and should not be smuggled into D.

## 3. Capability and applicability: exact current behavior

### 3.1 `direct_calls` is permanently partial

The adapter unconditionally declares `direct_calls=Partial`, alongside four
other resolver-bounded capabilities (`rust.rs:159-176`), and always emits a
source-backed limitation explaining unique-local-syntax bounds, absent
cross-crate resolution, and unresolved dynamic dispatch (`rust.rs:165-193`).

`materialize` treats every required capability not fully available as missing;
with none missing it emits `applicable`, otherwise `unknown` and attaches the
reason/qualification IDs (`crates/reviewgraphen-core/src/synthesize.rs:1083-1109`).
“Fully available” is exactly `CapabilityState::Complete`; the comment explicitly
says resolved facts under a partial capability still create their targeted
**unknown** obligation rather than being dropped (`synthesize.rs:1111-1125`).
The reason becomes `capability_partial:direct_calls`, with linked limitations
(`synthesize.rs:1128-1166`).

The current relation rule contract requires `direct_calls`
(`synthesize.rs:1457-1469`), and the materialized spec repeats that requirement
(`synthesize.rs:654-688`). Therefore, under the current public semantics:

- a D-shaped obligation over an existing accepted call can be emitted;
- it is **not applicable**; it is `unknown`;
- and it is not replaced solely by a gap: a separate rule-level capability-gap
  obligation is also emitted for every incompletely available rule, even when
  concrete facts exist (`synthesize.rs:827-879`).

Answer to the challenge's (a)/(b)/(c): **(b), plus (c)**—one targeted unknown
obligation per resolved target that the rule emits, and one rule-level gap for
the incomplete enumeration. It cannot be (a) without a versioned contract
change.

The existing `relation.changed_call_contract@1` is payment-idempotency fixture
semantics (`synthesize.rs:654-688,1457-1469`). Candidate D must not silently
reuse that stable rule/property ID with a new meaning; it needs a new rule and
property version (or an explicitly migrated major rule pack).

### 3.2 Where unreachable calls appear in the denominator

The universe contains the exact obligation IDs, exclusions, extraction
capabilities and limitations, and limitation IDs
(`synthesize.rs:932-1019`). Each unresolved direct or method call is retained as
an ingestion obstruction, but the D rule cannot mint a relation obligation for
an absent relation ID. The current rule-level gap is rooted at the snapshot and
collects the capability limitation; it is **not one denominator item per
unresolved callsite** (`synthesize.rs:856-877`).

Accordingly, the honest denominator has two layers:

1. **resolved-target denominator:** exact accepted `calls` relations that meet
   the D filters; and
2. **candidate-space incompleteness:** `direct_calls=partial`, source-backed
   obstructions/limitation IDs, and the rule-level gap.

It cannot support “reviewed 100% of callers,” “call-graph recall,” or a count of
all missing relation obligations. The unavailable method/cross-crate relations
are not silently absent—their incompleteness is represented—but they are not
enumerated individual D candidates.

## 4. Can a resolved-edge obligation honestly become applicable?

Yes, but only after an explicit contract split. AGENTS.md forbids implicit
coverage denominators and fact/claim laundering; it does not require a known-
valid accepted fact to become unreviewable merely because enumeration of all
facts of that kind is incomplete.

The new rule contract should distinguish:

- **target support requirements:** `ast`, `containment`, and
  `changed_structure` must be complete for the exact relation endpoints/change
  mapping. `ast` and containment are complete when admitted Rust parses
  (`rust.rs:121-137`); `changed_structure` is complete unless a changed entry is
  excluded (`crates/reviewgraphen-ingest/src/git.rs:623-645`). The target must
  itself be an accepted `calls` relation with `syntactic_unique` resolution.
- **enumeration completeness requirements:** `direct_calls`, expected to remain
  partial. It qualifies universe coverage, produces the rule-level gap, and is
  always projected into report limitations, but does not invalidate the
  already accepted relation target.

Then `applicable` means only: “this property is applicable to **this accepted
relation target**.” It must never mean: “all calls were found.” This needs a new
rule/profile/contract version, explicit DTO fields (for example
`target_support_capabilities` and `enumeration_capabilities`), schema migration,
and mutation tests proving neither field can be omitted or interchanged.

Simply removing `direct_calls` from `required_capabilities`, hard-coding
`applicable`, or declaring `direct_calls=complete` would hide the denominator
gap and violates the project boundary. Leaving the current contract unchanged
is honest but yields only unknown D targets and fails the practical
applicability gate. These are the only defensible choices.

## 5. Noise and universe growth

The broad “changed endpoint on either side” rule can grow as the sum of callers
of every changed callee plus callees of every changed caller. A large edited
dispatcher or utility can therefore mint many obligations, and body-only edits
may have no contract relevance. The first wedge controls this deterministically:

1. production profile only; test/example/generated/vendor exclusions must be
   profile-declared with excluded weight, never hard-coded disappearance;
2. accepted `calls` plus `resolution=syntactic_unique` only;
3. callee function intersects changed lines;
4. callee has exact `public=true` syntax; and
5. one relation obligation per accepted caller→callee edge.

Do not cap the universe by dropping excess callers. If a profile sets a maximum
review packet/model count, every overflow candidate retains its stable ID and
weight as a deterministic `deferred`/exclusion record with reason. Planning can
sample at most one stable-ranked obligation per commit for evaluation while the
universe remains complete relative to the accepted-edge candidate definition.

Risk remains: a public function body edit selects all resolved local callers,
even when its written signature is unchanged. That is intentional uncertainty
in the review question, not a deterministic assertion. Stage 0's prevalence,
per-commit obligation-count distribution, packet bytes, and overflow fraction
must be published. A sensible product gate is p95 ≤50 applicable D obligations
per commit and ≤5% deferred by packet/planning bounds; failure requires a
narrower versioned filter, not a silent cap.

## 6. Read-only local prevalence probe

### 6.1 Method

No external repository was fetched. I examined the latest 100 first-parent
commits at these already-present revisions:

- `/home/rizumita/github/fsl` at
  `e589014d1655b1224f5b83a7f2de99532a0dcdba`;
- `/home/rizumita/github/casegraphen` at
  `20af165fe0d95866c5eb0739b3a20f0193486232`; and
- ReviewGraphen at the audited revision.

For every parent→commit diff I excluded paths containing `test`, `tests`,
`benches`, or `examples`. The proxies count commit clusters, not lines:

- A exact-line proxy: changed line contains `unsafe fn/impl/trait` or
  `unsafe {`;
- A permissive upper bound: any changed production Rust file contains that
  syntax in either parent or target;
- D broad upper proxy: a changed line contains path-call-like syntax;
- D nearer proxy: a changed `fn` name appears on at least two production source
  lines in the target tree (declaration plus at least one other occurrence);
- C lower syntactic proxy: a changed line contains an explicit `pub` item
  declaration.

This is regex churn analysis, not `syn`, name resolution, D applicability, or a
defect census. Deleted and added changed lines both count. The D name proxy can
still count recursion, comments/macros, and ambiguous names; the call-line proxy
can count control/macro/external syntax. The A file-level measure is deliberately
an upper bound, so its zero is informative for these histories only.

### 6.2 Results

| Repository | Production-Rust commits | A changed unsafe | A file upper | D call-line upper | changed `fn` decl | changed name on ≥2 production lines | C changed public decl |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| FSL | 55 | 0 | 0 | 53 | 43 | 42 | 32 |
| CaseGraphen | 59 | 0 | 0 | 56 | 39 | 36 | 30 |
| ReviewGraphen | 29 | 0 | 0 | 28 | 21 | 20 | 18 |
| **Combined** | **143** | **0 (0%)** | **0 (0%)** | **137 (95.8%)** | **103 (72.0%)** | **98 (68.5%)** | **80 (55.9%)** |

D's exact accepted/public-callee prevalence will be lower than 68.5%; it remains
unmeasured until the rule exists. Even so, A is absent under a permissive upper
bound while D's prerequisite churn is routine. C also has a plausible
prevalence advantage over A. This is development evidence and cannot be reused
as blinded confirmatory holdout evidence.

Crossbeam and Bytes prevalence from A1 remains **unmeasured**: those repos/ranges
were not locally available and network acquisition was prohibited.

## 7. Implementation cost delta

Candidate D needs no new Rust ingest capability and no new accepted relation
shape. A1's U1 (“extract unsafe syntax facts”) disappears. The synthesis unit is
larger, however, because it must add the target-support/enumeration-completeness
split and schema/version tests; it is not safe to save effort by bypassing the
capability gate.

The rest of the vertical path remains: ADR/versions, rule, subject-first
projection, closed verifier outcome, deterministic observer, generic runtime
schema, short report, CLI/quickstart, and docs/reference tests. Estimated total:
**7–9 terra/high units**, roughly one unit cheaper than A's 8–10, not a
three-unit shortcut. U1 is eliminated; U2 expands. If signature/error-diff
filtering is added, D regains an ingest/base-head comparator unit and loses most
of that advantage.

## 8. A vs C vs D final comparison

| Axis | A: unsafe boundary | C: public API invariant | D: changed public callee relation |
| --- | --- | --- | --- |
| Practicality gate | **Fails current local prevalence evidence:** 0/143 even under permissive upper bound | Plausible: public-decl proxy 80/143; comparator itself has deterministic value | **Best:** prerequisite proxies 98–137/143; exact Stage 0 still mandatory |
| Evaluation feasibility | Cannot reliably source even 10 positive clusters locally; A1 83-unit census unsupported | Model-free comparator study feasible after larger implementation; model value is secondary | 10/40/120 paired clusters fit 6/24/72 h; high chance Stage 0 yields units |
| Implementation cost | 8–10 units; new exact unsafe ingest capability | 11–14; rustdoc/toolchain/cfg/feature/re-export comparator | **7–9**; existing facts, but public applicability/denominator split required |
| Boundary risk | Low fact risk, high overclaim risk (“unsafe” ≠ bug) | Toolchain/cfg and “API difference” ≠ breaking bug | Partial-call laundering and fan-out risk; controlled by two-layer denominator/public-callee filter |
| Final decision | Do not select first | Fallback for library-maintainer market or D Stage 0 failure | **Select** |

## 9. Final recommendation

Replace A1's recommendation with **Candidate D, narrowed to changed exact-public
callee over an accepted syntactic-unique local call relation**. Implement it
only with the target-support versus enumeration-completeness contract split.
Retain `direct_calls=partial`, every limitation, and the rule-level gap; never
claim total caller coverage.

Run the model-free 300-commit Stage 0 first. If fewer than 15% of commits produce
applicable D obligations, subject retention/context reduction fails, or fan-out
exceeds the frozen bound, stop and select C through a new ADR. Do not fall back
to A on the present evidence. If Stage 0 passes, authorize the 10-pair Stage 1
and preferably the 40-pair/24-hour cumulative confirmation described in
`07_feasible_evaluation_redesign.md`.

## 10. Focused verification executed

The existing tests were rerun without source changes:

- `cargo test -p reviewgraphen-core m1_tests::capability_gaps_remain_unknown_obligations_in_the_coverage_denominator -- --exact`
- `cargo test -p reviewgraphen-ingest --test m2 capability_gap_obligation_reason_and_qualification_trace_are_verified -- --exact`
- `cargo test -p reviewgraphen-ingest --test m2 qualified_call_to_an_unrelated_module_is_not_matched_to_a_same_named_local_target -- --exact`

All three passed. They verify the current partial-capability denominator and a
conservative non-match case; they do not test Candidate D, which is not
implemented.
