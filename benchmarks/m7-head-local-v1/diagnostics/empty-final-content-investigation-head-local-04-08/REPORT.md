# Investigation: head-local-08's "raw response size" rejection and head-local-04's zero-content stop

Status: fact-finding only, verified from code and raw SSE, per operator
instruction. **No execution condition, classification script, or
generation code was changed.** No judge call has been run. This report
does not decide how `head-local-04`/`head-local-08` are treated in the
final judge pool — that decision is the operator's, made after reading
this.

## Part 1 — head-local-08: is `MAX_CAPTURE_BYTES` the actual cause?

**Answer: No.** The raw response was empty (0 bytes), not oversized. The
4 MiB cap was never approached.

### Where the limit lives, and its value

`crates/reviewgraphen-reviewer/src/process.rs:21`:
```rust
const MAX_CAPTURE_BYTES: usize = 4 * 1024 * 1024;  // 4,194,304 bytes
```
Enforced at two sites:
- Line 740: `output.stdout.len() > MAX_CAPTURE_BYTES || output.stderr.len() > MAX_CAPTURE_BYTES` → error `"process capture exceeds limit"` (a **different** string from what was observed).
- Line 775: `raw.is_empty() || raw.len() > MAX_CAPTURE_BYTES` → error `"raw response size"` (this is the string actually observed in `adapter.stderr`: `process reviewer input rejected: raw response size`).

Both branches of the line-775 `||` produce the **identical error string** —
the code does not distinguish "empty" from "too large" in its output.
This is a confirmed code defect in the error message's specificity,
independent of which branch actually fired for this trial.

Grepped the whole crate for `"raw response size"`: it occurs exactly
once, at line 775. There is no other source for this exact string, so
the observed error unambiguously came from this one check.

### What head-local-08's raw response actually was

`process-output/raw-response.json` (the file `fs::read` loads into
`raw` for the `CodexCli` backend) is **0 bytes** — confirmed by direct
`wc -c`. Since `0 > MAX_CAPTURE_BYTES` is false, the branch that fired
was `raw.is_empty()`, not `raw.len() > MAX_CAPTURE_BYTES`. **The 4 MiB
cap was not the proximate cause of this rejection; the file was
completely empty, the opposite condition.**

### Is the underlying response legitimate, or abnormally inflated?

Neither framing fits — the raw-response.json file itself contains no
JSON at all (empty). But the **actual wire-level provider response**
(`provider-response.sse.gz`, decompressed 5,729,358 bytes / 80,241 SSE
lines — this is normal-sized reasoning output, not abnormal) is a
well-formed OpenAI-Responses-style event stream, verified directly:

- `response.completed` event present, with `status: "completed"`,
  `error: null`, `incomplete_details: null` — the server itself claims
  a clean, non-truncated finish.
- `usage.output_tokens: 27305`, entirely `output_tokens_details.reasoning_tokens: 27305` — **zero** tokens went to any message/text output.
- `output` array in the completed response: exactly one item, `{"type": "reasoning", "status": "completed"}` — **no `message`-type output item was ever created.**
- `max_output_tokens: 131072` confirmed correctly applied; actual usage (27,305) is only **20.8%** of that cap.

Full extracted evidence: `head-local-08-response-completed-summary.json`,
`head-local-08-event-type-histogram.txt` in this directory.

Because `codex` was invoked with `-o /workspace/output/raw-response.json`
and the response contains no message/text output item at all, `codex`
had nothing to serialize to that path — hence the 0-byte file. This is
a **downstream, correct consequence** of the upstream response's own
shape, not an adapter bug and not a size-limit misfire.

### Is the 4 MiB cap an intentional design value or provisional?

**No documented rationale was found anywhere for this specific number.**
Checked:
- The introducing commit (`33f78ef`, "feat(m7): add detection benchmark corpora and full ReviewGraphen verification"): the commit message discusses the reviewer adapter's isolation properties and benchmark results, not this constant.
- The line itself and its surrounding code: no comment.
- `docs/adr/0027-isolated-process-reviewer-adapter.md`: documents a **different**, input-side cap (`MAX_MATERIALIZED_PROMPT_BYTES`) with an explicit empirical rationale ("the largest exact input was 8,127,461 bytes"). It says nothing about the output-capture cap.

This is reported as an absence of evidence, not a claim that no reasoning
ever existed for it — but on everything checked, `MAX_CAPTURE_BYTES`
looks like an unexplained/provisional constant, not a derived one.

**However, for head-local-08 specifically, this question is moot** — the
cap did not fire; emptiness did.

### Classification: does the existing taxonomy already cover this, or is a new class needed?

**The existing `empty_final_after_process_completion` class is factually
correct for head-local-08** — verified independently from the raw SSE
(not merely inferred from the 0-byte file): the process/CLI exited
cleanly and the response's own `output` contains no message content. No
new `adapter_response_size_rejected` class is warranted, because the
adapter did not incorrectly reject a legitimate response — the response
genuinely had no final content to extract.

**What *is* a real, separate, confirmed defect** (reported for the
record, not fixed):

1. **The classifier script's regex is dead code.**
   `benchmarks/m7-head-local-v1/scripts/run_trial.sh:148`:
   ```bash
   elif (( process_status != 0 )) && rg -q 'raw response size 0 is outside' "$result_dir/adapter.stderr"; then
   ```
   This pattern (`raw response size 0 is outside`) **never appears** in
   the actual Rust error text (`raw response size`, no "0 is outside"
   substring, ever). This branch can never match the current code's
   output and has been dead since it was written. It did not affect
   head-local-08's outcome only because an **earlier, independent**
   check (line 143: does `process-output/raw-response.json` exist, and
   is it 0 bytes?) already correctly set `empty_final=true` before this
   dead branch is ever reached.
