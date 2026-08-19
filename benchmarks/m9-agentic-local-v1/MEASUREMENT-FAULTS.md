# Two measurement faults in the loop record, and a correction

Established from the retained `stream.jsonl` of trial `skill-1`, not by
reasoning about it. The stream is the evidence; `loop-behaviour.json` is a
projection of it, and the projection was wrong in two ways.

No trial was re-run. Everything below was already in the stream.

## 0. Correction of something I told the coordinator

While trial 1 was still running I reported:

> "8 cargo invocations — the compile loop the single-shot harness could
> never show."

**That was false. The agent never invoked cargo. Not once.** See fault A.
The claim is withdrawn.

## Fault A — `cargo_invocations` counted a substring, not a command

The analyser tested `"cargo" in command`. The agent's Bash commands
contained:

```
SYN=$(ls -d ~/.cargo/registry/src/*/syn-2.0.119 …)
SYN=/home/rizumita/.cargo/registry/src/index.crates.io-…/syn-2.0.119
grep -n "pub enum ForeignItem" "$SYN/src/…"
```

Every one of the 8 counted "cargo invocations" was the path
`~/.cargo/registry`, where the agent was **reading syn's source to learn the
`ForeignItem` API** — precisely the knowledge gap m8 identified as the cause
of all three of its treatment failures.

Re-measured with `cargo` matched as a command word after stripping
path-like `.cargo` occurrences:

| | before | after |
| --- | ---: | ---: |
| bash commands containing the substring `cargo` | 8 | 8 |
| bash commands **invoking** cargo | 8 | **0** |
| cargo build / test / check | 0 / 0 / 0 | 0 / 0 / 0 |

The trial's own bash history confirms it independently: 13 commands, all
`ls`, `grep`, `sed`, `head`. No build, no test.

**Interpretive consequence.** The read of trial 1 changes from "compiled
eight times and never edited" to "**never compiled and never edited** —
spent 90 minutes reading source, including the dependency's source". The
compile-and-read-the-error cycle that motivated this whole experiment did
not occur in trial 1 at all.

## Fault B — token totals were summed from per-turn `usage`

### What the backend actually sends per turn

Exactly two keys, on all 45 assistant events:

```json
{"input_tokens": 27880, "output_tokens": 0}
```

Answering the question directly:

- **`output_tokens` is PRESENT AND ZERO.** Not absent, not lost in
  aggregation. The aggregator summed it correctly; the source is zero.
- **There is no separate reasoning key.** No `output_tokens_details`, no
  thinking field, nothing. Reasoning volume is not observable from this
  client at all.
- So the backend simply does not populate output token counts per turn.

### But a correct total does exist

The client's terminal `result` event carries its own accounting:

| field | value |
| --- | ---: |
| `usage.output_tokens` | **20,264** |
| `usage.input_tokens` | **1,269,205** |
| `usage.output_tokens_details.thinking_tokens` | 0 |
| `usage.cache_read_input_tokens` | 0 |
| `modelUsage[qwen3.8:27b-mlx].outputTokens` | 20,409 |
| `num_turns` | 23 |

So the fix is not to repair the aggregation but to **take token totals from
the `result` event**, which Claude Code computes itself, and to keep the
per-turn backend usage only as evidence of what the backend reports.

### `input_tokens` 3,818,353 was not trustworthy

Per-turn `input_tokens` is **cumulative context, re-reported every turn**:
monotonic non-decreasing, 27,880 → 102,411, with runs of identical values
(`27880, 27880`; `31322 ×4`; `100305 ×3`) because several stream events
share one API call's usage.

Summing it therefore multiply-counts the same prefix. The naive sum,
3,818,353, is **three times** the result event's authoritative 1,269,205.

The coordinator's alternative hypothesis — that the backend bills cached
prefix as input each turn — is **not** what happened: the growth is genuine
context accumulation, not a fixed prefix re-billed. Separately,
`cache_read_input_tokens` is 0, so the prefix-cache savings the operator
measured in wall-clock (0.147 s at 32k) are invisible in this backend's
token accounting.

The naive sum is retained in the record under the key
`input_tokens_naive_sum_DO_NOT_USE`, with the reason attached, rather than
deleted.

## Which fields were suspect, and which were not

| field | provenance | trustworthy |
| --- | --- | --- |
| `edits_to_target_file` = 0 | `tool_use` blocks in the transcript | **yes** — there are no `Edit`/`Write` blocks at all; the file list is empty |
| `compiler_errors_observed` = 0 | `tool_result` blocks, all plain strings | **yes** — and consistent with never having compiled |
| `tool_calls`, `bash_calls` | `tool_use` blocks | yes |
| `cargo_invocations` | `tool_use` inputs, but matched by substring | **was wrong**, now fixed |
| `assistant_turns` = 45 | raw stream events | yes, but it is not the logical turn count; the client reports 23 |
| `reported_usage_totals` | backend per-turn `usage` | **was wrong**, replaced |

The two load-bearing fields for interpretation — zero edits and zero
compiler errors — are counted from actual tool-call events and stand. The
finding that trial 1 made no change is unaffected by either fault; only the
account of what it did *instead* changes, and it changes for the worse.

## What was changed

`scripts/analyse_loop.py` (schema `v2`):

- `cargo` matched as a command word; the substring-only count is retained
  separately as `bash_commands_mentioning_cargo_path_only`;
- token totals taken from the `result` event under `tokens_authoritative`;
- backend per-turn usage kept under `tokens_backend_per_turn`, flagged
  suspect, with `output_tokens_all_zero` stated explicitly;
- `assistant_stream_events` and `logical_turns` reported as distinct;
- **every group carries a `provenance` field** saying whether it comes from
  stream events or from usage metadata.

`runs-v2/skill-1/loop-behaviour.json` was re-derived from the retained
stream. No trial was re-run.

## Why this is recorded at this length

This is the second time in this program that a corrupted measurement
produced a confident wrong statement — the first was the shared
`CARGO_TARGET_DIR` inverting the `_ => {}` conclusion. Both times the raw
artifact was retained and settled it. Retaining the stream, and treating the
derived JSON as a projection rather than as the record, is what made both
recoverable without spending a single further request.
