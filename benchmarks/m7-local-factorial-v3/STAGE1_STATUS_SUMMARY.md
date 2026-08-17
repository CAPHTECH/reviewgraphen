# v3 stage1 status summary

Status: frozen before the sequential-revert stage1 attempt and after
`PARALLEL2_REVERT_AMENDMENT.md`. This document consolidates the per-event
records already frozen elsewhere in this directory. It changes no prior
record; every figure below is traceable to the cited file or raw artifact.
Where this document computes a rate or aggregate across attempts run under
different execution conditions, that is stated explicitly as a descriptive
cross-condition figure, not a gate statistic — the v3 4/4 gate is evaluated
only within one uniform, currently-authoritative execution condition, per
the non-pooling rule already stated in every amendment below.

## 1. Chronological attempt list

### 1.1 Non-semantic attempts (consumed no v3 semantic attempt)

| Order | Attempt | What happened | Record |
| --- | --- | --- | --- |
| 1 | `v3-stage1` | Wrapper omitted `reasoning_effort=none`; issued a request but is not a v3 condition. | `STAGE1_INFRASTRUCTURE_FAILURE.md` |
| 2 | `v3-stage1-r1` | Corrected wrapper; experimenter SIGINT after 282,318 ms (status 130). | `STAGE1_INFRASTRUCTURE_FAILURE.md` |
| 3 | `v3-stage1-r2` | Ollama backend; chat request unresponsive for 79+ minutes, then HTTP 502 after 28,951,818 ms (8h02m). | `STAGE1_INFRASTRUCTURE_FAILURE.md` |
| 4 | `lm-studio-reasoning-none-probe-1` | Backend transitioned Ollama→LM Studio. Minimal probe: `reasoning_effort=none` did **not** suppress reasoning (34 reasoning delta events), but a nonempty final `OK` was produced in 29.460 s. | `LM_STUDIO_TRANSITION.md` |
| 5 | `v3-lm-studio-stage1`, snapshot-06 full (first pass) | Runner plumbing bug (extractor called with 2 args); interrupted by experimenter after 40,497 ms (status 130) once the bug on the B1 cell was noticed. | `LM_STUDIO_STAGE1_PROGRESS.md` |
| 6 | `v3-lm-studio-stage1-recovery-1`, snapshot-06 B1 | Sequential restart under the 60-minute idle timeout; experimenter SIGINT after 305,555 ms (status 130) once 2-way server parallelism became available. One partial SSE (6,136 reasoning deltas, no output text) retained. | `LM_STUDIO_STAGE1_PROGRESS.md` |

### 1.2 Semantic-condition attempts (provider requests that ran to natural conclusion)

All are LM Studio, `qwen3.8:27b-mlx`, Codex CLI 0.147.0, provider-default
sampling, 262,144 context tokens, 65,536 output tokens, zero retries.
"Timeout" is the configured SSE idle-timeout; "Concurrency" is how many
provider requests could be in flight at once under that attempt.

| # | Attempt / cell | Timeout | Concurrency | Elapsed (s) | Outcome (`failure_class` or gate result) | Reached final content |
| --- | --- | --- | --- | --- | --- | --- |
| A | `v3-lm-studio-stage1`, snapshot-06 B1 (extraction re-run offline in `-offline-recovery` after the plumbing fix; no second model call) | 300,000 ms | 1 | 2,546.122 | **valid** — `sha256:09cace30767b0d9fe42a95fb71935d5ed7f718be8810e3e2b50f5d0c3b374b17` | yes |
| B | `v3-lm-studio-stage1-continuation`, snapshot-06 full | 300,000 ms | 1 | 2,658.716 | `candidate_schema_invalid` (model wrote `reviewgraphen.bandidate_output.v1`) | yes |
| C | `v3-lm-studio-stage1-continuation`, snapshot-34 B1 | 300,000 ms | 1 | 300.305 | `client_idle_timeout` (excluded) | no |
| D | `v3-lm-studio-stage1-timeout-3600000`, snapshot-06 B1 | 3,600,000 ms | 1 | 9,536.248 | `upstream_server_stream_incomplete` (`Model unloaded`, excluded) | no |
| E | `v3-lm-studio-stage1-parallel2-1`, snapshot-06 B1 | 3,600,000 ms | 2 | 3,449.481 | `upstream_model_crash` subtype (excluded) — 32,876 reasoning deltas, 0 output-text deltas | no |
| F | `v3-lm-studio-stage1-parallel2-1`, snapshot-06 full | 3,600,000 ms | 2 | 3,449.289 | `upstream_model_crash` subtype (excluded) — 0 reasoning deltas, 0 output-text deltas | no |

