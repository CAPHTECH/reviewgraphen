# head-local-04 / head-local-08 retry — final resolution

Status: retry authorized by
`RESTART_PROTOCOL_AND_UPSTREAM_HISTORY.md` section 6, executed under the
**unchanged** execution condition (same 131,072-token cap, same
`reasoning_effort=none`, same profile). Both units' prior
`upstream_silent_truncation` attempts consumed zero semantic attempts, so
these were first semantic attempts, not retries, per section 4.

## head-local-04: recovered, `valid`

First attempt succeeded. `provider_output_tokens: 84385/131072` (64.4% of
cap), `elapsed_seconds: 4116.52`, `outcome: structured`, **2 findings, 8
obligations**, schema-valid (`candidate-status: 0`). This result
**replaces** the unit's prior `upstream_silent_truncation` record for all
pooling purposes — that prior attempt was excluded by definition and
carries no findings.

Full artifacts: `head-local-04-attempt1/`.

## head-local-08: failed identically twice, `upstream_blocked`

- **Attempt 1:** `provider_output_tokens: 20718/131072` (15.8% of cap),
  `elapsed_seconds: 917.971`, `empty_final: true`,
  `failure_class: upstream_silent_truncation`. Independently confirmed
  via `scripts/detect_silent_truncation.py` against the saved
  `provider-response.sse.gz` (not just the metrics file) — `true`.
- **Attempt 2:** `provider_output_tokens: 14529/131072` (11.1% of cap),
  `elapsed_seconds: 625.333`, `empty_final: true`,
  `failure_class: upstream_silent_truncation`. Also independently
  confirmed via the same detector — `true`. Different token count from
  attempt 1 (14,529 vs 20,718), ruling out a cached/stuck-repeat
  artifact — these are two genuinely distinct upstream responses, both
  silently truncated.

Per section 4's two-strikes rule, `head-local-08` is now
**`upstream_blocked`** and excluded from `qwen_skill`'s pool. **No third
request was issued for this unit**, per the operator's explicit bound.

Full artifacts: `head-local-08-attempt1/`, `head-local-08-attempt2/`.

## Effect on qwen_skill's completion tally

Original 9-unit batch: 7 valid, 2 `empty_final_after_process_completion`
(later reclassified: both were actually `upstream_silent_truncation`).

After this retry:
- `head-local-04`: now `valid` (was `upstream_silent_truncation`, excluded, zero attempts consumed — now counted).
- `head-local-08`: `upstream_blocked` (2/2 consecutive `upstream_silent_truncation`) — excluded from the pool entirely, not counted as a model failure.

**Final: 8/8 measurable units valid (head-local-00 through 07), 1/9
(`head-local-08`) excluded as `upstream_blocked` — not part of the
semantic denominator.** This is reported as 8 valid units, not "8 of 9,"
because `head-local-08` was never actually measured; the taxonomy treats
`upstream_blocked` units as absent from the attempt, not as a 9th
attempt that failed.
