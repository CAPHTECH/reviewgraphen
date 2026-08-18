# AMENDMENT-002 — clean control, model-identity pinning, task-1 re-run

Date: 2026-08-19.
Status: frozen **before** the task-1 re-run and before any task-2 generation
request. `preregistration.json` and `AMENDMENT-001.md` are not edited.

## 0. What had already happened when this amendment was written

Stated first, as in AMENDMENT-001, so nothing here can be read as preceding
results it did not precede:

| Fact | State |
| --- | --- |
| task-1 methodology + baseline generation, verification, blind judge | complete (see `RESULTS.md`) |
| task-2 treatment arm | one attempt, died server-side, `upstream_stream_closed_before_completion` |
| task-2 control arm | never issued |
| everything preregistered below | not yet run |

The defect this amendment fixes was found and disclosed by this experiment's
own reporting (`RESULTS.md` section 1.4), not discovered afterwards by
someone else. The fix is applied *before* the arm it would otherwise damage.

## 1. The control was contaminated. It is now clean.

### 1.1 The defect

Both arms were given the **same** output schema,
`reviewgraphen.benchmark.implementation_candidate_output.v1`, which names
`obligations`, `claim`, `evidence_status`, and `limitations`. Those are
methodology vocabulary. Naming them in the control's own contract told the
control to enumerate obligations and to label evidence statuses — so the
control's "zero `verified`" result was not a null. It was the treatment's
discipline, leaking through the schema.

This made the secondary evidence-discipline measure meaningless in exactly
the direction that flatters the experiment: it made both arms look
disciplined, and it would have looked like a clean null forever.

### 1.2 The fix

A second, minimal contract for the control only:
`reviewgraphen.benchmark.implementation_candidate_edit.v1`
(`schemas/implementation-candidate-edit.schema.json`). It carries
`schema`, `outcome`, `edits`, and `abstention_reason`. `additionalProperties`
is `false`, so a control that invents obligation fields fails validation
rather than passing unnoticed. `apply_and_verify*.py` now selects the schema
from the candidate's own `schema` field and records which one it used in
`schema_validated_against`.

### 1.3 What the manipulated variable now is — stated plainly

**The methodology as a whole: its procedure *and* its reporting surface.**
That is what "with the method versus without it" actually means, and it is
the honest contrast. It is not a single-sentence intervention holding output
format constant.

The consequence, recorded in advance rather than discovered later: the
control is now asked for **less output** than the treatment. If the control
terminates more reliably or faster, part of that is the smaller output
demand, not the absence of the method. This is a real confound in the
opposite direction from the old one, it cannot be removed without
reintroducing the contamination, and it must be carried into any
interpretation of a termination or latency difference between the arms.

### 1.4 Packet hashes — the treatment condition is provably unchanged

| packet | sha256 | bytes | vs. before |
| --- | --- | ---: | --- |
| task-1 treatment | `b1f9d5f412ff35609eb7103e9217cb98e47d98bada1364aa5f837f302800ea06` | 37,234 | **byte-identical** |
| task-1 control | `7abfadd9c53c81cd971a02604b22db8ceccd73b2a7487fc2d64ab27bf9375de0` | 22,309 | changed (was `a329fd11…`, 23,119) |
| task-2 treatment | `dd06c8ba09f50db4cbcc75de23e997a38974f66a532da15ca932a57ec6cb2816` | 102,298 | **byte-identical** |
| task-2 control | `2e4c2fa6ab93c9e22553087076074dfb656ca0db5888a0eb729705d8e51ab7d7` | 87,373 | changed (was `3f7f116e…`, 88,183) |

Only the control changed. The treatment arms are the same bytes that were
frozen before the first generation request.

## 2. Model identity is pinned and verified, not assumed

The operator disclosed that the server's model list grew to
`qwen3.8-27b-mlx`, `qwen3.8-27b-mlx@8bit`, `qwen3.8-27b-mlx@4bit`,
`qwen3.8-27b-mtp`, plus the embedding model.

