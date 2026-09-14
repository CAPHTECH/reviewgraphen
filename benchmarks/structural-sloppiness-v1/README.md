# Structural-sloppiness trial

This is a bounded, non-authoritative benchmark for the fixed
`changed_input_consumer_bridge_mismatch@1` predicate. It classifies accepted
ProgramSpace facts only: it does not execute target code, assert a source bug,
decide obligation applicability, or grant accepted/verified/sign-off authority.

The report is bound to the complete canonical ProgramSpace and to the exact
bytes of [`CONTRACT.md`](CONTRACT.md). The checked-in
[`example-missing-bridge-report.json`](example-missing-bridge-report.json) is a
generated report for the synthetic fixture, not a claim about a real source
repository.

```text
cargo run --locked -p reviewgraphen-benchmark \
  --bin reviewgraphen-structural-sloppiness -- \
  analyze --input PROGRAM_SPACE.json --output FRESH_REPORT.json

cargo run --locked -p reviewgraphen-benchmark \
  --bin reviewgraphen-structural-sloppiness -- \
  validate --input PROGRAM_SPACE.json --report REPORT.json

cargo run --locked -p reviewgraphen-benchmark \
  --bin reviewgraphen-structural-sloppiness -- \
  flat --input PROGRAM_SPACE.json --output FRESH_FLAT.json
```

`analyze` success means the analysis ran; it never means clean, safe, or
signed-off. Outputs are create-new only and are refused when the destination
already exists. `flat` retains artifact/capability/limitation inventory and
relation kind/provenance counts while erasing relation-record endpoints and
IDs; opaque capability or limitation references may still name relation IDs.
It is an information-loss ablation, not a competing static-analysis baseline
or a proof about a HigherGraphen library.

Parent-observed historical calibration has three real eligible gates whose
named predicate changed across the old/fixed/current producer wiring. In these
captures all three subjects have `async`, `awaits`, and `spawns` false and an empty
`concurrency_primitives`; no captured public free function meets the local
concurrency hints. The trial therefore reports a named predicate mismatch,
not three lost applicable obligations.

## Observed result (2026-09-13)

The executable replay completed with exit 0. Retained
[compact projections](results/summary.json) preserve observed gates, candidate
claims, input/report hashes and declared loss. The
[capture manifest](results/capture-manifest.json) pins producer revisions,
target diffs, tool versions and the source-identical exporter.

| Input | Eligible public functions | Candidate gate mismatches |
| --- | ---: | ---: |
| First historical diff | 0 | 0 — **not exercised**, not clean |
| Old producer `271c932`, selected real diff | 3 | 3 |
| Exact repair `7b40d48`, same real diff | 3 | 0 |
| Current producer `c074761`, same real diff | 3 | 0 |
| Current graph, one synthetic endpoint rewire | 3 | 1 |

The three old observations share **one known producer/consumer wiring defect**;
they are not three independently discovered bugs. The current and rewired flat
projections are byte-identical. This demonstrates a case where endpoint
information matters, not superiority over a competent static analyzer.

結論: 接続不整合を決定論的に再検出する転用は動いた。ただし、今回のルールは
既知の修正を読んでから作っている。未知の「雑さ」を見つける有効性、AIと人間の
品質差、HigherGraphen固有の高次構造の優位性は、この試行では検証していない。
通常の関係クエリでも同じ条件を表現できる。再利用できたのは、主にProgramSpace
の事実抽出・入力拘束・来歴と解析不足の保持である。

## Reproduce the capture and replay

Build the experimental binary with `cargo build --locked -p
reviewgraphen-benchmark --bin reviewgraphen-structural-sloppiness`.

For each producer revision in the capture manifest, use a **fresh separate Git
clone**, check out that exact commit, and place the unchanged
[`harness/slop_export.rs`](harness/slop_export.rs) at
`crates/reviewgraphen-ingest/examples/slop_export.rs` in that clone. The exporter
was compiled and run at all three producer revisions without modifying the
producer or consumer. Run from the producer clone:

```text
cargo run --locked -p reviewgraphen-ingest --example slop_export -- \
  /absolute/path/to/higher-graphen BASE_REVISION TARGET_REVISION FRESH_PROGRAM_JSON
```

The actual target revisions and raw hashes are in the manifest. The old producer
has two captures: the first nonexercising diff, then the selected positive diff.
Fixed and current producers use the selected diff. Cargo admission for the
**target repository** is disabled; the exporter builds ReviewGraphen, not target
code. Do not run the checkout step in a dirty working tree.

Then run the tested [`replay.sh`](replay.sh) with the four captured files:

```text
bash benchmarks/structural-sloppiness-v1/replay.sh \
  /absolute/path/to/reviewgraphen-structural-sloppiness \
  OLD_ZERO_PROGRAM_JSON OLD_POSITIVE_PROGRAM_JSON \
  FIXED_PROGRAM_JSON CURRENT_PROGRAM_JSON FRESH_OUTPUT_DIRECTORY
```

The replay validates all reports, compares the exact eligible subject IDs,
constructs a synthetic rewire while preserving endpoint-order validity,
compares flat bytes and rejects a stale report (expected exit 4). It keeps raw
outputs in the fresh directory and emits `summary.json`. The script trusts the
operator's producer-role assignment; it does not infer the producer revision
from ProgramSpace data. Check that assignment against the capture manifest.

Full inputs are about 38 MB each and full reports about 5.6 MB each, mostly
limitation IDs. They are not duplicated in this repository. A compact projection
is **not** an `AnalysisReport` and cannot substitute for `validate` on the full
report. Limitation counts are never defect counts.

## Verification boundary

The original 15 acceptance tests passed. Replacing the real detector's OR with AND made
the exact-branches test fail (exit 101); restoring it restored 15/15 passing.
The real replay passed, authority tampering and stale input were rejected with
exit 4, and a copied-attribute non-change artifact was excluded.
See [WORK.md](WORK.md) for package checks, full-workspace gate status and review.

The final focused suite has 18 tests. Added ID-collision, candidate-ordering and
closed-report deserialization tests each failed under their corresponding
defect/mutation and passed after restoration. The actual CLI rejected an extra
top-level report field with exit 3. Two minor limits remain: replay case roles
require manual manifest comparison, and not every change-shape/extraction-state
branch has its own mutation-sensitive test. Those branches are not evidence of
broader empirical coverage than the added-function calibration above.

Final verification (2026-09-14): the isolated checkout's complete
`scripts/ci.sh fast` passed with `CARGO_PROFILE_TEST_OPT_LEVEL=1` and
`NEXTEST_TEST_THREADS=1`: 1,139 tests passed, two existing configuration skips,
and all 71 documentation tests passed. Assertions and the 120-second timeout
were unchanged. The default unoptimized run had timed out; this is not a claim
that it passes. Root integration passed the 18 focused tests and bundle checks;
the user's pre-existing runtime formatting change was preserved, not repaired.
