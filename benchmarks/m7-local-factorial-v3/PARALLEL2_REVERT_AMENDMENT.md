# Parallel-2 execution revert

Status: frozen before any further v3 stage1 provider request and after the
parallel-2 wave in `PARALLEL2_AMENDMENT.md` completed with zero semantic
results.

## Observed evidence

Both parallel-2 cells, run concurrently, are recorded in
`benchmarks/m7-local-factorial-v3/diagnostics/lm-studio-stage1-parallel2-1/`:

- snapshot-06 B1: 3,449.481 seconds elapsed, 32,876
  `response.reasoning_text.delta` events (132,892 UTF-8 payload bytes), 0
  output-text delta events.
- snapshot-06 full: 3,449.289 seconds elapsed, 0 reasoning delta events, 0
  output-text delta events.
- Both HTTP 200 SSE responses ended with the same message, retained verbatim
  in each trial's `adapter.stderr`: `stream disconnected before completion:
  The model has crashed without additional information. (Exit code: null)`.
  Codex CLI 0.147.0 exited 1 in both cases. The two elapsed times differ by
  0.192 seconds.

These figures are read directly from
`stage_1/snapshot-06/{b1_free_form,full_reviewgraphen}/elapsed-milliseconds`
and `generation-metrics.json` under
`/tmp/m7-local-factorial-v3-runs/v3-lm-studio-stage1-parallel2-1/` and their
repository copies; they are unchanged from the figures already frozen in
`PARALLEL2_AMENDMENT.md`.

## Interpretation

Parallel-2 produced zero semantic completions from two cells occupying the
server for approximately 57.5 minutes. Over that same window, one of the two
concurrently running cells (full) received no reasoning or output-text delta
at all — a complete starvation of that request while its concurrent peer (B1)
received the large majority of observed streamed content. The two crashes
occurred within 0.2 seconds of each other.

No throughput benefit from parallelism was observed: two cells run
concurrently for 57.5 minutes yielded fewer semantic completions (zero) than
a single prior sequential cell reaching final content in under 45 minutes
(`v3-lm-studio-stage1/stage_1/snapshot-06/b1_free_form`, 2,546.122 seconds,
recovered valid; `v3-lm-studio-stage1-continuation/.../full_reviewgraphen`,
2,658.716 seconds, schema-invalid but reached final content). Parallelism did
not shorten wall-clock time to a usable result in this observation; it
increased the compute lost per crash from one cell's worth to two cells'
worth, and one of the two cells contributed nothing before the shared
failure.

The near-simultaneous timing of the two crashes (0.192-second separation) is
consistent with, but does not establish, a shared server-side resource limit
triggered by concurrent load. Causation between parallelism and the crash is
**not** established from this single observation: the immediately preceding
sequential attempt under the same 3,600,000 ms idle-timeout condition also
ended in a server-side failure (`Model unloaded` after 9,536.248 seconds, in
`v3-lm-studio-stage1-timeout-3600000`). Both concurrency conditions have now
produced an unrecovered mid-stream server failure; this amendment does not
claim parallelism caused the parallel-2 crash, only that parallelism did not
demonstrate a benefit and coincided with a complete starvation of one cell.

## Decision

The v3 stage1 execution condition reverts to sequential (at most one provider
request in flight at a time), using the existing `scripts/run_batch.sh`
runner. `scripts/run_batch_parallel2.sh` and the
`X-ReviewGraphen-Trial-Key` parallel-routing mechanism in
`scripts/request_shaper.py` are retained unmodified in the repository for
audit; they are not invoked by the next stage1 attempt.

All other execution conditions are unchanged from
`LM_STUDIO_IDLE_TIMEOUT_AMENDMENT.md`: LM Studio behind cch, Codex CLI
0.147.0, `reasoning_effort=none` (not observed to suppress reasoning on this
backend), provider-default sampling, 262,144 context tokens, 65,536 output
tokens, 3,600,000 ms SSE idle timeout, and zero configured retries. The
candidate schema, ADR 0037 extraction contract, and the v3 4/4 semantic gate
are unchanged.

Parallel-2 results remain frozen as recorded in `PARALLEL2_AMENDMENT.md` and
are not pooled with the next sequential attempt.

## Failure-classification addition

`scripts/run_trial.sh` previously wrote `failure_class:
"upstream_server_stream_incomplete"` for any stream ending in `stream
disconnected before completion` or `stream closed before response.completed`,
without distinguishing a model-crash message from any other stream
interruption. Both parallel-2 `adapter.stderr` files, and no other frozen
`adapter.stderr` file, contain the text `The model has crashed`. The runner
now checks for that text (case-insensitive) within the same branch and writes
`failure_class: "upstream_model_crash"` instead when it matches; all other
stream-incomplete cases keep `upstream_server_stream_incomplete`. This is a
new, more specific subtype of the existing exclusion — not a new exclusion
category. It exits 71 in both cases, exactly as before: it consumes no
semantic attempt, and `scripts/run_batch.sh` stops the batch on either
subtype exactly as it did before this change. The v3 4/4 gate, the candidate
schema, and ADR 0037 extraction are untouched by this classification-only
change. It does not reclassify any already-frozen result; the two frozen
parallel-2 `generation-metrics.json` files already committed under
`benchmarks/m7-local-factorial-v3/diagnostics/lm-studio-stage1-parallel2-1/`
keep their originally recorded `failure_class:
"upstream_server_stream_incomplete"` unchanged, since editing frozen
diagnostic artifacts after the fact is excluded by the same discipline that
governs candidate normalization.
