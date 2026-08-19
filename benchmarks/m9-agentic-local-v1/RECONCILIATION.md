# Reconciling 14.7–17.0 tok/s against the operator's 9.5

Asked: does skill-1's corrected decode rate agree with the operator's dspark
deep-reasoning measurement, and if not, which derivation needs qualifying?

**Answer: mine did. It is withdrawn.** And a second, larger correction falls
out of the same analysis.

## 1. The cache was crediting during skill-1. My "cache missing" finding is wrong.

I concluded the cache had missed entirely, because summing the 15 API calls'
contexts gave exactly the reported `input_tokens` total. I had already shown
that identity is a **reporting artifact** that holds in both cache states —
and then reasoned from it anyway.

Per-call timings settle it. **Twelve of the fifteen calls completed in less
time than a full recompute would physically take:**

| context (tok) | elapsed | full recompute at 311 tok/s |
| ---: | ---: | ---: |
| 27,880 | 3.4 s | 89.6 s |
| 31,322 | 41.7 s | 100.7 s |
| 92,725 | **57.6 s** | **298.2 s** |
| 93,164 | 103.6 s | 299.6 s |
| 93,736 | 125.8 s | 301.4 s |
| 94,108 | 64.9 s | 302.6 s |

A 92,725-token context answered in 57.6 s cannot have been recomputed from
scratch. The prefix cache was crediting for most calls.

Consequently the 4,081 s full-recompute prefill was not paid, so **the
14.7–17.0 tok/s I derived by subtracting it is withdrawn**, and so is the
claim that the cache bug accounted for ~72% of the wall clock.

## 2. What the trial actually did

Prefill was small, so elapsed is essentially decode plus tool time:

| | calls | output tok | elapsed | rate |
| --- | ---: | ---: | ---: | ---: |
| deep (>1,000 out tok) | 3 | 14,435 | 3,213 s | **4.5 tok/s** |
| shallow | 12 | 3,126 | 1,170 s | 2.7 tok/s |

**Three calls consumed 73% of the wall clock.** So the coordinator's first
option is also right: skill-1 was *not* in the deep-reasoning regime
throughout — it was deep for three calls out of fifteen.

## 3. Against the operator's figure

| | context | rate |
| --- | --- | ---: |
| operator, dspark, deep reasoning | ~30k | 9.5 tok/s |
| skill-1, dspark, deep calls | 58k–102k | **4.5 tok/s** |

A factor of 2.1, in the direction the operator's own data predicts: they
document decode degrading with depth (28.7 tok/s at 4k → 22.6 at 32k), and
our contexts were two to three times deeper than their measurement point.

**Neither derivation was wrong about the server.** Mine was wrong about the
prefill; the residual gap is a depth effect. This is recorded as a
consistent reading, not a proof — a single trial at one depth range cannot
establish a depth curve.

## 4. The speculative-decoding hypothesis, restored

`CACHE-MEASUREMENT.md` §5 proposed that speculative decoding wins when the
drafter predicts well, favouring code and disfavouring free-form reasoning,
because a rejected draft buys only verification cost. `CACHE-MEASUREMENT-2.md`
retired it as needing no test.

The operator's measurement confirms the mechanism:

| workload | dspark | baseline | |
| --- | ---: | ---: | --- |
| deep reasoning | 4,121 tok / 432.2 s = 9.5 tok/s | 6,000 tok / 363.0 s = 16.5 tok/s | **baseline 1.74×** |
| shallow code | 36.4 tok/s | 28.7 tok/s | reversed |

Restored as **confirmed by the operator's measurement**, not as something
this experiment tested. Draft acceptance tracks target entropy: it hits on
low-entropy code and misses on free-form reasoning.

The two explanations were never competing. Prefill dominating some accounts
and draft rejection dominating others are both true of different calls.

Also noted from the operator: their 28.7 / 22.6 / 16.5 were **mixed
conditions** — 4k single-shot, 32k single-shot, and 30k post-tool-result.
Only the third resembles this load, and it is the one our figures should be
compared against.

## 5. New counterfactual, for baseline

The old ~1,468 s prediction was derived against dspark **and** against a
full-recompute prefill that did not happen. Both premises are gone.

From skill-1's own measured decomposition, applying the operator's 1.74×:

```
deep     14,435 tok at 4.5 × 1.74 = 7.8 tok/s   ->  1,847 s
shallow  unchanged                              ->  1,170 s
                                        total   ~  3,017 s = 56% of the 5,400 s cap
```

Recorded before the trials, and far less optimistic than the figure it
replaces. It is a projection from one trial's decomposition, not a forecast:
turn count, tool use and reasoning volume all vary freely.

**It is an upper bound in one respect and a lower bound in another.** If the
operator's cache fix improves crediting beyond what skill-1 already had, the
shallow term shrinks. If a trial reasons more than skill-1 did, the deep
term grows without limit — that is the term that hit the cap.

## 6. Open, not forced

Nothing here is left unresolved, but two things are asserted only as
consistent readings rather than established:

- that the 4.5 vs 9.5 gap is a depth effect, rather than some other
  difference between our load and theirs;
- that shallow calls' 2.7 tok/s reflects tool time inside the measured
  window rather than genuinely slower decode — the window from one call's
  end to the next call's end contains the tool execution, and I did not
  separate them.
