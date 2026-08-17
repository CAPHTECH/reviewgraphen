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

### 3.3 Scale of the planned production run

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

### 3.4 Separation from the gate decision

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

## 4. Authoritative execution condition going forward

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
- v3 4/4 gate: unchanged, zero of four cells consumed under this condition
