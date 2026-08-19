# Your task

You are working in a checkout of a Rust workspace at the current directory.
You have a shell. `cargo` is available and the toolchain is already
installed. You can build, run tests, and read any file in this checkout.

Work until the change is done, then stop.

## Ground rules

- You may change exactly one file: `crates/reviewgraphen-ingest/src/rust.rs`.
- **Do not add, delete, or modify any test, in this crate or any other.**
  Your change is checked by an acceptance test you have not been given and
  cannot see. Editing tests cannot help you and will be detected.
- Do not add dependencies and do not change any `Cargo.toml`.
- The existing test suite is in the checkout and you may run it.

# The change to make

Repository: ReviewGraphen (Rust, edition 2024, toolchain 1.95.0).
Crate: `reviewgraphen-ingest`. This crate ingests a Git snapshot into a
ProgramSpace of accepted structural facts plus explicit unknowns.
File you may change: `crates/reviewgraphen-ingest/src/rust.rs` — **this file
only**. It is given to you in full below.

## Background you need

`FunctionBodyVisitor` resolves unqualified (naked) calls inside function
bodies to same-named module-level functions, and tracks lexical scopes so
that a locally bound name *shadows* a module-level one. When a call is
shadowed, it must **not** be matched to the module function; instead the
unresolved call is retained as an explicit obstruction. That "record the
unknown rather than guess" posture is the crate's contract, and both halves
matter: a wrong match is a false fact, and a silently dropped call is a lost
unknown.

`visit_block` scans a block's statements for items that bind a name for the
whole block (Rust hoists items), and pushes them as shadows. It currently
handles `fn`, `const`, `static`, tuple/unit `struct`, and `use` items, and
treats a glob `use` as making the whole block conservatively unresolved.

## The defect to fix

A block-local **foreign module** — `extern "C" { ... }` written inside a
function body — is not handled. Its `fn` and `static` declarations put their
names in the **value namespace** of the enclosing block, exactly as a
block-local `fn` or `static` item does, so they shadow a same-named
module-level function for a naked call in that block. Today they fall into
`visit_block`'s catch-all arm, contribute no shadow, and the naked call is
still matched to the module-level function. That match is unsound.

## Required behaviour after your change

1. A block-local `extern "C" { fn target(); }` makes a naked `target()` call
   in that block **shadowed**: no `calls` relation from the enclosing
   function to the module-level `target`, and the unresolved call retained
   as a `RelationUnresolved` obstruction naming the enclosing function.
2. A block-local `extern "C" { static target: u8; }` does the same. A
   foreign `static` is in the value namespace just as a foreign `fn` is.
3. The shadow is scoped to its own block and **must not leak past it**. In
   a function whose body is `{ extern "C" { fn target(); } target(); }
   target();` the inner call stays unresolved and the outer call still
   resolves to the module-level `target` — exactly one resolved edge for
   that caller.
4. A block-local `extern "C"` block that declares **other** names only —
   `extern "C" { fn unrelated(); }` — must **not** block anything: a naked
   `target()` call in that block still resolves to the module-level
   `target`. Marking the whole block conservatively unresolved whenever any
   foreign module is present would satisfy points 1-3 and violate this one.
5. A naked call in a block with no foreign module at all still resolves
   exactly as before.
6. Every existing behaviour of this crate is preserved. In particular the
   existing shadow handling for `fn`/`const`/`static`/tuple-struct/`use`
   items, glob-`use` conservatism, pattern-position shadowing, the
   obstruction ledger, and the base-invariance of snapshot and fact IDs are
   all unchanged.

## How the change is checked

Three commands are run against a fresh, isolated copy of the repository
after your change is applied. They are the only checks that count.

```text
cargo build -p reviewgraphen-ingest
cargo clippy -p reviewgraphen-ingest --all-targets
cargo test  -p reviewgraphen-ingest
```

`cargo test -p reviewgraphen-ingest` runs the crate's whole existing test
suite — 115 tests you have not been shown, across unit tests and the
integration suites `tests/m2.rs` and `tests/extraction_report_schema.rs` —
plus a harness-owned acceptance test of the behaviour above. All of them
must pass. All 115 existing tests pass on the code as given to you; the
acceptance test does not.

You are not shown the acceptance test. You may not add or change any test.

## Constraints

- Rust edition 2024, toolchain 1.95.0. `unsafe_code` is `forbid` at the
  workspace level: your change must not contain the `unsafe` keyword.
  (Declaring that the *ingested fixture source* contains an `extern` block
  is a fact about parsed input, not about this crate's own code.)
- `unused_must_use`, `clippy::dbg_macro`, `clippy::todo`, and
  `clippy::unimplemented` are `deny`.
- The parser is `syn` 2.0.119 with features `full` and `visit`.
- Do not add dependencies, do not change any `Cargo.toml`, do not add or
  change any other file.
- Keep the change as small as it can be.

## When you are done

Leave the working tree in the state you want checked. Then stop and briefly
state what you changed and what you ran.
