# Implementation task (identical text in every arm)

Repository: ReviewGraphen (Rust, edition 2024, toolchain 1.95.0).
Crate: `reviewgraphen-cli`.
File you may change: `crates/reviewgraphen-cli/src/lib.rs` — **this file only**.

## What must change

`reviewgraphen_cli::run` currently accepts the generic review command in
exactly one argument order:

```text
review --request <request.json> --artifacts <artifact-root>
```

It must also accept the same two flag/value pairs in the opposite order:

```text
review --artifacts <artifact-root> --request <request.json>
```

Both orders must call `generic_review(Path::new(<request.json>), Path::new(<artifact-root>))`
with the same two values.

## What must be preserved

Everything else about the command surface stays exactly as it is now:

1. Any `review` command line that is not exactly one `--request <value>`
   pair plus one `--artifacts <value>` pair is rejected with
   `CommandOutcome::failure(2, usage())` — exit code 2, empty stdout, and the
   **unchanged** `usage()` string on stderr. This includes: `review` with no
   arguments, a missing pair, a flag with no value, a repeated `--request`,
   a repeated `--artifacts`, any unknown flag such as `--fixture`, and any
   extra trailing argument.
2. Flag **values are consumed positionally**. The token immediately after
   `--request` is its value even if that token itself looks like a flag. For
   example `review --request --artifacts --artifacts <dir>` is a complete,
   accepted command whose request path is the literal string `--artifacts`.
   Do not implement this by scanning for flag-looking tokens.
3. The `schema list`, `schema print <name>`, and `schema validate <path>`
   command forms, and the final catch-all rejection, are unchanged.
4. `usage()`'s text is unchanged.
5. The three existing `#[cfg(test)] mod tests` tests in the same file keep
   passing, and the file's existing behaviour for every other input is
   unchanged.

## How the change is checked

Two commands are run against a fresh, isolated copy of the repository after
your change is applied. They are the only checks that count.

```text
cargo build -p reviewgraphen-cli
cargo test  -p reviewgraphen-cli
```

The acceptance test file
`crates/reviewgraphen-cli/tests/review_flag_order.rs` is supplied to you
verbatim below and is already present in that copy. You must not change it,
and you cannot: your edit may only touch
`crates/reviewgraphen-cli/src/lib.rs`.

On the current, unchanged code that test file compiles, and exactly one of
its six tests fails: `artifacts_before_request_is_accepted`. The other five
pass. A correct change makes all six pass without breaking the three
in-file unit tests.

## Constraints

- Rust edition 2024, toolchain 1.95.0. `unsafe_code` is `forbid` at the
  workspace level; `unused_must_use`, `clippy::dbg_macro`, `clippy::todo`,
  and `clippy::unimplemented` are `deny`.
- Do not add dependencies, do not change any `Cargo.toml`, do not add or
  change any other file.
- Do not change any function other than `run` (adding a private helper
  function inside `crates/reviewgraphen-cli/src/lib.rs` is allowed).