Row E/F `failure_class` in the already-committed `generation-metrics.json` is
the pre-classification-change value `upstream_server_stream_incomplete`; see
`PARALLEL2_REVERT_AMENDMENT.md` for why frozen artifacts were not retroactively
edited when the more specific `upstream_model_crash` subtype was added.

snapshot-34 full has never been issued under any condition.

## 2. v3 4/4 gate: current tally

Rows A and B are the only two semantic completions recorded anywhere in v3.
Both ran under the **300,000 ms idle-timeout condition**, which
`LM_STUDIO_IDLE_TIMEOUT_AMENDMENT.md` superseded before any further request;
that amendment states explicitly that replacement-condition results "must not
be pooled with the pre-amendment stage1." Rows A and B are therefore **not**
counted toward the gate under the current (3,600,000 ms) condition.

Under the current condition (3,600,000 ms idle timeout), before this
document's sequential revert: 3 natural-conclusion requests (D, E, F), 0
valid, 0 schema-invalid, 3 excluded server-side failures. **Zero of four
gate cells have been consumed under the currently authoritative execution
condition.** The next attempt starts all four cells from the beginning, as
every prior non-pooled restart in this experiment has done.

## 3. Operational feasibility (independent of the gate)

This section answers a different question from the gate: not "did the model
output a valid candidate," but "can this execution condition complete enough
requests to run the planned experiment at all." The user has stated that
elapsed time is not a constraint; completion rate is the relevant question.

### 3.1 Duration distribution (natural-conclusion requests only, n=6)

Sorted ascending, from rows C–F and A–B above, all measured from
`elapsed-milliseconds`:

- 300.305 s (5.0 min) — client idle timeout, no streamed content reached
  final text
- 2,546.122 s (42.4 min) — reached final content, valid
- 2,658.716 s (44.3 min) — reached final content, schema-invalid
- 3,449.289 s (57.5 min) — model crash, no final content
- 3,449.481 s (57.5 min) — model crash, no final content
- 9,536.248 s (158.9 min) — model unloaded, no final content

Both requests that ever reached final content did so in under 45 minutes.
The two longest-running requests (57.5 min and 158.9 min) both failed without
final content. There is no measured case of a request exceeding roughly 45
minutes and still reaching final content; this is descriptive of six
observations, not a demonstrated ceiling.

### 3.2 Completion rate

Three nested denominators, all read from raw artifacts (see §1.2):

- **All LM Studio stage1 requests ever issued, any condition (n=8,
  excludes the two experimenter-interrupted requests as censored/unknown
  rather than failed):** 2 of 6 natural-conclusion requests reached final
  content = **33.3%**. This figure spans two different idle-timeout
  conditions and two different concurrency conditions and must not be used
  as a gate statistic; it is reported only as cross-condition context.
- **Requests issued under the current 3,600,000 ms idle-timeout condition,
  any concurrency (n=4 issued, 1 experimenter-interrupted and censored,
  3 natural-conclusion):** 0 of 3 natural-conclusion requests reached final
  content = **0%**.
- **Requests issued under the current idle-timeout condition restricted to
  the concurrency=1 condition being restored by this document (n=2 issued,
  1 experimenter-interrupted and censored, 1 natural-conclusion):** 0 of 1
  natural-conclusion request reached final content. This single-observation
  rate is not statistically meaningful on its own; it is reported for
  completeness, not as an estimate.

None of these are precise rate estimates; n is small throughout. The
directionally consistent fact across all three denominators is that no
request under the current (post-idle-timeout-amendment) execution condition
has yet reached final content, regardless of concurrency.

### 3.3 Duration-correlated failure risk (unconfirmed)

Re-reading §3.1 by outcome: the two requests that reached final content ran
2,546.122 s and 2,658.716 s (both under 45 minutes); the two requests that
ended in an excluded server-side failure without final content ran 3,449.289
s / 3,449.481 s (57.5 min) and 9,536.248 s (158.9 min). Every request under
45 minutes reached final content; every request over 45 minutes did not.

This is an observation, not a finding. With n=6 and no controlled
manipulation of duration, no correlation between elapsed time and failure is
asserted here, and confounds are not excluded — snapshot-06 full and B1
differ in input size and arm content, and the two long-running failures
occurred under different execution conditions (300,000 ms and 3,600,000 ms
idle timeout, sequential and parallel concurrency) from each other and from
the two successes.

