# Output-cap causal proof, new failure classification, restart protocol, upstream failure history

Status: recorded after server-recovery confirmation (operator's own minimal
request, 1.63 s, not a retry), before resuming generation. Amends how
restarts after excluded upstream failures are handled; does not change any
generation execution condition value.

## 1. Causal proof that the old 65,536-token cap was the actual bottleneck

**This is measured, not inferred.** `head-local-00` ran under both
conditions on the identical unit:

| Condition | Outcome | `provider_output_tokens` | Cap |
| --- | --- | --- | --- |
| old (65,536) | `empty_final_after_process_completion`-class wall risk (this exact unit did complete at 64,421/65,536 under the old cap in the very first `qwen_skill` attempt, `OUTPUT_CAP_STOP_AND_PROBE.md`) | 64,421 | 65,536 |
| new (131,072) | `valid` | 72,863 | 131,072 |

The same unit's actual completion, under a cap that did not constrain it,
required **72,863 tokens — 11.1% more than the old 65,536-token cap could
ever have supplied.** Since `head-local-01` (a different unit) failed at
exactly 65,535/65,536 under the old cap, and `head-local-00` is now proven
to need more than 65,536 to reach its own natural stopping point, the old
cap is directly shown to have been an active constraint on this task, not
merely a conservative-but-sufficient value. This is a measured causal
result, not a projection from token-count reasoning alone.

## 2. New failure classification: `upstream_stream_closed_before_completion`

`head-local-01`'s failure under the new cap is **not** a reasoning-budget
failure and must not be classified or discussed as one. Verified directly
from `generation-metrics.json` and `adapter.stderr`:

- `reasoning_delta_events: 0`, `output_text_delta_events: 0`,
  `provider_output_tokens: null` — no generation of any kind was ever
  observed for this request.
- `elapsed_seconds: 1436.126` (23.9 min) of silence, then disconnect.
- `adapter.stderr`: `stream disconnected before completion: stream closed
  before response.completed` — Codex's own generic fallback message,
  used when the upstream server closed the connection without supplying
  any specific reason (contrast with `Model unloaded.` or `The model has
  crashed...`, both real server-supplied messages already seen in this
  program).

`scripts/run_trial.sh` now distinguishes this case: when the stream-
incomplete regex matches, `model has crashed` still maps to
`upstream_model_crash`; the literal Codex fallback text `stream closed
before response.completed`, **without** `Model unloaded` also present,
now maps to the new `upstream_stream_closed_before_completion`; every
other case keeps the existing generic `upstream_server_stream_incomplete`.
This is a classification-only change — `exit 71`, exclusion from the
semantic denominator, and zero-semantic-attempt consumption are unchanged
for every branch. The already-committed `head-local-01`
`generation-metrics.json` (which predates this refinement and still reads
`upstream_server_stream_incomplete`) is **not** retroactively edited, per
the same discipline already applied to the earlier `upstream_model_crash`
addition in `m7-local-factorial-v3`; this document is the correction of
record for that one file.

## 3. `reasoning_effort` is closed as a lever — recorded for the execution condition

Per `LOW_REASONING_PROBE.md`: `reasoning_effort=low` produced a
byte-identical reasoning payload (SHA-256 match) to `reasoning_effort=none`
on this backend. Combined with `none` already being confirmed not to
suppress reasoning (`m7-local-factorial-v3`/`LM_STUDIO_TRANSITION.md`):
**this backend does not interpret `reasoning_effort` at any value tried.**
This parameter is closed as a means of controlling reasoning volume for
the remainder of this experiment and any future amendment in this line;
it is retained in the execution condition only for continuity with the
frozen `m7-local-factorial-v3` condition, not because it is expected to
do anything.

## 4. Restart interpretation after an excluded upstream failure

Recorded explicitly because it sits close to the no-retry rule and must
not be confused with it:

- **What is prohibited:** issuing a new request for the same unit while
  the server is in a known-bad state, or issuing repeated requests
  without confirming recovery in between. This remains absolutely
  prohibited.
