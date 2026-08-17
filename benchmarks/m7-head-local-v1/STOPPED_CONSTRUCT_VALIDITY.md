# Execution stopped: construct-validity concern, not a result or timing decision

Status: recorded immediately upon stopping, before any diagnosis or fix.

## What was stopped and when

All `m7-head-local-v1` generation processes were terminated at
approximately 2026-08-17T10:34Z (operator-terminated, mid-flight):

- `qwen_full` on `head-local-00` was in progress (started
  2026-08-17T09:57Z per its `START` log line), streaming normally
  (6,094,848 bytes captured in its partial response at the time of
  termination). It produced no `generation-metrics.json`, no
  `candidate.json`, and no completed transport row — it consumes **zero
  semantic attempts**, the same `operator_terminated` classification
  already used once in `m7-local-factorial-v3` (`STAGE1_STATUS_SUMMARY.md`
  section 4.1).
- The request shaper and its batch process were also terminated.
- No further generation trial has been or will be issued until this
  concern is resolved.

## Why: this is not a time-based or outcome-based stop

The stop reason is that the measurement target itself is in question, not
elapsed time and not whether results looked good or bad. The operator
observed, by reading `agent_input/obligations.json` directly in the
already-built packets, that every obligation offered to the model in the
`qwen_full` arm has `property_id: "reviewgraphen.capability_gap"` — a
declaration that ReviewGraphen lacks the capability to judge the property
in question, not a substantive review obligation about the code. If this
holds generally, the `qwen_full` arm may not be exercising ReviewGraphen's
actual review capability at all, only its context-envelope and
output-contract scaffolding around an otherwise-empty obligation set. This
would call into question not just `m7-head-local-v1` but potentially prior
frontier comparisons in this benchmark family that used the same
`full_reviewgraphen` construction path.

Continuing to spend real server time and real judge budget on packets
whose obligation set may not represent what the experiment intends to
measure would not produce recoverable data even if the runs completed
successfully. The concern must be resolved before any further real
execution.

## What is preserved

All prior committed work — `preregistration.json`, `UNIT_SELECTION.md`,
`JUDGE_PROTOCOL.md`, `AUTH_AND_BUDGET_AMENDMENT.md`,
`DEVIATION_FULL_ARM_PRIORITY.md`, `units.json`, all scripts, and the two
completed `qwen_b1`/`qwen_full`-adjacent observations already on record
(`head-local-00` `qwen_b1`, complete and valid as a data point regardless
of this concern since B1 has no ReviewGraphen scaffold to be degenerate)
— remains unchanged. Nothing is rewritten by this stop. Diagnosis of the
capability-gap concern is tracked separately and reported before any
decision about resuming, modifying, or abandoning `m7-head-local-v1`.
