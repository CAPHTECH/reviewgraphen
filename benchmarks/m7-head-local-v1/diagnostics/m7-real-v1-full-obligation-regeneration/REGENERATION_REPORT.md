# m7-real-v1 full_review_graphen obligation regeneration

Status: diagnostic only. `benchmarks/m7-real-v1/` is not modified anywhere
by this investigation — every file referenced there was read-only.
Regenerated artifacts live only under this directory.

## Question: did m7-real-v1's actual scored full_review_graphen trials receive capability_gap-only obligations?

**Confirmed for snapshot-01, directly, not inferred.**

### What was reconstructed and how

`benchmarks/m7-real-v1/results/full-reviewgraphen-replicate-2/manifests/
snapshot-01.json` records `input_tree_hash: "git:76cb3c9b3ef46c9f77f716
c1cad083dc2eeba882"` and a `source_inventory` of exactly two production
files: `rust/fsl-tools/src/domain.rs` and `rust/fslc/src/main.rs`, each
with a declared `content_hash`.

1. Fetched both files at that exact tree from the live `fsl` repository
   (`git show 76cb3c9b3ef46c9f77f716c1cad083dc2eeba882:<path>`; the tree
   object is still reachable there). `domain.rs`'s raw content hashed
   directly to the manifest's declared value. `main.rs` did not — it
   contains one `#[cfg(test)]` block.
2. Found that `benchmarks/m7-real-v1/scripts/build_corpus.py`'s own
   `projected_blob()`/`redact_blind_source()` (used elsewhere in this
   repository, e.g. by `m7-head-issue-v1/scripts/prepare_judge_
   calibration.py`, to strip inline test modules and issue/branch
   references without changing line numbers) applied to the same raw
   blob produces content whose SHA-256 **exactly matches** the manifest's
   declared `content_hash` for `main.rs`. Confirmed by direct hash
   comparison, not assumption.