The risk this observation raises, if a duration-dependent failure mode is
real, is recorded here because of its consequence for the experiment rather
than its current evidentiary strength: if longer-running generations fail
systematically more often than shorter ones, then units that provoke longer
reasoning — plausibly the harder detection targets — would be dropped from
the recorded sample at a higher rate than easier units. Because
`no_semantic_retry: true` is preregistered, such trials are not retried and
recovered; they are simply excluded. A detection-capability measurement
built from a sample that has silently lost its harder cases would be biased
toward the easier end of the difficulty distribution, independent of
whatever the model's true detection capability is on the full unit set.

No mitigation is adopted from this observation alone — the 4/4 gate, the
current execution condition, and the no-retry policy are all unchanged. The
concrete action is measurement: every subsequent trial, in stage1 and in any
later production run, must have its elapsed time and final-content outcome
recorded (already implied by `generation-metrics.json`), so this question can
be re-evaluated with a larger n once more trials complete. If a real
duration-dependent failure pattern later becomes evident, addressing it would
require a new amendment decided before further results are seen, per the
same discipline applied to this experiment throughout.

### 3.4 Scale of the planned production run

The preregistered production run (`preregistration.json`) is 20 positive
units × 2 arms = 40 semantic trials, each under the same no-retry policy as
stage1. If the current-condition natural-conclusion completion rate (0/3
observed so far) or even the better cross-condition rate (33.3%, spanning a
now-superseded timeout condition) persisted at that scale, a large fraction
of the 40 trials would end as excluded server-side failures rather than
scored semantic outcomes, and — because `no_semantic_retry: true` is already
preregistered — those trials would not be recovered by retrying. This is an
inference from the observed rates, not a measured production-run result; no
production trial has been run.

### 3.5 Separation from the gate decision

Passing the 4/4 stage1 gate establishes only that the candidate schema and
ADR 0037 extraction are reachable under this execution condition on four
control cells. It does not establish that the execution condition can
sustain 40 trials without a prohibitive loss rate to excluded server-side
failures. If stage1 passes 4/4, whether to proceed to the 40-trial
production run is a **separate decision** that should weigh the completion
rate in §3.2, not an automatic consequence of the gate passing. This
document takes no position on that separate decision; it exists so the
decision can be made from the recorded rate rather than from the gate result
alone.

## 3.6 Sequential-restart plumbing defect (zero semantic attempts)

The first sequential-restart attempt after this document's revert,
`v3-lm-studio-stage1-sequential-1`, failed before any provider request. The
parallel-2 preregistration (`5958ac2`) had made `run_trial.sh` require
`M7_V3_TRIAL_KEY` unconditionally for every invocation, but
`scripts/run_batch.sh` (the sequential runner, unchanged since before
parallel-2) never set it. All three attempted cells failed at
`run_trial.sh`'s parameter check before `mkdir -p -- "$result_dir"`, before
the shaper health check, and before any HTTP request; `run_batch.sh`'s own
`three_consecutive_invalid` safety stop then ended the batch after cell 3.
Verified directly:
`request-shaper-stage_1-0.jsonl` is 0 bytes, the response capture directory
is empty, and no `request_shaper.py` process was left running. This consumed
zero semantic attempts and issued zero provider requests; it is the same
class of defect as the extractor-argument-count bug in
`LM_STUDIO_STAGE1_PROGRESS.md`'s "Runner plumbing interruption," not a model
or server outcome.

`scripts/run_batch.sh` now exports `M7_V3_TRIAL_KEY="${attempt}-${mode}-${start}"`
before its trial loop, matching the pattern `run_trial.sh` requires and the
constant-key precedent already used for sequential execution. This is a
plumbing fix, not an execution-condition change: it does not touch the
model, reasoning effort, sampling, context/output limits, idle timeout,
retry count, candidate schema, or ADR 0037 extraction. The next attempt uses
a new label, `v3-lm-studio-stage1-sequential-2`, to avoid colliding with the
already-created (and harmless, zero-content) health-check directory left by
`-sequential-1`.

## 3.7 Launch-time reasoning-effort misconfiguration (caught, zero attempts consumed)