- **What is permitted:** after the operator independently confirms server
  recovery (a minimal request outside this experiment, not a retry of the
  failed trial), resuming generation for the next unissued unit or
  re-issuing the **same** unit that failed is a **new, first semantic
  attempt** for that unit — not a retry — because the prior attempt
  consumed zero semantic attempts (it is excluded by definition). The
  no-retry rule governs repeating a *counted* attempt; an excluded
  attempt was never counted.
- **Bound on repetition, so this permission cannot become unlimited
  retrying in practice:** if the **same** unit fails from an excluded
  upstream cause **twice in a row** (regardless of which specific
  subtype), that unit is marked `upstream_blocked` and excluded from this
  attempt; generation moves to the next unissued unit. **No unit receives
  a third provider request in this attempt.** `upstream_blocked` units
  are reported explicitly, with the same "how many completed vs excluded"
  discipline already used elsewhere in this experiment (e.g.
  `AUTH_AND_BUDGET_AMENDMENT.md`'s judge-budget exhaustion rule).

Under this rule, `head-local-01`'s upcoming re-issue is its **first**
semantic attempt under the current (131,072-token) condition — its prior
excluded attempt does not count against the two-strikes bound.

## 5. Upstream failure history across this benchmark program

Recorded because the pattern across backends, concurrency settings, and
now output caps is itself information the server administrator does not
otherwise have visibility into:

| When | Backend | Concurrency | Elapsed before failure | Symptom | Reasoning activity before failure |
| --- | --- | --- | --- | --- | --- |
| `m7-local-factorial-v3` stage1-r2 | Ollama | sequential | 28,951.818 s (8h 2m) | HTTP 502 | none observed (silent throughout) |
| `m7-local-factorial-v3` timeout-3600000 | LM Studio | sequential | 9,536.248 s (158.9 min) | `Model unloaded.` | 25,530 reasoning events streamed, then stopped |
| `m7-local-factorial-v3` parallel-2 | LM Studio | 2-way parallel | 3,449.481 s / 3,449.289 s (57.5 min, both) | `The model has crashed...` | one side 32,876 reasoning events, the other **zero** (starved) |
| `m7-head-local-v1` skill-gen-2, head-local-01 | LM Studio | sequential | 1,436.126 s (23.9 min) | `stream closed before response.completed` (no server-supplied reason) | **zero** reasoning events for the entire duration |

Every entry is a long-running stream that failed to reach a completed
response, across two different backend implementations (Ollama, LM
Studio), both concurrency settings tried in this program, and now two
different client-side output caps — with no consistent relationship to
elapsed time (23.9 minutes to 8 hours), to reasoning volume (zero to tens
of thousands of events), or to which specific error text the server or
client eventually surfaced. This is consistent with a shared instability
in sustained generation somewhere below the level this program's client
can observe or control, rather than any one setting this program has
tried being the specific cause. Reported here as an aggregate observation
of value to the server administrator, not as a new claim about root cause.

## Resuming

Generation resumes from `head-local-01` under the unchanged 131,072-token
condition. `head-local-00` is not re-run (already `valid` under the current
condition). The two-strikes-then-`upstream_blocked` rule (§4) applies from
this point forward for every remaining unit.

## 6. `upstream_silent_truncation` — a new upstream class, added after the 9-unit batch completed

Status: recorded after the operator reviewed
`diagnostics/empty-final-content-investigation-head-local-04-08/REPORT.md`
(a fact-finding investigation the operator requested into
`head-local-08`'s `"raw response size"` adapter rejection and
`head-local-04`'s zero-content stop at 59% of the output-token budget).
Amends the failure taxonomy and this section's restart rule; does not
change the generation execution condition.

### What was established, directly from the raw SSE (not from the ambiguous adapter.stderr text)

Both `head-local-04` (77,520/131,072 output tokens, 59.1% of cap) and
`head-local-08` (27,305/131,072, 20.8% of cap) show, in their
`response.completed` event:
- `status: "completed"`, `error: null`, `incomplete_details: null` — the
  server itself reports a clean, non-truncated finish.
- `output`: exactly one item, `{"type": "reasoning"}` — no
  `message`-type item was ever created, so no final answer of any kind
  (not even an explicit `outcome: abstained` JSON) was ever produced.
- `usage.output_tokens` well under `max_output_tokens` in both cases —
  ruling out genuine budget exhaustion.
- Reading the actual `reasoning_text` transcript for both: the content
  stops mid-sentence, mid-analysis, with no sign of concluding or
  transitioning toward an answer.

**Conclusion, from these four converged observations, not inferred from
any single one alone:** the upstream server silently truncated the
response while reporting success. This is a measurement failure, not a
model failure — the model was never given the chance to finish; there is
no way to know from what was returned whether it would have found
anything.

### Why this is a new class, not a use of an existing one

Every other `upstream_*` class in this taxonomy
(`upstream_server_failure`, `upstream_server_stream_incomplete`,
`upstream_model_crash`, `upstream_stream_closed_before_completion`)
reports failure explicitly — a 5xx, a disconnect, a crash message.
**`upstream_silent_truncation` is the only class where the server claims
success.** Operationally this is the most dangerous kind, precisely
because nothing else in the taxonomy would catch it without inspecting
the response body's own structure (no message item) rather than trusting
its status field.

### Detection

`scripts/detect_silent_truncation.py`, wired into `scripts/run_trial.sh`
as a new branch ahead of the `empty_final_after_process_completion`
fallback. Checks, from the raw `provider-response.sse.gz`, exactly the
three conditions above (status/error/incomplete_details clean,
no `message`-type output item, `output_tokens < 0.95 * max_output_tokens`
— an operator-chosen margin, disclosed as chosen rather than derived).
Verified by test (`scripts/test_detect_silent_truncation.py`) against the
real recorded responses: `head-local-04` and `head-local-08` classify
`true`; `head-local-07` (a genuinely valid unit from the same batch, a
similar reasoning-token count) classifies `false`. Excluded from the
semantic denominator and consumes zero semantic attempts, same as every
other `upstream_*` class (exit 71, §4's two-strikes rule applies).

### Known code defect, recorded but not fixed during this experiment

`crates/reviewgraphen-reviewer/src/process.rs:775`:
```rust
if raw.is_empty() || raw.len() > MAX_CAPTURE_BYTES {
    return Err(ProcessReviewerError::Input("raw response size"));
}
```
Both branches of this `||` produce the identical error string. **This
ambiguity is not hypothetical — it actually misled this investigation's
starting point.** The operator's first hypothesis, based on reading
`head-local-08`'s adapter.stderr (`process reviewer input rejected: raw
response size`) alone, was that the 4 MiB `MAX_CAPTURE_BYTES` cap had
incorrectly rejected an oversized-but-valid response. Only decompressing
the raw SSE and measuring `raw-response.json` directly (0 bytes, not
oversized) ruled this out. Had the error string distinguished "empty"
from "exceeds 4 MiB," this dead end would not have been necessary.

Per operator instruction, `crates/reviewgraphen-reviewer` is not modified
during this experiment. This defect is recorded here as the concrete
incident of its cost, for a fix after the experiment completes — not
fixed now.

A second, related but non-blocking defect found in the same
investigation: `scripts/run_trial.sh`'s pre-existing classifier line
(`rg -q 'raw response size 0 is outside' adapter.stderr`) is dead code —
that exact substring never appears in the real Rust error text, which is
just `raw response size`. It did not affect any classification outcome
in this experiment, because an earlier, independent file-existence check
already correctly determined emptiness before that branch is ever
reached. Left in place (unreachable, harmless) rather than removed
mid-experiment, since removing dead code is itself a code change to a
file this section's own new branch was just added to — deferred to the
same post-experiment cleanup as the `process.rs` defect above.

### head-local-04 / head-local-08: re-run authorized

Per §4's excluded-attempt rule, both units' prior `upstream_silent_truncation`
attempts consumed zero semantic attempts, so re-issuing either is a
**first** semantic attempt, not a retry. Operator-authorized: re-run both
under the **unchanged** execution condition (same 131,072-token cap, same
everything) — no condition is loosened to try to avoid a recurrence.
Bounded per §4: at most 2 consecutive same-unit attempts; if a unit fails
identically twice, it is marked `upstream_blocked` and excluded, and no
third request is issued for it.
