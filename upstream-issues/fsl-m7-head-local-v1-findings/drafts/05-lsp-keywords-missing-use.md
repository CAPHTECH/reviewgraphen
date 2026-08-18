## Summary

The FSL LSP's completion keyword list doesn't include `use`, even
though `use` is a real, load-bearing keyword in compose documents and
is correctly recognized as reserved by the (separate) rename-validation
keyword check. Confirmed by reading both lists directly — this is a
static fact, not something that needed a runtime reproduction.

## Where

`rust/fsl-lsp/src/server.rs`, at commit
`e589014d1655b1224f5b83a7f2de99532a0dcdba` (unchanged on current `main`
as of this writing).

`KEYWORDS` (line 29), the array LSP completion offers — and the *only*
place it's read is `server.rs:301`, `items.extend(KEYWORDS.iter()...)`
— is a 36-entry literal list. I read all 36 entries directly; `"use"`
is not among them.

`is_keyword()` (`rust/fsl-lsp/src/index.rs:895`) is consulted by rename
validation (`server.rs:823`, `&& !crate::index::is_keyword(value)`) and
by symbol indexing (3 more sites in `index.rs`). It checks
`declaration_keyword(value).is_some() || INDEX_KEYWORDS.contains(&value)`,
and `INDEX_KEYWORDS` (`index.rs:772`) has `"use"` as its literal first
entry.

`use` is a real FSL keyword — compose documents declare component
imports with `use ComponentName as alias from "path"`, confirmed in
this repository's own `specs/bank_system.fsl`.

## Concrete, confirmed consequence

While editing a compose document, the LSP's completion list will never
suggest `use`, even though it's a real reserved word there. Renaming a
symbol *to* the literal name `use` is correctly rejected (via
`is_keyword`) — only completion is affected, not correctness.

## Severity

Low. This is an editor-UX completeness gap, not a compiler-correctness
issue — nothing about parsing, lowering, or validation is affected;
`use` still parses and works exactly as documented, it's just absent
from the suggestion list.

## Possibly relevant context

I found no comment documenting `KEYWORDS`'s intended scope, or
explaining why it's a separate list from `INDEX_KEYWORDS` rather than
sharing a common keyword source. Nothing found suggests the omission is
deliberate — it reads more like `KEYWORDS` was assembled by hand at some
point and not kept in sync when `use`/compose syntax was added or grew,
but I can't confirm that history from the code alone.

## Verification note

This report was produced by an AI agent (Claude) as part of an
independent review exercise, verified by reading both arrays and their
consumers directly at the commit named above (no test was needed — the
omission is directly observable in the source). I'm disclosing the
origin so you can weigh it appropriately — please evaluate the code on
its own merits, not on the fact that an AI flagged it.
