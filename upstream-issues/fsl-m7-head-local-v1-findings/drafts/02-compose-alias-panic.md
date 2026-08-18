## Summary

A compose document with an undeclared component alias inside a
top-level `init` block's `forall` binder crashes the process
(`.expect()` panic) instead of returning the `CoreError` the lowering
function's own signature promises. Reproduced end-to-end through the
actual public parsing entry point, not an isolated unit call.

## Where

`rust/fsl-core/src/compose.rs`, `rewrite_compose_statements()` (line
952), at commit `e589014d1655b1224f5b83a7f2de99532a0dcdba` (unchanged on
current `main` as of this writing):

```rust
fn rewrite_compose_statements(
    statements: Vec<Statement>,
    components: &BTreeMap<String, Component>,
) -> Vec<Statement> {
    statements
        .into_iter()
        .map(|statement| resolve_alias_statement(statement, components))
        .collect::<Result<_, _>>()
        .expect("compose alias validation occurs during lowering")
}
```

`resolve_alias_statement` can return `Err(CoreError)` — traced the path:
a `forall` binder with a qualified type name (`alias.TypeName`) goes
through `resolve_alias_binder` → `resolve_alias_qualified_name`, which
returns `Err(CoreError{message: "unknown alias '...'"})` when the
namespace isn't a declared `use` alias. That `Err` propagates back to
`rewrite_compose_statements`'s `.expect()`, which then panics instead of
surfacing it.

## Reproduction — mechanical, verified

Minimal compose document, no `use` items needed to trigger it:

```fsl
compose Broken {
  state { x: Int }
  init {
    forall u: nonexistent.UserId { x = 0 }
  }
}
```

Parsed and lowered through `fsl_core::parse_kernel_source` (the actual
public entry point `rust/fslc` uses for compose files) against a clone
at `e589014`:

```
thread '...' panicked at fsl-core/src/compose.rs:960:10:
compose alias validation occurs during lowering: CoreError { message: "unknown alias 'nonexistent'", line: 1, column: 1, origin: None, name_resolution: false }
```

**The process panics; `parse_kernel_source` never returns.** A caller
(e.g. `fslc` itself, or any tool built on this crate) has no `Result` to
handle — the panic unwinds past the public API boundary.

## Why this is reachable

This isn't a contrived internal state — it's the result of an ordinary
authoring mistake: typo'ing a component alias, or forgetting a `use`
line, inside a compose document's shared (`common`) `init` block. No
`use` declaration is even required for the crash to occur.

## Possibly relevant context

The `.expect()`'s own message — "compose alias validation occurs during
lowering" — states the author's belief that this exact case is already
excluded before this point runs. The reproduction above is a direct
counterexample to that belief. I found no other validation pass, in
this crate or its callers, that would reject the undeclared-alias case
before reaching this line.

## Verification note

This report and the reproduction above were produced by an AI agent
(Claude) as part of an independent review exercise, then verified by
running the exact test shown against the actual source at the commit
named above. I'm disclosing the origin so you can weigh it
appropriately — please evaluate the code and the panic on their own
merits, not on the fact that an AI flagged it.
