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

---

# Addendum, 2026-08-19 evening

## 7. Correction: my halt report was wrong

Section 4 of this document said all m9 processes were stopped. **That was
false.** One `claude --print` under bwrap was still running when the
coordinator checked — 12 m 25 s elapsed, still holding a connection to the
swapped backend. They terminated it. A halt that reports success while a
trial keeps generating is worse than the swap it was responding to, and the
report of success was mine.

### 7.1 Why the kill missed, established rather than guessed

Three faults, compounding.

**1. The killing shell killed itself.** The halt ran
`pkill -f run_series.sh; pkill -f run_trial.sh; pkill -f "claude --print"; pkill -f bwrap; …`.
`pkill -f` matches against full command lines, and the shell executing that
sequence has `claude --print` inside *its own* argv. So the third `pkill`
SIGTERM'd its own wrapper shell, and `pkill -f bwrap` never ran. The
evidence at the time was that the command produced **no output at all** —
not the `ps` listing, not the trailing `echo`, not the `date` — and exited
on a signal.

Confirmed empirically afterwards rather than assumed:

```
$ bash -c 'pgrep -af HALTPROBE_TOKEN_9931'
MATCHED: 1124205 /usr/bin/zsh -c … eval 'bash -c '…HALTPROBE_TOKEN_9931…'
MATCHED: 1124208 bash -c pgrep -af HALTPROBE_TOKEN_9931 …
```

Both the outer wrapper and the inner shell match a token that exists only
inside the command itself.

**2. Killing the wrapper script did not kill the trial.** `run_trial.sh`
launched `timeout` → `bwrap` → `claude` as ordinary descendants in the
caller's process group. `pkill -f run_trial.sh` removed the script and
orphaned the client, which kept generating.

**3. The verification was truncated, and I read a negative from it.** The
follow-up check was `ps -ef | grep -E … | head -10`. Ten lines came back,
all unrelated `claude --resume` sessions, and I concluded "none above means
stopped". The trial process was further down a list that `head -10` had
already cut off.

Fault 3 is the one that turned a failed halt into a success report. Faults 1
and 2 caused the survival; fault 3 caused the false claim about it.

### 7.2 What replaces it

`m9-agentic-local-v1/scripts/halt.sh`, and two changes to how trials launch:

- Trials run under `setsid` via `pgid_exec.sh`, which records its own `$$`
  as the process-group id and then `exec`s the sandbox, so one
  `kill -- -PGID` covers `timeout`, `bwrap`, `claude` and every descendant.
- `halt.sh` never pattern-matches inline argv text. It kills recorded
  process groups by id and sweeps for survivors by **binary path**
  (`codex-resources/bwrap`, `installs/claude/latest/claude --print`),
  excluding its own process group from every sweep.
- Verification is a **count**, printed with the complete untruncated
  survivor list, and a non-zero count makes `halt.sh` exit non-zero. No
  halt can report success from a list that was cut short again.

Self-tested: prints the full list, `survivor_count=0`, `HALT VERIFIED`.

### 7.3 The surviving trial is void

It was `skill-1`, already marked `VOID` for a different reason. It is now
void for a second, independent one: it ran partly or wholly against the new
backend. Which portion is not determinable and is not being determined. It
remains outside the trial count.

## 8. Codex can no longer reach this backend at all

`/v1/responses` returns **404**, and Codex 0.147 dropped `wire_api = "chat"`,
so there is no supported path from `codex-exec` to this server.

Consequence, independent of the weights question: **every m7- and m8-style
harness that drove the local model through Codex is unrunnable against the
current backend.** `m7-head-local-v1` used
`run-process-reviewer-codex-profile` for all of its local generation, and
`m8-impl-local-v1` used a direct `/v1/responses` client. Neither can be
re-executed as written.

Those frozen results are therefore **not reproducible against the current
backend for a transport reason alone**, before anything is said about
quantization or weights. m9 is unaffected: it drives `claude --print`
against `/v1/messages`.

## 9. Reasoning now terminates — and this is a confound, not a win

Operator measurement: the server sets reasoning effort low, producing 2,942
characters of thinking plus 6,554 of body, against roughly 10,000 / 0
before. `chat_template_kwargs: {"enable_thinking": true}` overrides it per
request; that is **not** used here, and using it would be a separate
preregistered condition.

The dominant failure mode of this entire program was reasoning that never
terminated: `m7`'s bare arm spent 65,535 of 65,536 output tokens on thinking
and emitted nothing, and two of nine review units ended in silent
truncation. **The new backend removes that failure mode at the server.**

So a better m9 result under this condition is not evidence that the
methodology transfers. The confound is baked into the condition and is named
up front in the new preregistration.

Also measured: code decode 28.7 → 36.4 tok/s with speculative decoding and
unaffected output content; 32k prefill 102.9 s cold, 0.147 s on a cache hit.

## 10. The listing volunteers more identity than the old one did

The pinned listing is:

```json
[{"drafter":"/Users/rizumita/dspark-models/Qwen3.8-27B-Dspark-v1",
  "id":"Qwen3.8-27B-MLX-4bit","mode":"dspark","owned_by":"mlx-dspark",
  "target":"/Users/rizumita/.lmstudio/models/lmstudio-community/Qwen3.8-27B-MLX-4bit"}]
```

identity sha256 `b0efdd40ac172d0905dcadb3b224d10c6794b335a2070f431c29ad68203af6cf`
(`created` excluded as a volatile timestamp).

It names both the loaded weights and the speculative-decoding drafter. That
is strictly more than the previous backend gave, and it is what makes the
gate in `scripts/check_backend_identity.py` meaningful. It does **not**
retroactively answer section 5: the old default id still never stated which
weights it resolved to.

## 11. Truncated response tails — a correspondence, not a common cause

The operator reports a backend-specific bug: **response tails cut
unnaturally — a sentence ending mid-way, a code block left unclosed.**
Upstream unfixed, locally patched, and the patch can be lost on a package
update.

This matches, symptomatically, the `upstream_silent_truncation` class this
program defined on LM Studio: `status: "completed"`, `error: null`,
`incomplete_details: null`, reasoning cut mid-sentence, and **no message
item at all**, well short of `max_output_tokens`
(`m7-head-local-v1/scripts/detect_silent_truncation.py`, established from
`head-local-04` and `head-local-08`). Ollama produced its own variant: an
8-hour hang with no response header, then HTTP 502.

Three serving implementations — Ollama, LM Studio, and now mlx-dspark — have
each produced a truncated or absent tail on the same weights family.

**No common cause is asserted.** These are three symptom reports, and the
symptom is generic enough that independent causes are entirely plausible.
What is recorded is the correspondence, so that if one is ever root-caused
the others are already on file as candidates to re-check.

Operational consequence: unchanged. If it recurs — no retry, record the time
the symptom was noticed, the approximate token count, and the client, then
stop and report.

## 12. Concurrency guidance has changed; the choice has not

The server now batches up to 2 requests, queues 3 and beyond, and the
operator measured **under 3% difference at 3 concurrent**. Sequential is no
longer required.

m9 stays sequential anyway. The measured gain is nil and single-flight
attribution is cleaner — a queued or batched trial shares contention with
its neighbour, and wall-clock is one of the things this experiment records.
