# Cache measurement, and where the 5,400 seconds actually went

Ordered as instructed: the cache measurement comes first and stands on its
own. Task changes 1 and 2 are **not** implemented.

## 1. The cache works. It is not the wall.

Three independent measurements, all direct `/v1/messages` timings.

| test | cold | warm | speedup |
| --- | ---: | ---: | ---: |
| repetitive 9,048-token prefix, byte-identical repeat | 809.63 s | **0.37 s** | 2,179× |
| same prefix, new 30-char tail | — | **4.15 s** | 195× |
| **novel** 8,422-token prefix, byte-identical repeat | **31.52 s** | **0.36 s** | 87× |
| tiny 47-token prompt, three times | 0.35 s | 0.20 s / 0.20 s | — |

And, decisively, **for Claude Code's own request shape**, captured through a
logging pass-through proxy on a real session:

| request | bytes | elapsed |
| --- | ---: | ---: |
| 002 first turn | 2,324 | 3.13 s |
| 003 second turn (28 tool defs, full system) | 100,898 | **128.21 s** |
| 004 third turn | 102,591 | **3.70 s** |

**35× on consecutive Claude Code turns.** The cache hypothesis is refuted.

### 1.1 It caches on rendered tokens, not on raw bytes

Diffing 003 against 004 byte-for-byte, the first divergence is at **byte
580 of ~100 KB** — and it is a serialization change, not new content:

```
003: "content":[{"type":"text","text":"Available agent types for the Agent tool:…
004: "content":"Available agent types for the Agent tool:…
```

Claude Code emitted the same system message as a block array on one turn and
a bare string on the next. That breaks any byte-prefix match at 0.6% into
the request — and the turn was still 35× faster. So the backend caches the
*rendered* prompt, and this class of client-side churn does not defeat it.

### 1.2 `cache_read_input_tokens` is not reported, and that is all it means

The field is absent entirely from `/v1/messages` responses (`None`, not 0).
Timing proves caching happens. **Token accounting cannot see it.** The
operator's 0.147 s figure and these numbers agree; the counters simply do
not exist on this backend.

### 1.3 One outlier, withdrawn

The 809.63 s figure did **not** reproduce. A genuinely novel prefix of
comparable size cold-prefilled in **31.52 s (267 tok/s)**, consistent with
the operator's spec of 32k in 102.9 s (311 tok/s). A tiny prompt returns in
0.20 s, so there is no idle or model-load penalty. The 809 s observation was
the first request after three sandboxed trials were killed and is treated as
an unexplained one-off, not as the cold rate.

## 2. So where did 5,400 seconds go? Thinking, decoded very slowly.

`skill-1`'s transcript carries per-event timestamps. Span 5,275.5 s.

- **4,606.9 s (87%)** sits in gaps that *follow a tool result*, i.e. the
  model generating.
- The three largest gaps are 1,163.8 s, 1,009.9 s and 881.1 s — up to
  **19.4 minutes each** — and each ends at an `assistant(thinking)` event.

Pairing each gap with the block it produced:

| gap | block | chars | ~tokens | tok/s |
| ---: | --- | ---: | ---: | ---: |
| 1,163.8 s | thinking | 18,538 | 4,634 | **4.0** |
| 1,009.9 s | thinking | 17,184 | 4,296 | **4.3** |
| 881.1 s | thinking | 21,232 | 5,308 | **6.0** |
| 169.5 s | thinking | 3,081 | 770 | 4.5 |
| 90.1 s | text | 635 | 159 | 1.8 |

**The three largest thinking blocks: 3,055 s for ~14,238 tokens = 4.7 tok/s,
against the operator's measured 36.4 tok/s on code — 7.8× slower.**

Decode of 20,264 output tokens at 36.4 tok/s would be 557 s, 10.6% of the
span. At the observed 4.7 tok/s it is most of it.

## 3. Reasoning has not terminated for this workload

| | operator's post-change measurement | observed in `skill-1` |
| --- | ---: | ---: |
| thinking per response | 2,942 chars | mean **4,527**, max **21,232** |
| body per response | 6,554 chars | 2,333 chars **across the entire 90 minutes** |

15 thinking blocks, **67,910 chars of thinking against 2,333 chars of final
text — 96.7% thinking**. That is the same 97–99% ratio m7 measured on the
previous backend.

Estimated from characters, thinking ≈ 16,977 tokens and text ≈ 583, summing
to ~17.6k against the reported 20,264 output tokens: **thinking is counted
inside `output_tokens`, it is simply not broken out.** `thinking_tokens` is
0 and no per-turn reasoning key exists, so thinking volume is invisible to
any token-based check — it had to be measured from the block text.

**The confound named in the preregistration does not hold as stated.** The
server's low reasoning effort did not remove the dominant failure mode; it
reduced it for short single-turn prompts, and this workload still spends
almost all of its output on reasoning.

## 4. Correction to the premise of the stop order

The instruction said "Both arms timed out identically at 5400 s with zero
edits and zero compiles." That is true of `skill-1`. It is **not** true of
`noskill-1`:

| | skill-1 | noskill-1 |
| --- | --- | --- |
| logical turns | 23 | 36 |
| tool calls | 20 | 34 |
| **edits to the target file** | **0** | **6** |
| **real cargo invocations** | **0** | **6** |
| target file changed | no | **yes** |
| compiles | — (unmodified) | **yes** |
| **tests pass** | — | **120/120** |

`noskill-1` — the control arm, without the skill — **produced a change that
compiles and passes the entire suite**, including the 5 harness-owned
acceptance tests. It is still classified `loop_incomplete_timeout`, because
the preregistered rule classifies by how the loop ended and it was cut off
by the cap rather than terminating. That rule is correct and stands. But the
code state is a full pass, and calling both arms identical understates the
result badly.

## 5. What this means beyond m9

For the operator's server: **prefix caching is healthy and Claude Code
benefits from it.** The cost of an agentic loop here is not round-trip
prefill. It is that long free-form reasoning decodes at ~4.7 tok/s while
code decodes at 36.4.

A plausible mechanism, **not established**: speculative decoding wins when
the drafter predicts well, which favours code and disfavours free-form
reasoning; on reasoning you pay to draft and verify for a low acceptance
rate. Testing it would mean timing the same prompt with the drafter
disabled, which is a server-side change and the operator's call.

## 6. Recommended order, revised by this measurement

The cache is not the wall, so fixing it is not available as a lever. On
these numbers the ordering changes:

1. **The reasoning-rate question first.** At 4.7 tok/s, a single thinking
   block costs up to 19 minutes. No task change survives that. If the
   drafter is the cause, disabling it for this workload is worth more than
   both proposed task changes combined.
2. **Then give both arms the `syn` API surface.** Still right, and now
   sharper: `skill-1` spent its whole budget reading syn's source from
   `~/.cargo/registry` and never compiled, while `noskill-1` did not and
   finished a passing change.
3. **The forced exploration-to-implementation transition is worth less than
   it looked.** `noskill-1` made the transition unaided. What stopped it was
   the clock, not indecision.
