# 24. Context-projection feasibility for implementation targets

Status: feasibility finding, 2026-08-20. No model request of any kind was made
to produce it. Recorded here because the investigating agent was blocked from
writing its own report file; the findings are its work, transcribed.

## Question

The operator proposed that ReviewGraphen's role in *implementation* should be
grasping the target region — ProgramSpace facts plus a bounded context
projection — with no obligation enumeration (the spec already states what must
hold) and no claim/evidence records (the compiler and tests are the evidence).

Can the context-projection layer, as it exists on
`m7-detection-benchmark-and-full-verification` today, produce a projection over
`reviewgraphen-ingest` that would help make the `m8-impl-local-v1` task-2
change?

## Verdict: no, for two independent reasons

### 1. It hard-fails on this corpus — and this is a regression introduced today

`prepare_context` aborts with

```
validation failure: reached range-bearing artifact state:sha256:... has no
containing candidate file
```

whenever discovery reaches a `state:*` artifact. That happens on any Rust
snapshot whose changed region overlaps a syntactic write target — including a
one-line edit inside `visit_block` itself.

Mechanism:

- `reviewgraphen-ingest/src/rust.rs:76-110` creates `state:*` artifacts with a
  location and a `writes` relation, and **no `contains` edge to the enclosing
  file**.
- `writes` is not in the traversal adjacency
  (`reviewgraphen-core/src/context.rs:3110-3114` admits only `calls`,
  `contains`, `covers`), so these artifacts were unreachable and the defect was
  latent.
- **`7b40d48` ("ingest: wire changed-structure into obligation synthesis")**
  gave the `change:*` artifact a `contains` edge to every accepted record
  overlapping its changed lines. `change:*` has `kind == "custom"`, not
  `"file"`, and has no parent of its own.
- `state:*` is now reachable via file -> module -> function -> reverse contains
  -> `change:*` -> forward contains -> `state:*`. `discover`'s `file_for` walk
  (`context.rs:2843-2882`) climbs `contains` parents looking for a
  `kind == "file"` artifact, hits the parentless `change:*`, and returns empty.
  `prepare_context` (`context.rs:1943-1959`) treats an empty `file_for` for a
  reached range-bearing artifact as a hard `DomainError::Validation`.

Verified with a standalone probe: for the test snapshot the `file_for` closure
is empty for **all 22** `state:*` artifacts, and each one's only `contains`
parent is `change:added crates/reviewgraphen-ingest/src/<file>.rs`.

Three of three attempted runs reproduced this except where the diff happened
not to overlap a `state:*` line range.

### 2. When it does run, it returns the wrong bytes

On the configuration that survives:

| excerpt | path | range | bytes |
| --- | --- | --- | --- |
| `sources/0000.txt` | `reviewgraphen-ingest/src/rust.rs` | lines 1-400 | 15,928 |
| `sources/0001.txt` | `reviewgraphen-ingest/src/git.rs` | lines 1-400 | 19,789 |
| `sources/0002.txt` | `reviewgraphen-ingest/src/lib.rs` | lines 1-400 | 16,451 |

Four envelopes, all four obligations `reviewgraphen.capability_gap`, the three
excerpt files byte-identical across all four (md5-verified).

Coverage of the target file: **lines 1-400 of 2,062 (19.4%)**. Coverage of the
target region — `FunctionBodyVisitor` (1442-1460), `push_scope` (1481-1490),
`is_locally_bound` (1515-1519), `record_call` (1609-1768), `visit_expr_call`
(1802-1805), `visit_block` (1807-1863): **0%**.

`context.rs:3159-3231` builds **one contiguous window per file**: start = lowest
anchor start, end = max anchor end, truncated forward from start at
`MAX_LINES = 400`. The file artifact itself spans 1-2062, so start is always 1
and the window is always the first 400 lines for any file-seeded obligation on
a file longer than 400 lines.

### It misses even with a perfect seed

Seeded exactly at `visit_block`, discovery reaches via `calls` exactly one
thing: `use_item_bindings` (1348-1362). Anchors {1348-1362, 1807-1863} give
start 1348, end 1863, span 515 > 400, truncated to **1348-1747** — ending 60
lines before `visit_block` begins. The projection returns the callee and drops
the subject.

The facts needed for the change are not linked at all: `visit_expr_method_call`
(`rust.rs:1941`) records every method call as a `DynamicDispatchUnresolved`
obstruction rather than a `calls` edge, so there is no relation from
`visit_block` to `push_scope`, `pop_scope`, `is_locally_bound`, or
`record_call` — precisely the code a reader must understand to know what a
shadow is and how scope-leak and unrelated-name requirements are enforced.

