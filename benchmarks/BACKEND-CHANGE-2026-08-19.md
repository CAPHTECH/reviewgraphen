# Backend change on the local Qwen server, 2026-08-19

**Additive record. Nothing in any frozen preregistration, amendment, or
results file is edited by this document.**

Status: m9-agentic-local-v1 halted mid-bring-up. No provider request has
been issued since the halt instruction, and none will be until the operator
decides whether the swap was intentional.

## 1. What changed

Measured by the coordinator, quoted verbatim:

```
before:  qwen3.8-27b-mlx, @8bit, @4bit, mtp   owned_by organization_owner   (LM Studio)
after:   Qwen3.8-27B-MLX-4bit                 owned_by mlx-dspark
```

| | before | after |
| --- | --- | --- |
| models advertised | 4 qwen variants + an embedding model | **1** |
| id form | `qwen3.8-27b-mlx` (+ `@8bit`, `@4bit`, `mtp`) | `Qwen3.8-27B-MLX-4bit` |
| `owned_by` | `organization_owner` | `mlx-dspark` |
| serving implementation | LM Studio | different (per `owned_by`) |
| quantization | **not stated** by the default id | **explicitly 4-bit** |
| coordinator probe latency | 1.59 s | 11.6 s |

The operator restarted the proxy; the backend underneath it is not the one
every prior result was produced against.

### 1.1 The last listing this experiment captured itself

`benchmarks/m8-impl-local-v1/runs/replication/*/generation-metrics.json`
records, per trial, the `/v1/models` listing at the moment of the request.
Every one of the ten replication trials recorded:

```
qwen3.8-27b-mlx, qwen3.8-27b-mlx@4bit, qwen3.8-27b-mlx@8bit,
qwen3.8-27b-mtp, text-embedding-nomic-embed-text-v1.5
```

which is the "before" listing. That is a hash-pinnable, per-request record
of the old backend, already in version control.

## 2. Scope — what must not be pooled

**Every result in this program to date was produced under the previous
backend.** Specifically:

| body of work | trials | backend |
| --- | --- | --- |
| `m7-head-local-v1` (the entire review benchmark) | all | previous |
| `m8-impl-local-v1` task 1, first run + re-run, both arms | 4 | previous |
| `m8-impl-local-v1` task 2, n=1 per arm | 2 (+1 lost upstream) | previous |
| `m8-impl-local-v1` task-2 replication | 10 | previous |
| `m9-agentic-local-v1` bring-up probes | 3 | **unknown, see section 4** |

Results produced under the new backend **must not be pooled with any of the
above**, compared against them as if the condition were held constant, or
used to revise any of their conclusions. A different quantization is a
different model for measurement purposes, and 4-bit versus an unstated
previous default is exactly the kind of change that would show up as an
apparent methodology effect.

This applies in both directions: the old results do not predict the new
backend's behaviour either.

## 3. The model-identity pin is defeated, and how

`AMENDMENT-002.md` added a pin because the model list had grown and "a
silent alias re-point to a different quantization would look exactly like a
methodology effect". The pin asserts that
`response.completed.model == "qwen3.8-27b-mlx"`.

**Against this backend that check passes while the weights have changed.**
The coordinator sent `qwen3.8:27b-mlx` and the response echoed
`"model": "qwen3.8:27b-mlx"` verbatim — the new backend reflects the request
value rather than reporting what it actually loaded.

This experiment's own bring-up probes show the same thing from the client
side: both reported `modelUsage` keyed `qwen3.8:27b-mlx` with
`canonicalModel: qwen3.8:27b-mlx`, which is the string that was sent.

### 3.1 The general lesson

A pin that reads the response's own `model` field is only as trustworthy as
the server's willingness to report honestly. It verifies **that the request
was accepted under that name**, not that those weights ran. It is a Claim
the server makes about itself, not Evidence — the same distinction this
methodology exists to name, arriving this time in the transport layer.

### 3.2 What a robust pin would have to check instead

Recorded now even though none of it is being implemented until the operator
decides.

1. **Hash the `/v1/models` listing, and gate on it.** The strongest check
   available, and it is the one that actually detected this. The listing
   changed in id form, cardinality, and `owned_by`; any of the three would
   have failed a hash comparison. **This experiment already captured that
   listing per request — `advertised_model_ids` and `advertised-models.json`
   in every m8 replication trial — but only *recorded* it and asserted on
   the echoed field instead.** Recording without gating is what let a
   detectable change be undetected. Gating on it is a small change to
   `run_generation.py`.
2. **A fixed-prompt output fingerprint.** Hold a small fixed set of prompts,
   sample each a fixed number of times, and hash the outputs. `m7`
   established the technique already, proving `reasoning_effort` was a no-op
   by SHA-256-matching reasoning and final text across runs. Weaker than a
   listing hash — sampling is non-deterministic and the server's own
   defaults have already been seen to change — so it belongs as a
   distribution check across several samples, not a single-shot equality
   test.
3. **Tokenizer and vocabulary probes** (token counts reported for fixed
   strings) are near-useless here: quantizations of one model share a
   tokenizer.
4. **Throughput fingerprints** are confounded. The two m9 bring-up probes
   went 105,022 ms then 3,133 ms time-to-first-token on the same server;
   that is the documented prefix-cache behaviour of the `cch` proxy
   (16,674 ms → 57 ms on a repeated prefix), not a backend signal. Latency
   alone cannot distinguish a swap from a cache hit.
5. **What none of these give** is an attestation of the loaded weights. No
   endpoint in this stack offers one. The honest ceiling is "the advertised
   listing and the observed behaviour are unchanged", which is a
   falsifiable check, not proof of identity.

## 4. m9 trial data of unknown provenance

`m9-agentic-local-v1` was in bring-up when the swap was measured.

| artifact | time | status |
| --- | --- | --- |
| bring-up probe A | 15:17:39 | 2 turns, ttft 105,022 ms |
| bring-up probe B | 15:26:55 | 2 turns, ttft 3,133 ms |
| trial `skill-1` | started 15:27:58 | **killed mid-loop by the halt** |

**I cannot determine which backend any of these ran against**, and I am not
issuing a request to find out. The latency difference between the two probes
is fully explained by prefix caching (section 3.2, item 4) and is not
evidence of a swap.

Therefore:

- `skill-1` is **void**. It was terminated by the halt, not by the model, the
  server, or a timeout. It is not a trial outcome, it does not count against
  the preregistered maximum of 6, and its transcript must not be analysed
  as a result. Its artifacts are retained only as a record that it existed.
- The three bring-up probes remain what `BRINGUP.md` already says they are:
  harness checks, not trials.
- **`m9` has produced zero valid trials.** When it resumes — if it resumes —
  it starts at trial 1 on whatever condition the operator settles.

## 5. Open question the swap creates

The previous default id `qwen3.8-27b-mlx` did not state its quantization,
while `@8bit` and `@4bit` were offered as separate ids alongside it. The new
backend advertises **only** `Qwen3.8-27B-MLX-4bit`.

So it is not currently known whether the new backend is the same
quantization as the old default, or a different one. If the old default was
not 4-bit, then every comparison across the change would confound
quantization with everything else. This is a question the program did not
have before, and it cannot be answered from any record in this repository —
the old listing shows which ids were *offered*, never which weights the
unqualified id resolved to.

## 6. What happens next is the operator's call

- **Intentional** — the new backend becomes a new condition with its own
  preregistration, not pooled with anything above.
- **Unintentional** — the previous backend is restored and m9 resumes on the
  original condition, from trial 1.

Neither is worked around from here. No provider request will be issued until
that decision is made.
