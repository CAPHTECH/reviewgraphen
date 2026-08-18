## Summary

`db.rs`'s `column_member()` generates a column's identity string using a
sanitizer (`safe()`) that is not injective, so two distinct
`(table, column)` pairs can produce the same generated identity — a
mechanically reproduced collision, not a hypothetical one.

## Where

`rust/fsl-core/src/db.rs`, `safe()` (line 17) and `column_member()`
(line 37), at commit `e589014d1655b1224f5b83a7f2de99532a0dcdba` (current
`main` as of this writing — the file is unchanged since).

```rust
fn column_member(column: &DbColumnRef) -> String {
    format!("col_{}_{}", safe(&column.0), safe(&column.1))
}
```

`safe()` maps every character that is not ASCII-alphanumeric or `_` to
`_`. It does not otherwise disambiguate where the original boundary
between characters was.

## Reproduction — mechanical, verified

```rust
let a: DbColumnRef = ("a".to_owned(), "b_c".to_owned());
let b: DbColumnRef = ("a_b".to_owned(), "c".to_owned());
assert_ne!(a, b);                                  // distinct inputs
assert_eq!(column_member(&a), column_member(&b));  // -> both "col_a_b_c"
```

Run against a clone at `e589014` (a throwaway test appended locally,
not committed): **passed** — both distinct pairs produce `"col_a_b_c"`.

## Why this matters, as far as I could establish

`column_member`'s output is used as (a) the member name of a generated
`Enum` type (`db.rs:309`, `members: columns.keys().map(column_member).collect()`)
and (b) a map index key for three generated per-column state fields
(`column_exists`, `column_backfilled`, `column_not_null`). If the real
`columns` map (keyed by the actual `(table, column)` pairs) ever
contains two keys that collide under `column_member`, the generated
spec would silently merge two logically distinct database columns'
tracked state into one.

**Not verified:** whether any existing `.fsl` database spec in this
repository (or elsewhere) actually contains such a colliding pair. This
report reproduces the mechanism; it does not claim an observed failure
in a real spec.

## Possibly relevant context

The same file's `invariant_name()` (a few lines above `column_member`)
deliberately uses an unusual join separator (`"QqDbSepqQ"`) with a
comment explaining it is chosen to avoid a different collision. That
suggests the file's author was already thinking about collision safety
here and had a working technique for it elsewhere in the same file —
`column_member` doesn't use it. I'm noting this as context, not as a
claim about intent; I don't know why the two functions differ.

## Verification note

This report and the reproduction above were produced by an AI agent
(Claude) as part of an independent review exercise, then verified by
running the test shown against the actual source at the commit named
above. I'm disclosing the origin so you can weigh it appropriately —
please evaluate the code and the test result on their own merits, not
on the fact that an AI flagged it.
