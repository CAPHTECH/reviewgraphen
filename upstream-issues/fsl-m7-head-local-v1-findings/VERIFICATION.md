# Verification of the 5 cross-arm-corroborated findings, before drafting issues

Status: pre-issue verification, per operator instruction. ReviewGraphen's
own discipline — "a model's agreement is not Evidence" — applies here:
these 5 findings each got an `issue_should_be_created` judge disposition
from two independently-generated arms, but a judge disposition is a
non-authoritative Claim (ADR 0035), not Evidence. This document is the
Evidence-gathering step before any of them goes to a public repository.

**Working tree discipline:** `/home/rizumita/github/fsl` was never
written to. All reading was done there (or via `git show`/`git diff`,
which touch only `.git/` metadata, not the working tree). All test
writing and execution happened in a throwaway clone at
`/tmp/.../scratchpad/fsl-verify`, checked out to the pinned experiment
revision `e589014d1655b1224f5b83a7f2de99532a0dcdba`.

## Upstream drift check

`ymm-oss/fsl`'s `main` has moved 14 commits past `e589014` (to `f6a6d42`,
v4.3.0). **All 6 files these 5 findings reference are byte-identical
between `e589014` and current `origin/main`** — verified by SHA-256 for
each file, not just `git diff --stat`'s absence of output. No finding
needed re-reading against a moved target; the code the model saw is
exactly the code that exists at HEAD today.

| File | Status |
| --- | --- |
| `rust/fsl-core/src/db.rs` | unchanged |
| `rust/fsl-core/src/compose.rs` | unchanged |
| `rust/fsl-core/src/model.rs` | unchanged |
| `rust/fsl-core/src/origin.rs` | unchanged |
| `rust/fsl-runtime/src/explicit.rs` | unchanged |
| `rust/fsl-lsp/src/server.rs` | unchanged |

## Finding 1 — `db.rs`: `safe()` is not injective, `column_member` can collide

**Status: mechanically reproduced.**

Read `column_member(column: &DbColumnRef) -> String` (`db.rs:37`):
`format!("col_{}_{}", safe(&column.0), safe(&column.1))`, where `safe()`
maps every non-alphanumeric, non-underscore character to `_`.

Wrote a unit test directly against the private `column_member`/`safe`
functions (same module, `#[cfg(test)]`), asserting a specific claimed
collision:

```rust
let a: DbColumnRef = ("a".to_owned(), "b_c".to_owned());
let b: DbColumnRef = ("a_b".to_owned(), "c".to_owned());
assert_ne!(a, b);
assert_eq!(column_member(&a), column_member(&b));
```

**Ran, passed:** both distinct `("a","b_c")` and `("a_b","c")` produce
`"col_a_b_c"`.

