# ADR 0047: Accept Rust test scope and enumerate near-body candidates

- Status: Experimental
- Date: 2026-09-14

## Context

The exact-body benchmark excluded tests using a partial function flag. It did
not cover `#[cfg(test)]` helpers, test modules, or methods consistently. It also
missed identifier- and literal-renamed copies even though those are a common
form of repeated implementation.

Neither source similarity nor a normalized token shape establishes a shared
responsibility, contract, or change reason.

## Decision

The Rust ingest adapter emits two accepted, versioned artifact facts for every
function and method:

- `test_scope` with `reviewgraphen.ingest.rust-test-scope@1`, derived from the
  item and enclosing module/impl attributes;
- `responsibility_shape_hash` with
  `reviewgraphen.ingest.rust-responsibility-shape@1`, derived by preserving Rust
  keywords, delimiters, and punctuation while erasing identifier and literal
  values.

Exact discovery advances to report v2/extractor v2 and consumes only the
accepted test-scope fact. Near discovery v1 groups equal responsibility-shape
hashes only when at least two distinct exact-body hashes exist. Both reports
remain benchmark-only candidate inventories, expose their full denominator,
and classify missing facts as unknown exclusions.

The production path profile accepts both ReviewGraphen's
`crates/*/src/**/*.rs` layout and FSL's `rust/*/src/**/*.rs` layout. This is a
declared profile expansion, not a general Rust-workspace claim.

## Consequences

Test exclusion is reproducible and no longer guessed by the candidate layer.
Type-2-like clones become visible. Type-3 edits, macro expansions, semantic
equivalence, shared change reason, and abstraction fitness remain unknown.
Exact v1 schema/fixture bytes remain available; consumers must opt into v2.
