## Summary

`init`'s "assigned more than once" duplicate-write check and its
separate definite-assignment coverage tracking use different
granularity for the same statement shape (a `forall`-indexed map
write), and the mismatch is not just a worse error message — **an
overlapping double-write to `init` is silently accepted with no error
at all when the two writes happen to agree on value.** Reproduced
end-to-end, twice, through the real spec → model → initial-state
pipeline.

## Where

`rust/fsl-runtime/src/explicit.rs`, at commit
`e589014d1655b1224f5b83a7f2de99532a0dcdba` (unchanged on current `main`
as of this writing).

`init_write_key()` (line 911): for `LValue::Index(name, index)`, if
`index` is a variable bound by an enclosing `forall`
(`bound_names.contains_key(key)`), it falls through to
`InitWriteKey::Root(name)` — whole-variable granularity. If `index` is a
literal or a *free* variable, it returns `InitWriteKey::ConcreteIndex(name, key)`
— per-key granularity.

`assignment_coverage()` (line 610), used separately for definite-assignment
tracking on the same statements, does the opposite for the forall case:
a forall-bound index resolves to specific `Coverage::Keys(binder_values)`
— per-key.

`walk_init`'s duplicate check (`explicit.rs`, `Statement::Assign` arm)
uses only `init_write_key`'s value to detect a repeat write:
```rust
let key = init_write_key(target, bound_names);
if possibly_assigned.contains(&key) {
    return Err(runtime_error(format!(
        "state variable '{logical}' assigned more than once in {scope}"
    )));
}
```
Because `init_write_key` collapses every forall-indexed write on the
same variable to one `Root(name)` key regardless of which key it
actually touches, it cannot detect an overlap between a forall write and
a later concrete-index write to a key the forall already covered — they
hash to different `InitWriteKey` values.

## Reproduction — mechanical, verified, two cases

Both run through `parse_direct_kernel_spec` → `build_model` →
`deterministic_initial_state` (the real pipeline, not an isolated call)
against a clone at `e589014`.

**Case 1 — conflicting values:**
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
Result: `Err(RuntimeError { message: "init constraints are unsatisfiable" })`.
The spec is rejected, but not for the reason or with the diagnostic the
"assigned more than once" rule is meant to give — some other,
downstream mechanism catches the value conflict incidentally.

**Case 2 — agreeing values (the serious one):**
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
Result: **`Ok`, no error at all.**
`state = {"m": Map({Int(0): Bool(true), Int(1): Bool(true), Int(2): Bool(true)})}`.

`m[0]` is genuinely written twice in `init` — once by the `forall`, once
directly — which is exactly what `"assigned more than once in {scope}"`
exists to reject (that string is the literal error message in the
source). It is not rejected here, because the two writes never collide
under `init_write_key`, and because the values happening to agree gives
the downstream unsatisfiability check (which caught case 1 incidentally)
nothing to object to either.

## Why I'm calling this a soundness gap, not just a message-quality issue

Case 1 shows the language's stated rule can still be rescued by an
unrelated check, with a confusing message. Case 2 shows the rule can be
bypassed completely — a spec with a genuine, redundant double-assignment
in `init` compiles and runs as if it were unambiguous, with no diagnostic
at all telling the author their `init` block says the same thing twice.

**Not verified:** whether this pattern (a `forall` write followed by a
concrete-index write to a value the forall already covers) occurs in
any spec in this repository today. This reproduces the mechanism on a
minimal example; I have not searched the existing spec corpus for real
instances.

## Possibly relevant context

I found no comment near either function documenting an intended scope
difference between `init_write_key`'s duplicate-detection granularity
and `assignment_coverage`'s tracking granularity. Nothing I found
suggests the asymmetry is deliberate, but I can't rule that out from
reading alone.

## Verification note

This report and both reproductions above were produced by an AI agent
(Claude) as part of an independent review exercise, then verified by
running the exact tests shown against the actual source at the commit
named above. I'm disclosing the origin so you can weigh it
appropriately — please evaluate the code and the test results on their
own merits, not on the fact that an AI flagged it.