Confirmed by reading that `column_member`'s output is used as (a) an
`Enum` type's member name (`SpecItem::Enum { members:
columns.keys().map(column_member).collect(), ... }`, `db.rs:309`) and
(b) a map index key for three generated per-column state fields
(`column_exists`, `column_backfilled`, `column_not_null`). If `columns`
(the map being iterated, keyed by the real `DbColumnRef`) ever contains
two distinct keys that collide under `column_member`, the generated
spec would silently merge two logically distinct database columns'
tracked state into one enum member / one map key.

**Not verified:** whether any *currently existing* `.fsl` database spec
in this repository or elsewhere actually contains such a colliding
column pair — this is a reproduction of the mechanism, not a report of
an observed failure in a real spec. The vulnerability is in the
generator; whether it has fired on real input is unknown.

**Design intent:** the same file's `invariant_name` function (a few
lines above `column_member`) uses a deliberately unusual join separator
(`"QqDbSepqQ"`, with a comment explaining it's chosen for collision
avoidance against a display-format double-underscore). This shows the
author was already aware of collision risk in this exact file and had a
working technique for avoiding it — `column_member` does not use it.
This reads more like an oversight than an intentional simplification,
but that is an inference from context, not something the code proves.

## Finding 2 — `compose.rs`: reachable panic instead of a `CoreError`

**Status: mechanically reproduced**, via the actual public entry point
(`fsl_core::parse_kernel_source`), not an isolated unit call.

`rewrite_compose_statements` (`compose.rs:952`) calls
`.expect("compose alias validation occurs during lowering")` on a
`Result` collected from `resolve_alias_statement`. Traced the one error
site that can produce `Err` here: `resolve_alias_qualified_name`
(`compose.rs`, called via `resolve_alias_binder`'s `Binder::Typed` arm)
returns `Err(CoreError{message: "unknown alias '...'"})` when a
qualified type name's namespace isn't a declared component alias.

Wrote a full end-to-end test: parsed and lowered a minimal compose
document through `parse_kernel_source` (the same function
`rust/fslc` uses for compose input):

```fsl
compose Broken {
  state { x: Int }
  init {
    forall u: nonexistent.UserId { x = 0 }
  }
}
```

**Ran — this panicked, it did not return `Err`:**
```
thread '...' panicked at fsl-core/src/compose.rs:960:10:
compose alias validation occurs during lowering: CoreError { message: "unknown alias 'nonexistent'", ... }
```

This confirms the finding exactly: a syntactically valid compose
document with a plausible authoring mistake (a typo'd or forgotten
component alias in a `forall` binder's qualified type, inside a
top-level `init` block) crashes the process instead of surfacing the
`CoreError` the function's own signature promises.

**Design intent:** the `.expect()`'s own message ("compose alias
validation occurs during lowering") states the author's belief that
this case is already excluded by validation elsewhere. That belief is
demonstrably false — this reproduction is the counterexample. Nothing
found suggests this is deliberate; the `.expect()` reads as an
unproven safety assumption, not an intentional trap.

## Finding 3 — `model.rs`/`origin.rs`: `source_origin`'s hardcoded prefix reused for an unrelated error

**Status: confirmed as a code fact by reading; impact is limited and
explicitly caveated, not mechanically demonstrated to be user-visible.**

`source_origin("annotation", error.span, None)` (`model.rs:550`) is
called for an annotation-validation failure, but `source_origin` itself
hardcodes `id: "kernel:inline-initializer:{name}:..."` and
`lowering_steps: [{kind: "inline_initializer", detail: "normalized to
init assignment"}]` (`model.rs:1506`) — content written for a
completely different case (state-field inline-initializer normalization,
its only other call sites, `model.rs:626/637/648`, are genuinely about
that). An annotation-validation error carries misleading provenance
metadata describing a lowering step that never happened.

**What limits this finding's practical impact, found while checking:**
1. `OriginChain` (the struct `source_origin` builds) carries this doc
   comment: *"Internal provenance carrier. This is deliberately not
   serialized by the public Kernel v1 exporter."* (`origin.rs`, directly
   above the struct.)
2. Traced where `ModelError.origin` is actually consumed in the CLI's
   diagnostic rendering (`fslc/src/source_diagnostic.rs`,
   `model_diagnostic`): the user-visible `message` and `span` for this
   exact error path come from `error.message` (the annotation
   validator's own text) and `error.span` (set directly, correctly,
   alongside the `origin` field) — **not** from `origin.id` or
   `origin.lowering_steps`. Those two fields are constructed but not
   read anywhere in this rendering path.
3. Grepped for any consumer that pattern-matches on `OriginId`'s string
   content (e.g. the `"kernel:inline-initializer:"` prefix) to
   distinguish error origins programmatically — found none.

**Conclusion: the mismatch is real (confirmed by reading both the
construction and every consumption site), but its concrete user- or
system-visible consequence is not established** — everything checked
suggests this field is presently inert for this error path. This is
reported with that caveat, not with "verified impact."

## Finding 4 — `explicit.rs`: `init_write_key` vs `assignment_coverage` granularity mismatch

**Status: mechanically reproduced, with a materially worse consequence
than the original phrasing implied.** Two test cases, both run through
the real pipeline (`parse_direct_kernel_spec` → `build_model` →
`deterministic_initial_state`), not isolated unit calls.

Read both functions: `init_write_key` (`explicit.rs:911`) treats a
map-indexed write as `Root(name)` (whole-variable granularity) whenever
the index is a forall-bound variable, but as `ConcreteIndex(name, key)`
(per-key granularity) when the index is a literal or a free variable.
`assignment_coverage` (`explicit.rs:610`), used for the *separate*
definite-assignment tracking, does the opposite: it resolves a
forall-bound index to specific `Coverage::Keys`, per-key. The two
mechanisms disagree about what "the same write" means for exactly the
forall case.

**Test 1 — overlapping writes with conflicting values:**
```fsl
spec Probe {
  type Idx = 0..2
  state { m: Map<Idx, Bool> }
  init {
    forall i: Idx { m[i] = true }
    m[0] = false
  }
}
```
`init_write_key`'s own "assigned more than once in init" guard does
**not** fire (`Root("m")` from the forall write never equals
`ConcreteIndex("m","num:0")` from the direct write). The spec is
instead rejected later, by an unrelated mechanism, with a much less
specific message: `RuntimeError { message: "init constraints are
unsatisfiable" }`. The spec is still rejected — but not for the reason,
or with the diagnostic, the language's own "assigned more than once"
rule is meant to give.

**Test 2 — same overlap, agreeing values (the more serious case):**
```fsl
spec Probe {
  type Idx = 0..2
  state { m: Map<Idx, Bool> }
  init {
    forall i: Idx { m[i] = true }
    m[0] = true
  }
}
```
**This is silently accepted — no error at all:**
`state = {"m": Map({Int(0): Bool(true), Int(1): Bool(true), Int(2): Bool(true)})}`.
`m[0]` is genuinely assigned twice in `init` (once by the `forall`, once
directly) — precisely the condition `"state variable '{logical}'
assigned more than once in {scope}"` exists to catch (that literal
error message, read directly from the `Assign` arm of `walk_init`) — and
it is not caught, because the two writes never collide under
`init_write_key`'s inconsistent granularity, and because agreeing
values give the downstream unsatisfiability check nothing to object to.

This is a **soundness gap in a validation rule the language states it
enforces**, not merely a worse-than-ideal error message. Test 1 shows
the rule can still be rescued by an unrelated check (with a confusing
message); test 2 shows it can be bypassed completely.

**Design intent:** no comment near either function states an intended
scope difference between them; nothing found suggests this asymmetry is
deliberate.

## Finding 5 — `server.rs`: `KEYWORDS` (completion) omits `use`, unlike `is_keyword` (rename)

**Status: confirmed directly by reading — a static fact, not something
that needed dynamic reproduction.**

`KEYWORDS` (`server.rs:29`), the literal array LSP completion offers
(`server.rs:301`, the only place `KEYWORDS` is read), does not contain
`"use"` — checked by reading the full 36-entry array. `is_keyword`
(`index.rs:895`), consulted by rename validation
(`server.rs:823: !crate::index::is_keyword(value)`) and by symbol
indexing (3 sites in `index.rs`), checks `declaration_keyword(value)`
**or** `INDEX_KEYWORDS.contains(&value)` — and `INDEX_KEYWORDS`
(`index.rs:772`) has `"use"` as its literal first entry. `use` is a real
FSL keyword (compose documents: `use ComponentName as alias from
"path"`, confirmed in this repo's own `specs/bank_system.fsl`).

**Concrete, confirmed consequence:** the LSP will never suggest `use` in
its completion list while editing a compose document, even though `use`
is a real, load-bearing keyword there. Renaming a symbol *to* `use` is
correctly rejected (via `is_keyword`) — only completion is affected.
This is a real but low-severity finding: a completeness gap in editor
UX, not a compiler-correctness issue like findings 2 or 4.

**Design intent:** no comment documents `KEYWORDS`'s intended scope or
explains the split between it and `INDEX_KEYWORDS`; nothing found
suggests the omission is deliberate.

## Summary — nothing dropped, severities differ sharply

All 5 findings hold up under verification; none is withdrawn. But they
are not equally severe, and the draft issues say so explicitly:

| # | Reproduction | Severity, as far as established |
| --- | --- | --- |
| 1 | mechanical (unit test) | real generator defect; unknown whether any existing spec has triggered it |
| 2 | mechanical (real panic via public API) | real, user-triggerable crash from a plausible authoring mistake |
| 3 | code-reading only | real inconsistency; impact appears limited to an explicitly-internal, unserialized field |
| 4 | mechanical (2 end-to-end cases) | real soundness gap; silently accepted when values agree, confusing error when they conflict |
| 5 | code-reading only (static fact) | real but low-severity editor completeness gap |
