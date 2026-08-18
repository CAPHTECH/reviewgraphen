# /tmp data loss and recovery (Plan B)

Status: operator-confirmed damage assessment, operator-directed Plan B
(recover what is recoverable, mark the rest `artifact_lost`, do not
regenerate). This document is the recovery procedure and its
verification, written before the pool composition changed.

## What happened

A PC restart cleared `/tmp`. `/tmp/m7-head-local-v1-runs` and
`/tmp/m7-head-local-v1-skill-prepared` (and every other
`m7-head-local-v1-*` path under `/tmp`) were destroyed. `POOL_SOURCE_MANIFEST.json`'s
7 qwen_skill entries all pointed into `/tmp/m7-head-local-v1-runs/...` —
verified, all 7 now `exists: False`. The 3 claude_skill entries pointed
into the repo's own `diagnostics/` tree and survived untouched.

## Root cause: an operational gap, not a model or upstream failure

Generation artifacts were left in `/tmp` and copied into the repo only
selectively (claude_skill's 3 units, the retry-1 units, the diagnostic
extracts used for reports) — not systematically, for every trial, as
each one completed. `head-local-03`, `head-local-05`, `head-local-06`
were never copied, because no report happened to need them individually
pulled out at the time. This is a supervision/procedure gap on the
operator's side, not a defect in ReviewGraphen, the generator, or the
upstream server, and not a new addition to the failure taxonomy that
describes generation or judging behavior. It is recorded here plainly,
per the operator's own instruction, as exactly that: an operational
failure to preserve artifacts, now corrected going forward (see
"Operational change" below).

## Recovery, per (unit, arm), each independently verified

### `head-local-04` / `qwen_skill` — unaffected

Already committed to `diagnostics/silent-truncation-retry-1/head-local-04-attempt1/candidate.json`
before the loss. SHA-256 `698b612989f4104bb31afc3d8ea1697b56fff716b14d89bf81eaaf911e14e2fd`,
verified to still match `POOL_SOURCE_MANIFEST.json`'s original record
exactly, byte for byte.

### `head-local-00/01/02` / `claude_skill` — unaffected

Already committed to `diagnostics/claude-skill-generation-head-local-00-01-02/`.
All 3 SHA-256 values verified unchanged against the original manifest.

### `head-local-00/01/02` / `qwen_skill` — recovered from the interim judge pass

The original `candidate.json` files (gen-2 for `head-local-00`, gen-3 for
`head-local-01`/`head-local-02`) are gone — no copy exists anywhere.
**Recovered from `diagnostics/interim-judge-head-local-00-01-02/<unit>/pooled-findings-shown-to-judge.json`**,
committed to git before the loss (the interim exploratory judge pass,
`qwen_skill`-only at the time, ran on exactly these 3 units before
`claude_skill` existed).

Two independent checks, both passing for all 3 units:

1. **Hash integrity of the recovered file itself.** Each
   `pooled-findings-shown-to-judge.json`'s current SHA-256 was compared
   against the SHA-256 that unit's own `judge-record.json` recorded as
   the actual `01-findings.json` admitted into that interim judge call
   (`input_files["01-findings.json"]`). All 3 match exactly — the file
   has not been altered since the interim call actually consumed it.
2. **`finding_id` self-consistency.** Every finding's `finding_id` was
   recomputed from its own `{unit_id, locations, mechanism_tags,
   rationale, severity}` using the same function
   (`scripts/build_final_judge_pool.py:finding_id`) that will build the
   new pool. All 13 findings (4 + 5 + 4) reproduce their own recorded
   `finding_id` exactly — 0 mismatches.

These findings are already in judge-pool form (normalized paths,
computed `finding_id`, no `local_id`/`packet_id`) rather than raw
`candidate.json` form — that is what the interim pass produced, and it
is exactly the content the new pool needs, so it is used directly, not
re-derived through an intermediate step that would add no verification
value.

**What this recovery does *not* prove:** it does not reproduce the
original `candidate.json`'s `trial_id`, `outcome`, or
`obligation_results` fields — those are gone. Only the `findings` array
content survives, which is the only part the judge pool ever needed.

### `head-local-03/05/06` / `qwen_skill` — `artifact_lost`, not recoverable

No copy exists anywhere: not in git, not in any diagnostic extract, not
in any report's raw-quoted content. **5 findings total** (1 + 1 + 3)
across these 3 units are unrecoverable. Not regenerated, per the
operator's Plan B decision — a fresh generation would not be the same
observation as the one that was lost, and pooling it in would silently
change what "the 9-unit run" means without saying so.

## Bias introduced by this loss — direction stated explicitly

Original `qwen_skill` per-unit finding counts (all 8 measured units):
`00:4, 01:5, 02:4, 03:1, 04:2, 05:1, 06:3, 07:0` — total 20.

Retained after loss: `00:4, 01:5, 02:4, 04:2` — total 15 (75% of the
original 20). Lost: `03:1, 05:1, 06:3` — total 5 (25%).

**The three lost units (1, 1, 3 findings) are exactly the three lowest
non-zero finding counts in the original 8-unit set** (only `07`'s 0 is
lower, and it was already excluded as `empty_pool` before the loss,
unaffected by it). **The retained qwen_skill sample is therefore
systematically skewed toward units where `qwen_skill` found more,** not
a random 4-of-8 subsample. Any aggregate rate computed from the 4
retained units (e.g. `issue_should_be_created` fraction, findings per
unit) should be read as a description of qwen_skill's denser units,
not as representative of its full original spread — this is a real,
disclosed sampling bias, not merely a smaller n.

## Operational change, effective immediately

Per operator instruction: **every future generation trial's artifacts
(`candidate.json`, raw SSE, `generation-metrics.json`, `process-status`)
are copied into the repo's `diagnostics/` tree as soon as that trial
completes** — never left solely in `/tmp` pending some later report that
might or might not need them individually. This loss happened because
that discipline was applied selectively (only when a report happened to
need a specific unit) instead of unconditionally, every trial, every
time. Applied starting with this pass's own judge-call outputs (see
`scripts/run_final_judge_pool.sh`, updated to copy each unit's result
into `diagnostics/final-judge-pool/results/` immediately after that
unit's call returns, not batched at the end).