The second sequential-restart launch,
`v3-lm-studio-stage1-sequential-2`, was started by invoking
`scripts/run_batch.sh` directly without exporting
`M7_V3_REASONING_EFFORT=none` first. `scripts/run_batch.sh` does not set
this variable itself and never has; every prior successful v3 stage1
invocation set it in a small `/tmp` wrapper script that exports it before
calling `run_batch.sh` (for example, the already-used
`/tmp/m7-local-factorial-v3-stage1-recovery.sh`). Without it,
`run_trial.sh`'s `reasoning_effort=${M7_V3_REASONING_EFFORT:-${3:-high}}`
fell back to `high`, which is not the preregistered condition.

This was caught by inspecting the live process command line
(`-c model_reasoning_effort='high'` visible under the spawned `codex`
process) approximately 15-20 seconds after launch, before any data reached
the request shaper. The process tree was killed immediately. Verified after
the kill: `request-shaper-stage_1-0.jsonl` is 0 bytes, the sole partial
response capture file is 0 bytes, and no `process-status` was ever written
for the cell. This consumed zero semantic attempts under any reasoning
condition. Whether the upstream server itself received and briefly began
processing a `reasoning_effort=high` request in that ~20-second window before
the kill reached it is not established either way; no output was produced or
captured regardless. The next attempt,
`v3-lm-studio-stage1-sequential-3`, uses the same `/tmp` wrapper pattern as
prior attempts, exporting both `OLLAMA_PRIV_API_KEY` (a non-secret constant
required only to satisfy the Codex profile's `env_key`, per the server
administrator) and `M7_V3_REASONING_EFFORT=none` before invoking
`run_batch.sh`.

## 3.8 Cell-2 transport-attribution bug (discovered after sequential-3 cells 1-2 completed)

§3.6 above stated that a single `M7_V3_TRIAL_KEY` constant for the whole
attempt (`"${attempt}-${mode}-${start}"`) was a safe "constant-key precedent
already used for sequential execution." That claim was wrong and is
corrected here rather than edited away.

After cell 2 (snapshot-06 full) completed, its `transport-record.json` and
derived `generation-metrics.json` were found to contain **cell 1's** transport
data (`raw_response_artifact: "response-000001.sse.gz"`,
`provider_output_tokens: 65535`, `reasoning_delta_events: 65289` — identical
to cell 1's own recorded values), not cell 2's own. Root cause: `run_trial.sh`
waits only for `request_count >= 1` matching rows for its trial key before
reading `transport-record.jsonl`; with one constant key shared across the
whole attempt, cell 2's very first poll already found cell 1's row a match
and could read `transport-record.jsonl` before cell 2's own row had been
appended to the shaper's master log, then take the (at that instant) only
matching row via `tail -n1` — cell 1's stale row. This is a race, not a
deterministic failure, which is why cell 1 (the first cell, with no prior
row to race against) was unaffected and its own transport-record.jsonl
correctly held exactly its own one row.

**Scope of the bug**: it corrupts only the transport-derived telemetry fields
in cell 2's `generation-metrics.json` (`provider_*_tokens`,
`*_delta_events`, `*_delta_utf8_bytes`, `cached_input_tokens`,
`token_attribution`). It does **not** affect `candidate.json`,
`candidate-validation`, `collection.json`, `elapsed_milliseconds`, or the
`valid` outcome for cell 2 — those are read from cell 2's own
`process-record.json` and process-output files, produced by cell 2's own
`bwrap`/codex invocation, independent of the shaper's JSONL bookkeeping.
The schema-gate result for cell 2 is unaffected; only its descriptive
telemetry was wrong.

**Corrected cell 2 transport values**, read directly from
`request-shaper-stage_1-0.jsonl` row 2 (`request_sequence: 2`,
`raw_response_artifact: "response-000002.sse.gz"`, verified against
`raw_response_compressed_sha256:
sha256:e117f158f9fb2bf230ab17f8b4cd815e611dd375bd741e62be96ee6e7de49b8a`):

| Field | Cell 2 corrected value |
| --- | --- |
| `elapsed_seconds` | 805.675 |
| `provider_input_tokens` | 39,647 |
| `cached_input_tokens` | 6,144 |
| `provider_output_tokens` | 15,159 |
| `provider_reported_reasoning_tokens` | 14,706 |
| `provider_non_reasoning_output_tokens` | 453 |
| `reasoning_delta_events` | 14,569 |
| `reasoning_delta_utf8_bytes` | 57,133 |
| `output_text_delta_events` | 453 |
| `output_text_delta_utf8_bytes` | 955 |
| `token_attribution` | `ambiguous_mixed_or_absent_content_events` |

