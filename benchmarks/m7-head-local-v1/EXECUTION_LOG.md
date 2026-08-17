# m7-head-local-v1 execution log

Status: recorded as execution proceeds. Design documents
(`preregistration.json`, `UNIT_SELECTION.md`, `JUDGE_PROTOCOL.md`,
`AUTH_AND_BUDGET_AMENDMENT.md`) are unchanged by anything in this log; this
is a record of what happened, not a new decision surface.

## Packet construction infrastructure

`prepare-real`/`prepare-real-full` (the existing CLI subcommands) both
require a `units_dir` bound to known-defect positive/control tree hashes
and presence evidence, which this experiment has none of. Their actual
scaffold-construction primitives (`prepare_from_ingest_request`,
`prepare_real_b1`, `prepare_real_full_review`) are oracle-free; a new
subcommand, `prepare-head-local-unit`, calls them directly on an
already-staged single-commit Git snapshot (`crates/reviewgraphen-benchmark/
src/main.rs`, committed `6c43676`). `scripts/build_packets.sh` stages one
such snapshot per unit (via `git show <pinned HEAD commit>:<path>`, never a
working-tree checkout of the real `fsl` repository) and calls it for all 9
units. Verified against the frozen v2 packet shape and mechanically
scanned for identity leakage before use (`SMOKE_TEST.md`-equivalent
verification performed inline during the build, not written up separately
since it reused the already-verified subcommand).

`scripts/run_trial.sh` and `scripts/run_batch.sh` mirror
`m7-local-factorial-v3`'s scripts exactly for the generation execution
condition (same profile file, same request shaper, same
`reasoning_effort` binding, same ADR 0037 extraction contract, same
candidate schema, same failure-class taxonomy), changed only in: the
admitted-input path allowlist (points at `/tmp/m7-head-local-v1-prepared/`
instead of the frozen v2 corpus), the omission of the `collect` scoring
step (no oracle exists for this experiment), and dropping the
`three_consecutive_invalid` auto-stop that `m7-local-factorial-v3`'s
`run_batch.sh` has — that stop condition is not among the operator's
stated stop conditions for this experiment (upstream 5xx, model crash, or a
non-responding generation request only), and an expected string of
`qwen_b1` reasoning-budget failures should not truncate the run before
later units are attempted.

## Packet size observation (flagged before generation started)

Built packets' total admitted input bytes, summed across every file in
each `agent_input/` directory:

| Unit | b1 bytes | full bytes | full / b1 |
| --- | ---: | ---: | ---: |
| head-local-00 | 85,599 | 173,959 | 2.03x |
| head-local-01 | 80,914 | 114,943 | 1.42x |
| head-local-02 | 114,370 | 263,435 | 2.30x |
| head-local-03 | 86,241 | 106,803 | 1.24x |
| head-local-04 | 81,700 | 180,251 | 2.21x |
| head-local-05 | 62,658 | 165,275 | 2.64x |
| head-local-06 | 103,013 | 237,299 | 2.30x |
| head-local-07 | 119,071 | 300,327 | 2.52x |
| head-local-08 | 118,109 | 306,075 | 2.59x |

`UNIT_SELECTION.md`'s 120,000-byte cap applied only to raw source bytes and
anticipated "headroom... for this experiment's own ReviewGraphen scaffold
overhead," but the actual full-arm scaffold overhead (multiple context
bundles per obligation) is larger than "modest" for several units: three
units (02, 07, 08) have full-arm admitted input exceeding 260,000 bytes,
roughly 2.8-3.2x the largest admitted input (94,358 bytes) that has ever
reached valid final content anywhere in this benchmark family
(`m7-local-factorial-v3`). This is reported here as a real, unmitigated risk
factor for the full arm's completion rate, not silently absorbed or fixed
by changing unit definitions after seeing it — unit membership, the source
byte cap, and the systematic sampling that produced these 9 units all
remain exactly as frozen in `units.json` and `UNIT_SELECTION.md`. All b1
packets remain within 120,000 bytes, consistent with the source cap (b1 has
no scaffold overhead).

## Generation trials

Recorded per unit as both arms complete, per the operator's requested
cadence.
