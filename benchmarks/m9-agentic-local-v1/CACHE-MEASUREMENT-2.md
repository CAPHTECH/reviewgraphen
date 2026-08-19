# Re-measurement under the fixed server

Supersedes the conclusions of `CACHE-MEASUREMENT.md`. That document's
*measurements* stand; its *diagnosis* was wrong, and the correction is below.

## 1. The tension in my own measurement: resolved, and it was genuine

The 128.21 s -> 3.70 s pair was **not** a replay of one captured request.
Established from the captured bytes: request 003 carried
`messages: 2`, request 004 carried `messages: 4`. The conversation grew
between them, so they were consecutive real turns.

Repeated just now with m9's **exact** request shape -- same sandbox, same
flags, same 28 tools, same skill, same task prompt -- through the logging
proxy:

| request | bytes | elapsed |
| --- | ---: | ---: |
| 002 first turn | 8,022 | 11.37 s |
| 003 second turn | 111,303 | **123.82 s** |
| 004 third turn | 125,694 | **19.89 s** |

A 14,391-byte delta on top of a 125 KB request, answered in 19.89 s. Cold
recompute of that context would be ~100 s at the operator's 311 tok/s.
**The cache is hitting for this shape now.**

### 1.1 But the `<total_tokens>` marker is not in these requests

Searched the raw captured bytes of every deep request across both proxy
sessions: **zero occurrences** of `total_tokens`, and zero of `tokens left`.
The first divergence between consecutive turns is at byte 6,280 (5%) and is
a serialization change -- Claude Code emitting the same system message as a
block array on one turn and a bare string on the next -- not a token counter.

So the specific mechanism the operator found does not appear in
`claude --print` requests at any depth I have captured. **The effect they
describe is nonetheless real for m9** -- see section 2 -- so I record the
effect as established and the mechanism for this shape as **not**
established. It may be that their fix was more general than the marker, or
that the marker is injected only in interactive sessions. I am not asserting
either.

## 2. The corrected time budget. The operator is right and my figure is retired.

The decisive arithmetic comes from skill-1's own transcript, not from a
model of it.

**Sum of the full context over the 15 distinct API calls = 1,269,205 tokens,
which is exactly the authoritative `input_tokens` total for the trial.**
Claude Code billed every turn's entire context as fresh input. That is the
signature of no cache credit, and it is internal to the trial's own record.

| | seconds | share of the 5,275 s span |
| --- | ---: | ---: |
| prefill, 1,269,205 tok at 311 tok/s | **4,081** | **77%** |
| decode, remainder | **1,194** | **23%** |

Corrected decode rate:

| basis | rate |
| --- | ---: |
| reported output 20,264 tok / 1,194 s | **17.0 tok/s** |
| char-estimated 17,561 tok / 1,194 s | **14.7 tok/s** |
| **operator's stated deep-reasoning rate** | **16.5 tok/s** |

The corrected rate **brackets the operator's figure**. My 4.7 tok/s is
**withdrawn**: it was thinking tokens divided by the whole gap, and the gap
was 77% uncached prefill.

**The speculative-decoding hypothesis is retired. Do not ask for the drafter
to be disabled.** There is nothing left for it to explain.

### 2.1 What the trial would have cost with the cache working

Delta-only prefill (74,531 tokens of context growth) at 311 tok/s = 240 s,
plus decode of 20,264 tokens at 16.5 tok/s = 1,228 s.

**Total 1,468 s -- 28% of the 5,400 s cap.** The same work, same turns, same
reasoning, inside a quarter of the budget. The cache bug alone accounts for
roughly 72% of the wall clock.

## 3. Thinking share: both figures, with their depths

| source | context depth | thinking share |
| --- | --- | ---: |
| operator | 30k | 62% |
| skill-1 (this experiment) | reaching 102k | 96.7% |

Recorded together. These are **consistent with thinking share rising with
depth**, and 96.7% is not offered as contradicting 62%. Noted alongside:
turning thinking fully off makes deep inputs produce no body at all, so
`enable_thinking: false` is not an escape -- the same wall m7 hit from the
other side.

Decode also degrades with depth by the operator's spec: 28.7 tok/s at 4k,
22.6 at 32k, 16.5 during deep reasoning. skill-1 sat at the deep end for
most of its run, which is why 16.5 is the right comparator.

## 4. What this means for the next experiment

**The unchanged task under the fixed server.** Not the `syn` surface change,
not the forced transition.

The reasoning is the coordinator's own and the numbers now support it: if
the cache bug accounts for ~72% of the wall clock, then changing the task
*and* the condition at once leaves the difference unattributable. A plain
re-run isolates the condition change, and it is also the cheaper move --
the harness, prompts, sandbox and verifier are unchanged and hash-pinned.

The `syn`-surface argument is weaker than it looked for a second reason:
`noskill-1` reached a change that **compiles and passes all 120 tests**
without being given syn's API, in 36 logical turns. The gap m8 identified is
real, but it is not obviously what stopped m9.

What a re-run would need: a fresh preregistration, because the condition
changed materially again (cache fixed), with results not poolable with the
two trials already run under the broken cache.

## 5. Status

- Series stopped, trial 3 void, `HALT VERIFIED` with zero survivors.
- No task change implemented.
- Two completed trials retained: `skill-1` (`loop_incomplete_timeout`, zero
  edits, zero compiles) and `noskill-1` (`loop_incomplete_timeout`, 6 edits,
  6 cargo invocations, code compiles and passes 120/120).
- Both were run under the broken cache and must not be pooled with anything
  run after the fix.