3. Staged both redacted files into a fresh Git snapshot (same empty-base-
   then-snapshot-commit pattern already used by
   `stage_real_snapshot`/`m7-head-local-v1`'s own packet builder) and ran
   the new `prepare-head-local-unit` subcommand — which internally calls
   the exact same `prepare_from_ingest_request` →
   `prepare_real_b1`/`prepare_real_full_review` primitives that
   `prepare_real_full` (the function class that built the real,
   score-contributing m7-real-v1 packets) calls — into this diagnostic
   directory only.

### Result

The regenerated `full/agent_input/obligations.json` (copied here as
`regenerated-snapshot-01-full-obligations.json`) contains **5 obligations,
all `property_id: "reviewgraphen.capability_gap"`, all
`applicability_status: "unknown"`**, with the identical missing-capability
reasons already documented in `CAPABILITY_GAP_DIAGNOSIS.md` for every
other packet checked (`capability_undeclared:concurrency_model`,
`capability_partial:direct_calls`, `capability_partial:test_mapping`, one
per each of the 5 `MvpRulePack` rule origins). This is the same pattern as
`head-local-08`, `m7-local-factorial-v2` snapshot-06 and snapshot-40, and
`m7-pilot-v2`'s `g3_proxy` arm — now confirmed for an actual m7-real-v1
scored unit as well.

The regenerated `mechanism-ontology.json` hashes to
`sha256:7f79f39e74fc9e50151fb9a8c2895c6c19a0ebd5ac64295112e9e1a2073a820a`,
which **is** present in the original manifest's `packet_hashes` list —
i.e., this file is proven byte-identical to what the real trial received,
cross-validating that this regeneration pipeline faithfully reproduces the
real construction process (it is not merely "plausible," this specific
file is an exact match).

### What was not achieved, stated plainly

The full packet is not byte-for-byte identical to the original: 6 of 17
`packet_hashes` matched exactly (both source files' content hashes,
`mechanism-ontology.json`, and 3 others), 11 did not, including
`obligations.json` itself. The most likely explanation, consistent with
everything else observed: obligation IDs are content-hashed from
identifiers that depend on the exact snapshot/tree identity, and
`input_tree_hash` in this reconstruction does not equal the original
`76cb3c9b3ef46c9f77f716c1cad083dc2eeba882` (my reconstructed tree, using
the verified-correct redacted file bytes, hashes to a different value —
most likely because the real preparation pipeline's exact base-commit/
staging mechanics differ from this reconstruction's in some detail not
fully identified). This changes embedded IDs and hashes throughout the
packet without changing the *kind* of content produced, because capability
declarations (`rust.rs`) and the rule-trigger patterns (`MvpRulePack`) that
determine capability_gap-vs-substantive are independent of exact tree
identity — they depend only on the ingest adapter's fixed capability
declarations and on relation kinds/attributes that, per
`CAPABILITY_GAP_DIAGNOSIS.md` question 3, are never produced by real
ingestion regardless of which snapshot is processed. This is why the
5-obligation capability_gap result is reported as **confirmed** despite
the byte-level packet not matching in full: the mechanism producing that
result does not depend on the part that didn't reproduce exactly.

Only snapshot-01 was directly regenerated. The other 39 m7-real-v1 units
were not individually reconstructed; extending this result to them rests
on the same code-level, snapshot-independent argument in
`CAPABILITY_GAP_DIAGNOSIS.md`, not on having regenerated each one.

## Where "M7 consumer" and "five admitted obligations per snapshot" come from in the code

Confirmed directly from `crates/reviewgraphen-benchmark/src/prepare.rs`:

- "M7 consumer" is `prepare_real_b1`/`prepare_real_g3_proxy`/
  `prepare_real_full_review`, called from `prepare_real`/`prepare_real_full`
  — the same functions this benchmark family has always used to build
  `agent_input/` packets for `m7-real-v1`, `m7-pilot-v2`, and (via the new,
  oracle-free `prepare-head-local-unit` wrapper) `m7-head-local-v1`.
- "The frozen eight-ID mechanism ontology" is the `MECHANISM_ONTOLOGY`
  Rust constant, written into every packet's `mechanism-ontology.json` via
  `base_files`/`real_base_files` (`prepare.rs` lines ~1305-1335). This part
  genuinely is a frozen, hardcoded constant, confirmed byte-identical to
  the real artifact in this regeneration.
- **"Five admitted obligations per snapshot" is not a separate, hardcoded,
  per-snapshot substantive obligation file.** There is no code path in
  `prepare_real`/`prepare_real_full`/`prepare_real_b1`/
  `prepare_real_full_review` that reads a frozen obligations file per unit
  and substitutes it for synthesis. `input.obligations` is populated
  exclusively by `prepare_from_ingest_request` calling
  `MvpRulePack::synthesize(&ingested.program_space)` — the identical
  function, and identical resulting count (`MvpRulePack::rules()` always
  has exactly 5 entries, so the capability-gap fallback loop always
  produces exactly 5 obligations when none of the 5 rules' capabilities
  are satisfied), that this whole investigation has been tracing. This
  regeneration confirms it directly: no frozen-obligations file was read
  at any point in reconstructing snapshot-01's obligations from raw
  (redacted) source — they were computed fresh, and came out
  capability_gap, exactly as `MvpRulePack::synthesize` and `rust.rs`'s
  capability declarations, read in `CAPABILITY_GAP_DIAGNOSIS.md`, predict
  for any Rust input.

## Assessment of the REPORT.md passage

`benchmarks/m7-real-v1/results/full-reviewgraphen-replicate-2/REPORT.md`
states: "The ordinary `MvpRulePack` smoke input generated only
`reviewgraphen.capability_gap` obligations and therefore measured analyzer
capability gaps rather than code-defect detection. The M7 consumer
explicitly supplies the frozen eight-ID mechanism ontology, five admitted
obligations per snapshot, canonical Core context envelopes, and exact
selected excerpts. This is the missing explicit review profile."

Given the above: the "five admitted obligations per snapshot" the M7
consumer supplies **are the same `reviewgraphen.capability_gap`
obligations** the passage's own prior sentence describes as the ordinary
path's problem — not a distinct, substantive set. The sentence "This is
the missing explicit review profile" is not supported by what the
artifacts actually contain: mechanism ontology, context envelopes, and
excerpt selection are genuinely explicit and frozen, but the *obligations*
component of that "explicit review profile" is capability-gapped in
exactly the same way the passage says the ordinary path's is. Whether this
wording reflects the report's authors believing something different was
delivered, or a narrower intended claim (about packaging/framing rather
than obligation substance) that reads more broadly than intended, is not
something this investigation can determine — that is a question about
authorial intent, not an artifact fact, and is not asserted here either
way. What is asserted, on direct evidence, is that the artifact and the
most natural reading of the sentence do not match.

## Confirmed / not confirmed / speculative, summarized

**Confirmed by direct evidence in this investigation:**
- snapshot-01's two admitted production files' exact content (after the
  benchmark's own documented redaction), verified by hash match against
  the historical manifest.
- Regenerating obligations from that verified content through the same
  code path yields 5/5 `capability_gap`, matching every other packet
  checked across `m7-head-local-v1`, `m7-local-factorial-v2`, and
  `m7-pilot-v2`.
- `mechanism-ontology.json` is byte-identical to the real artifact.
- No hardcoded per-snapshot substantive obligation file exists in the
  relevant code path; "five admitted obligations" is
  `MvpRulePack::synthesize`'s output, not a frozen file.

**Not confirmed (explicitly, not glossed over):**
- Full 17-file byte-for-byte packet identity (11/17 hashes differ; most
  plausibly explained by tree-identity-dependent IDs, not by different
  obligation content — but not proven to be exactly that cause).
- Any of the other 39 m7-real-v1 snapshots individually.
- What the live Codex agent's `rg` command (found in
  `results/replicate-1/tool-commands.jsonl`, `trial_slug: snapshot-32-g3`,
  searching its own `obligations.json`/`program-space-facts.json` for
  `capability_gap` and the rule-origin names) actually returned — its raw
  transcript was not retained in this repository per
  `results/replicate-1/README.md`.

**Speculative, and explicitly not asserted as fact:**
- Whether `REPORT.md`'s authors already knew the five obligations were
  capability_gap-only when writing that passage.