**A silent alias re-point to a different quantization would look exactly
like a methodology effect.** So:

- `qwen3.8:27b-mlx` remains the string **sent**. Unchanged.
- `EXPECTED_RESOLVED_MODEL = "qwen3.8-27b-mlx"` is what the server must
  report back in `response.completed.model`.
- Every run record now carries `model_sent`,
  `expected_resolved_model`, `resolved_model_reported_by_server`, and
  `model_identity_verified`.
- A verified-otherwise-successful run whose resolved model does not match
  exits **2** with `MODEL IDENTITY MISMATCH` rather than being accepted.
- The advertised model list is captured from `/v1/models` immediately before
  each request and stored as `advertised-models.json`, so a later reader can
  see which variants existed at that moment. **This is a control-endpoint
  read, not a generation request, and a responsive control endpoint remains
  explicitly not evidence that generation is healthy.**
- The `@8bit`, `@4bit`, and `mtp` variants are **not used**. Comparing across
  quantizations needs its own preregistration.

This is a strict addition to the recorded facts. Nothing about the frozen
execution condition changes: same endpoint, same model string sent, same
`max_output_tokens` 131072, no sampling overrides, no reasoning suppression,
one request in flight, no retries.

## 3. Task 1 is re-run, both arms

### 3.1 The decision, and why

**Chosen: re-run both arms.** Cost is roughly 16 minutes of server time.

Reasons, in the order they actually weighed:

1. **De-risking the arm that matters.** The clean control contract has never
   been sent to this model. Discovering a problem with it — a refusal to
   emit the minimal shape, a schema mismatch, an anchoring regression —
   on the cheap 13 KB task costs 8 minutes. Discovering it on task 2 costs
   the discriminating arm, which has already been lost once to the server.
2. **Same-session comparison.** The server changed between the first task-1
   run and now: it was unreachable, then recovered, and its model list grew.
   Comparing a new clean-control result against a treatment result recorded
   before all that would confound the control fix with a server change.
   Re-running both under the newly pinned and verified model id removes
   that.
3. **A stronger statement if it replicates.** A byte-identical diff under a
   *clean* control says more than one under a contaminated control: it says
   the task is genuinely undiscriminating, not that both arms were nudged
   into the same shape by a shared schema.
4. **Free second sample of an identical condition.** The task-1 treatment
   packet is byte-identical, so its re-run measures run-to-run variance of
   the same condition — the only such measurement this experiment will
   have.

### 3.2 What the re-run does and does not supersede

The first task-1 run is **not** deleted, overwritten, or reinterpreted. It
stays in `runs/methodology/` and `runs/baseline/` and stays reported. The
re-run is recorded separately under `runs/task1-rerun-*/`. Where they
disagree, both are reported.

The blind judgement already made on the first task-1 change stands as
recorded. If the re-run produces a different change, it gets its own blind
judge call under the same protocol; if it produces the same change, the
existing content-addressed judgement already covers it and no second call is
made, because the `change_id` is the same object.

## 4. Order of execution, and the budget

Sequential, one request in flight, never concurrent:

1. task-1 control (clean contract, first exposure — cheapest failure)
2. task-1 treatment (identical packet to the first run)
3. task-2 treatment
4. task-2 control

Four generation requests. `preregistration.json` allows at most 3 per task;
this is 2 per task.

## 5. Stopping rules — unchanged and restated

The standing order is unchanged. If any request fails with an `upstream_*`
class: **do not retry, do not start the next arm, stop the campaign**, and
report the time the symptom was noticed, the approximate token count of the
request, and the client. A model-identity mismatch (exit 2) is also a full
stop, and is reported the same way.

## 6. Reporting commitment added

The report must state which task-1 result it is quoting — first run or
re-run — every time it quotes one, and must not silently prefer whichever is
more convenient.
