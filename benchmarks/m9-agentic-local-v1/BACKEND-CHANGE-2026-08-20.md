# The backend changed again — caught before the first request

Date: 2026-08-20T07:18:36+09:00.

**Zero generation requests were issued.** The pre-flight identity gate
refused, `run_trial.sh` exited 66, and the series stopped. There is no
`stream.jsonl` for the refused trial; the only artifact is the gate record.

## What changed

Same weights, same id, same owner. Speculative decoding is gone.

| field | v3 pin (2026-08-19) | observed now |
| --- | --- | --- |
| `id` | `Qwen3.8-27B-MLX-4bit` | `Qwen3.8-27B-MLX-4bit` |
| `owned_by` | `mlx-dspark` | `mlx-dspark` |
| `target` | `…/lmstudio-community/Qwen3.8-27B-MLX-4bit` | unchanged |
| **`mode`** | **`dspark`** | **`baseline`** |
| **`drafter`** | **`…/dspark-models/Qwen3.8-27B-Dspark-v1`** | **`null`** |
| `display_name` | `… (mlx-dspark dspark)` | `… (mlx-dspark baseline)` |

identity sha256 `b0efdd40…` → `afc7adb7…`

## Why this matters, and why it is not a small change

Every decode figure this program is currently reasoning with was measured
**with speculative decoding on**: the operator's 28.7 tok/s at 4k, 22.6 at
32k, and 16.5 during deep reasoning, and therefore also the corrected
14.7–17.0 tok/s I derived for `skill-1` and the ~1,468 s counterfactual that
`preregistration-v3.json` put on record.

With `drafter: null` those numbers no longer describe the server. The
prediction on record was made for a condition that no longer exists.

It also lands on a hypothesis I explicitly retired. After the cache
correction I said the speculative-decoding hypothesis needed no test and
asked for the drafter **not** to be disabled. It is now disabled. Whether
that was deliberate, unrelated, or a side effect of the cache fix is the
operator's to say — I am not inferring it, and I am not treating this as a
test I asked for.

## What the gate did, and what it cost

Nothing. The gate is a control-endpoint read; it ran before any request and
refused. Compare the previous swap, which was noticed only after a trial had
run 12 minutes against the wrong backend and I had wrongly reported it
stopped.

This is the second backend change inside this experiment. The first was
caught late by arithmetic; this one was caught at the door by the check that
replaced the echoed-`model` pin — which would have passed cleanly here,
since the id, the owner and the echoed field are all unchanged.

## A driver defect this exposed, now fixed

`run_series.sh` ran the loop analyser and the verifier even though
`run_trial.sh` had refused, producing `verdict=target_file_missing` for a
trial that never existed. That is a result-shaped artifact for a
non-event, and exactly the kind of thing that later gets read as data.

Fixed: the driver now checks `run_trial.sh`'s exit code, and on a
pre-flight refusal (66) it writes `NOT-A-TRIAL.md`, deletes the spurious
verification and loop artifacts, and stops. The stale artifacts from this
occurrence were removed.

## Status

- m9 v3: **0 of 6 trials.** The refused `skill-1` is not a trial and is
  excluded from every count.
- No generation request has been issued under the `baseline` backend.
- `preregistration-v3.json` is retained unedited. Its execution conditions
  are now false in one respect — speculative decoding — and it is not
  amended.

## What is needed before m9 can run

The operator's decision on whether `mode: baseline` is the intended
condition.

- **Intentional** — v3 needs re-pinning to `afc7adb7…`, and a note that its
  decode-rate expectations and the ~1,468 s prediction were derived under
  speculative decoding and no longer apply. Whether that warrants a v4
  preregistration or a pin update plus a disclosed amendment depends on
  whether they consider the drafter part of the model or part of the
  serving configuration. I would take a fresh preregistration, on the
  grounds that decode rate is the variable this experiment's cap is
  measured against.
- **Unintentional** — restoring `mode: dspark` returns the pinned
  condition and v3 runs unchanged.

Either way, one useful thing has become cheap: with `baseline` available
alongside `dspark`, the speculative-decoding question could be answered
directly by timing the same prompt under both. That is a server-side
comparison, it is the operator's call, and I am not requesting it.