## Would it have saved the 90-minute `syn`-reading trial?

No. That trial spent its budget learning how foreign modules appear in `syn`'s
AST (`Item::ForeignMod`, `ForeignItem::Fn`, `ForeignItem::Static`). The corpus
is the repository's own files; nothing in the projection concerns a
dependency's API surface.

Handing the model the whole 81 KB file — what the `b1` arm already does — is
strictly more useful than the current projection, which has zero coverage of
the relevant region and is larger in total bytes once replicated per
obligation (249,048 vs 321,011 total, but only 52,168 bytes of distinct source
in the projection).

What the projection does provide, stated fairly: which files are in the
snapshot, what was not reached and why, which capability gaps exist, that the
excerpt was truncated, and a content hash for every byte shown. That is
provenance and honesty machinery. It is not comprehension of a target region.

## What would have to change

Three separate layers, not a tuning problem:

1. **Ingest** — `state:*` needs a `contains` edge to its file, or `change:*`
   needs a parent, or `prepare_context` must tolerate an unparented
   range-bearing artifact. Today this is a hard crash on ordinary Rust diffs.
   **Resolved 2026-08-20 in the projection layer; see the section below.**
2. **Excerpt policy** — one contiguous 400-line window anchored at the lowest
   reached line cannot express "these five regions scattered across a
   2,000-line file". Multi-window excerpts per file are the minimum.
3. **Seeding** — there is no way to ask for a projection around a named symbol.
   The only entry point is per-obligation, and the obligation *is* the seed.

Item 3 is the one that blocks the operator's proposal directly. Even with 1 and
2 fixed, "project the context for `FunctionBodyVisitor::visit_block`" is not
something ReviewGraphen can be asked today.

## Resolution of item 1 (2026-08-20)

Fixed in the projection layer, not in ingest. `prepare_context` now treats a
reached range-bearing artifact with an empty containing-file closure as *not an
anchor*, instead of aborting.

The choice was between changing what ProgramSpace asserts and changing what the
projection tolerates. The two ingest-side options assert something the
extractor does not know:

- *`state:*` gains a `contains` edge to its file.* M2's containment family is
  exactly file -> module and module -> declared symbol
  (docs/20_m2_ingestion_contract.md). A `state:*` artifact is not a declared
  symbol; it is a *direct path/field/index assignment target*, keyed
  per-file only as deduplication, carrying the first write's span rather than a
  declaration span, and explicitly unresolved -- the extractor records that the
  token `self.total` was assigned, never what it refers to. Asserting
  containment would promote an unresolved syntactic token to a structural
  membership fact, and every reader of `contains` would then traverse it:
  unbounded-depth forward/reverse discovery, anchoring (a projection window
  anchored on an arbitrary assignment line), `is_changed_public_symbol`, and
  the store/index containment projections.
- *`change:*` gains a parent.* `file --contains--> change:*` would make a
  file's containment closure base-relative and would create a second,
  base-dependent path from a file down to its own symbols, changing discovery
  path ranks and `path_cap` selection on every real snapshot. A change record
  is also not a member of the file's structure; the `changed_by` edge already
  expresses the true relationship, in the direction the contract names.

The projection-layer assumption, by contrast, was never a contract. Nothing in
ADR 0016 or docs/20 promises that every range-bearing artifact has a file
ancestor through `contains`; ADR 0016 defines a file's anchors as exactly those
reached locations that *do* resolve through the reverse-`contains` closure --
a filter, not a demand. Discovery already treats an unresolved owner that way
nine lines earlier in the same function: it contributes no candidate file, no
`path_cap` entry, and no `test_cap` entry. The anchor loop was the single site
that instead failed closed. Only the exact-path check is a real integrity
claim, and it is unchanged: an artifact located in one file must never be
contained by another.

Re-validated over the same pinned axum range (`e4550d23..97def959`, throwaway
clone, nothing retained): 1 `async.concurrent_reentry` at `applicable` under
`node.changed_public_symbol@2` plus 4 `reviewgraphen.capability_gap`, and the
self-diff control at 0 substantive -- unchanged from what `7b40d48`
established, since the fix does not touch synthesis. Context projection over
that snapshot went from 1 of 5 obligations to 5 of 5; the four that failed were
the snapshot-seeded capability gaps, which reach the whole graph. That
snapshot carries 570 range-bearing artifacts with no file/module containment
parent.

Items 2 (single contiguous excerpt window) and 3 (no symbol-seeded projection)
are untouched and still stand.