2. **Latent consequence, not observed here:** if a *genuinely* oversized
   response (`raw.len() > MAX_CAPTURE_BYTES`, the case the "raw response
   size" string can also mean) ever occurred, `raw-response.json` would
   exist and be large (not 0 bytes), so `empty_final` would stay `false`
   at line 143, the dead regex at line 148 still wouldn't match, and
   classification would fall through to `else: write_metrics
   transport_or_process_invalid` (line 216-217) — a generic bucket, not
   a size-specific one. This has not happened in this experiment; it is
   a latent gap, not an observed failure.

## Part 2 — head-local-04: what actually ended the stream?

**head-local-04 is the same underlying phenomenon as head-local-08, at a
different token count — not the operator's originally-suspected budget
exhaustion, and not a distinct failure mode.**

Identical artifact pattern: `process-output/raw-response.json` is 0
bytes, `adapter.stderr` is the same `"raw response size"` string,
`process_status: 1`. Decompressed `provider-response.sse.gz`
(6,977,463 bytes-ish / 227,943 SSE lines) shows the same shape as
head-local-08:

- `response.completed`: `status: "completed"`, `error: null`, `incomplete_details: null`.
- `usage.output_tokens: 77520`, all of it `reasoning_tokens: 77520`.
- `output`: exactly one item, `{"type": "reasoning", "status": "completed"}` — no message item.
- `max_output_tokens: 131072`; actual usage is 59.1% of that — **well
  short of the cap**, ruling out "ran out of the 131,072-token budget"
  as the mechanism, as the operator suspected.

Full evidence: `head-local-04-response-completed-summary.json`,
`head-local-04-event-type-histogram.txt`.

**This corrects a characterization I gave earlier in this conversation**
(reporting head-local-04's completion as "a real, non-preempted measured
result" without further qualifying it, and QWEN_SKILL_ARM_AMENDMENT.md's
general framing of this failure class as budget pressure) — that framing
is not supported for this specific instance. Neither `head-local-04` nor
`head-local-08` exhausted the output-token budget; both stopped for some
other reason, at 59.1% and 20.8% of the cap respectively, two unrelated
fractions with no shared value.

### What the raw reasoning content shows, at the exact cutoff point

Read the last ~1500 characters of each unit's `reasoning_text.done`
event (the model's own reasoning transcript, as actually streamed):

- **head-local-04** cuts off mid-sentence, mid-proof, in the middle of a
  detailed case analysis of a hash-collision scenario in an ID-generation
  function: `"...For A to be at index 1 with base "x" and C (base "x_1")"` — the sentence has no verb, no conclusion; it stops.
- **head-local-08** cuts off mid-sentence, mid-recollection, while the
  model is trying to recall CPython's PRNG seeding implementation from
  memory to compare against Rust source: `"...Let me recall the actual CPython code:"` — followed by nothing.

**Neither transcript shows any sign of concluding, wrapping up, or
transitioning toward producing a final answer.** Both are mid-thought,
substantive technical reasoning, cut off at an arbitrary point.

By direct contrast, `head-local-07` — a **valid** unit from the same
batch, same execution condition, similar reasoning-token count
(29,357) — has a `response.completed.output` of `["reasoning",
"message"]`: **two** output items, the second being an actual message
with 204 `response.output_text.delta` events producing real final
content. This is the precise, mechanically verified distinction between
a valid and a failed generation in this batch: **whether a `message`
output item was ever created after the `reasoning` item, not how many
reasoning tokens were spent.**

### What this does and does not establish

**Established, directly from the raw SSE:**
- The server's own `status: "completed"` / `error: null` /
  `incomplete_details: null` signal is present on both failed
  responses, identical in shape to a successful one.
- The actual reasoning content for both is verifiably mid-sentence, not
  concluded, at the moment the stream ends.
- Neither failure is tied to the 131,072-token output cap; they stopped
  at 59.1% and 20.8% of it, with no shared token count between them.
- The mechanical distinguisher between "valid" and "empty final" in this
  batch is the presence or absence of a `message`-type output item, not
  token count.

**Not established, and not claimed:**
- *Why* the upstream server terminates the response mid-sentence while
  still reporting `status: "completed"` with no error. This is
  server-side behavior this client cannot observe past the SSE stream
  it receives; no root cause is asserted (not "an LM Studio bug,"
  not "a hidden secondary token cap," not any other specific
  mechanism) without further evidence this investigation does not have.
- Whether this is deterministic per-unit or would reproduce identically
  on a retry (no retry was run, per the no-semantic-retry discipline
  already in force, and none is proposed here).

## Bottom line for the pooling decision

Both `head-local-04` and `head-local-08` are **not** adapter defects and
**not** budget exhaustion. Both are cases where the upstream server
signaled a clean completion while the model's own reasoning content was
demonstrably unfinished, producing no final answer. This is a genuine
measurement gap — the model may or may not have "had a finding" for
these two units; there is no way to know from what was actually
returned. Whether to record `head-local-04`/`head-local-08` as
"could not be measured" rather than "the model failed to find anything"
changes what `qwen_skill`'s per-unit pool looks like for the final judge
pass; this report does not make that call.
