# Specification the change was written against

Repository: a Rust workspace, edition 2024, toolchain 1.95.0.
Crate: `reviewgraphen-cli`.
The change may touch `crates/reviewgraphen-cli/src/lib.rs` and nothing else.

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
   The rule must not be implemented by scanning for flag-looking tokens.
3. The `schema list`, `schema print <name>`, and `schema validate <path>`
   command forms, and the final catch-all rejection, are unchanged.
4. `usage()`'s text is unchanged.
5. The existing in-file unit tests keep passing, and the file's existing
   behaviour for every other input is unchanged.

## Constraints the author was given

- Rust edition 2024, toolchain 1.95.0. `unsafe_code` is `forbid` at the
  workspace level; `unused_must_use`, `clippy::dbg_macro`, `clippy::todo`,
  and `clippy::unimplemented` are `deny`.
- No new dependencies, no manifest changes, no other file added or changed.
- No function other than `run` may be changed; adding a private helper
  function in the same file is allowed.
- Keep the change as small as it can be.