The already-committed per-cell `generation-metrics.json` and
`transport-record.json` files for cell 2 are **not** retroactively edited in
place — they are an authentic record of what the script actually produced,
bug included. This table is the correction of record; any future reader
comparing per-cell diagnostics against this document must use this table,
not the cell's own `generation-metrics.json`, for cell 2's transport
telemetry.

**Fix applied**: `scripts/run_batch.sh` now exports a per-trial-unique
`M7_V3_TRIAL_KEY="${attempt}-${mode}-${index}"` inside the trial loop
(previously a single constant set once before the loop), so no cell can ever
match an earlier cell's stale row. This restores, for sequential execution,
the same per-trial-uniqueness invariant that `PARALLEL2_AMENDMENT.md`
required for concurrent execution — the invariant this document's §3.6 fix
incorrectly relaxed to a constant. This fix is plumbing only: it does not
touch the model, reasoning effort, sampling, context/output limits, idle
timeout, retry count, candidate schema, or ADR 0037 extraction.

## 4. Sequential-3 cells 3-4 stopped and v3 final result

The user redirected the overall benchmark program (see conversation record)
from re-detecting known frozen-oracle targets to searching for previously
undiscovered defects in `fsl` HEAD, under a new experiment
`m7-head-local-v1`. This section closes out v3 stage1 under its own frozen
rules before that pivot; it does not itself change any v3 rule.

### 4.1 Cell 3 (snapshot-34 B1): operator-terminated

Cell 3 started at 2026-08-17T07:17:13Z (per `run_batch.sh`'s `START` line)
and was terminated by the operator at approximately 2026-08-17T07:27:53Z–
07:28:00Z UTC, roughly 10-11 minutes after starting, because (a) the 4/4
gate was already mathematically unreachable once cell 1 recorded an invalid
outcome (§4.2), making the marginal value of completing cells 3-4 low, and
(b) the user redirected the experiment. This is **not** a time-based stop:
cell 1 alone had already run 5,146 seconds (85.8 minutes) without being
treated as anomalous, and the stop reason here is the gate arithmetic and
the redirect, not elapsed time.

Verified before and after termination: `response-000003.sse.partial` is 0
bytes both before and after the kill, `request-shaper-stage_1-0.jsonl`
contains no row for this cell, and no `process-status` file was ever written
for `stage_1/snapshot-34/b1_free_form`. The process tree (`run_trial.sh`,
`reviewgraphen-benchmark`, both `bwrap` sandboxes) was confirmed fully
terminated and the request shaper confirmed down (`healthz` connection
refused) before this document was written. Cell 3 consumed **zero semantic
attempts** — it is `operator_terminated`, a new outcome distinct from every
excluded server-side or client-side category already defined, because the
stop originated from the operator/user, not from the provider or transport.

Cell 4 (snapshot-34 full) was never issued.

### 4.2 Final v3 4/4 gate tally under the current execution condition

| Cell | Outcome | Elapsed | Counts toward gate? |
| --- | --- | --- | --- |
| snapshot-06 B1 | `empty_final_after_process_completion` (reasoning_runaway: reasoning consumed 65,535 of 65,536 output tokens, zero final content) | 5,146.477 s | yes — **invalid** |
| snapshot-06 full | `valid`, candidate hash `sha256:dd2c2215dbc6d4f50c02fbe1ce9979facd50cb3b078b782031ae6317c46bcd47`, 0 findings (correct abstention on a control unit — see §4.4) | 805.675 s | yes — **valid** |
| snapshot-34 B1 | `operator_terminated` | ~10-11 min in flight | no — excluded |
| snapshot-34 full | never issued | — | no |

**v3 4/4 gate: FAILED.** 1 valid, 1 invalid, 2 not attempted. The gate was
already mathematically unreachable after cell 1 (a maximum of 3 of 4 cells
could ever be valid once cell 1 recorded invalid), independent of the later
decision to stop cells 3-4. This is the frozen final result for
`m7-local-factorial-v3` stage1 under its currently-authoritative execution
condition (§5). It is not pooled with the earlier, superseded 300,000 ms
idle-timeout condition's 1-valid/1-invalid result (§2), which remains a
separate, non-pooled observation.

### 4.3 Why the gate failed: cause characterization

Across every natural-conclusion provider request issued anywhere in v3
stage1 (all conditions, n=8; see §1.2 and this section), exactly three
requests ever produced non-empty final content: the original pre-amendment
snapshot-06 B1 (valid), the pre-amendment snapshot-06 full (schema-invalid —
the model itself wrote `reviewgraphen.bandidate_output.v1`), and this
attempt's snapshot-06 full (valid). Of those three, two passed the unchanged
candidate schema and one failed on a self-inflicted spelling error — a 2-of-3
pass rate when the model does emit content. The other five natural-conclusion
requests never reached final content at all: one client idle timeout, one
`Model unloaded`, two `upstream_model_crash`, and this attempt's cell 1
(reasoning consumed the entire output budget). None of those five was a
candidate-schema rejection.

This supports characterizing the v3 4/4 gate failure as driven primarily by
**failure to reach final content** — upstream/transport failures beyond this
experiment's control, plus at least one clear case of reasoning consuming
the entire output budget before any candidate text — rather than by the
model's inability to conform to the candidate schema once it does produce
content. This is a description of the observed pattern across a small n (3
non-empty-final requests, 5 empty-final requests), not a statistical claim
about the model's true rate of either behavior.

### 4.4 Arm asymmetry: an unconfirmed hypothesis carried forward

Within this attempt's own two completed cells, arm and outcome coincide with
a large gap:

| Arm | Elapsed | Provider output tokens (of 65,536 max) | Reasoning share | Final content | Outcome |
| --- | --- | --- | --- | --- | --- |
| B1 (no scaffold) | 5,146.477 s (85.8 min) | 65,535 (99.998%) | 65,535 reasoning / 0 non-reasoning | 0 bytes | invalid (reasoning_runaway) |
| full (ReviewGraphen scaffold) | 805.675 s (13.4 min) | 15,159 (23.1%) | 14,706 reasoning / 453 non-reasoning | 955 bytes | valid |

Elapsed time differs by a factor of ~6.4×; output-token consumption differs
by a factor of ~4.3×. This is the opposite direction from a prior
(now-withdrawn) prediction in this experiment's history that the
scaffolded arm's larger input would drive longer reasoning and more
runaway risk; observed here, the unscaffolded arm ran longer and ran away,
the scaffolded arm converged quickly and passed.

**No causal claim is made.** This is one paired observation (n=1 per arm)
from two different snapshots' B1 and full cells being compared across two
different underlying inputs (snapshot-06 B1's input differs from
snapshot-06 full's input in content and byte size, though both are the same
snapshot/unit), under a schema-probe design not built to test this
question, with cell 1's own duration itself confounded with its failure (a
request that runs to the output cap necessarily also runs long). Confounds
are not excluded. The hypothesis is carried forward explicitly for
`m7-head-local-v1` to examine with more units per arm: **does the
ReviewGraphen scaffold reduce reasoning-runaway risk and shorten
time-to-completion relative to unscaffolded B1, on this model and backend?**
If the same direction recurs across more units, it becomes evidence
relevant to the user's second research question (does ReviewGraphen with a
local LLM help); if it does not recur, the single observation here was
noise.

