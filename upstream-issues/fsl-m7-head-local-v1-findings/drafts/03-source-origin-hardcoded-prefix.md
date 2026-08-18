## Summary

`source_origin()` in `model.rs` hardcodes provenance metadata written
for state-field inline-initializer normalization, but is also reused
for annotation-validation errors — an unrelated case. Confirmed by
reading construction and every consumption site I could find; **I could
not establish a concrete user- or system-visible consequence**, and I'm
reporting that limitation plainly rather than implying more impact than
I've shown.

## Where

`rust/fsl-core/src/model.rs`, at commit
`e589014d1655b1224f5b83a7f2de99532a0dcdba` (unchanged on current `main`
as of this writing):

`source_origin()` (line 1506) hardcodes:
```rust
id: OriginId(format!("kernel:inline-initializer:{name}:{}:{}", ...)),
...
lowering_steps: vec![LoweringStep {
    kind: "inline_initializer".to_owned(),
    detail: Some("normalized to init assignment".to_owned()),
}],
```

Three of its four call sites (`model.rs:626/637/648`) are genuinely
about inline-initializer normalization. The fourth (`model.rs:550`) is
not:
```rust
self.annotations.validate().map_err(|error| ModelError {
    message: error.message,
    origin: Some(Box::new(source_origin("annotation", error.span, None))),
    span: Some(error.span),
    name_resolution: false,
})?;
```
An annotation-validation failure gets an `origin` claiming
`"kernel:inline-initializer:annotation:..."` and a lowering step saying
"normalized to init assignment" — neither of which happened.

## What I checked for impact, and what I found

1. `OriginChain` (the struct this builds) carries the doc comment
   *"Internal provenance carrier. This is deliberately not serialized
   by the public Kernel v1 exporter."*
2. Traced how `ModelError.origin` is actually used in `rust/fslc`'s
   diagnostic rendering (`source_diagnostic.rs`, `model_diagnostic`):
   the user-visible message and span for this exact error path come
   from `error.message` and `error.span` — both set correctly,
   independent of `origin` — not from `origin.id` or
   `origin.lowering_steps`.
3. Searched for any code that pattern-matches on `OriginId`'s string
   content (e.g. the `"kernel:inline-initializer:"` prefix). Found
   none.

**I'm not able to show this currently causes an incorrect user-facing
message, a wrong decision downstream, or any other observable effect.**
It may be presently inert. I'm filing it because the inconsistency
itself is real and confirmed, and because `OriginChain` looks like
internal diagnostic infrastructure that could plausibly grow a new
consumer later (e.g. a future JSON export, or a `fslc explain`-style
feature) — at which point this mislabeling would become visible without
anyone having decided it should be. If you already know this field is
dead/unused, this may not be worth acting on; I'd rather say that
plainly than inflate the report.

## Verification note

This report was produced by an AI agent (Claude) as part of an
independent review exercise; I traced construction and every
consumption site by reading the source at the commit named above, but I
did not write a test demonstrating an observable effect, because I
could not find one to demonstrate. I'm disclosing the origin so you can
weigh it appropriately — please evaluate the code on its own merits,
not on the fact that an AI flagged it.