## 5. Authoritative execution condition going forward

Per `LM_STUDIO_IDLE_TIMEOUT_AMENDMENT.md` and `PARALLEL2_REVERT_AMENDMENT.md`,
combined:

- Backend: LM Studio behind cch, endpoint `http://192.168.68.71:11999` only
- Model: `qwen3.8:27b-mlx`
- Client: Codex CLI 0.147.0 (adapter `codex-exec`, invoked as a subprocess by
  this Bash session — not the orchestrating Claude Code session itself)
- `reasoning_effort=none` (does not suppress reasoning on this backend;
  retained as preregistered)
- Sampling: provider default, unmodified
- Context: 262,144 tokens; output: 65,536 tokens
- SSE idle timeout: 3,600,000 ms
- Retries: zero, no automatic or operator retry on any failure
- Concurrency: 1 (sequential), reverted by `PARALLEL2_REVERT_AMENDMENT.md`
- Candidate schema and ADR 0037 extraction: unchanged
- v3 4/4 gate: **closed, failed** (§4.2) — 1 valid, 1 invalid, 2 not
  attempted (1 operator-terminated, 1 never issued). `m7-local-factorial-v3`
  stage1 does not proceed to positive trials, per its own preregistered
  `otherwise: stop_before_positive_trials` rule. This execution condition
  (backend, model, reasoning effort, sampling, context/output limits, idle
  timeout, retry policy, concurrency=1) remains the reference condition
  carried into `m7-head-local-v1`'s own separate preregistration.
