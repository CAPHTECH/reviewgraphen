# Judge authentication and budget amendment

Status: frozen before any real (non-synthetic) generation or judge call.
Operator-approved. This amendment records two operational conditions that
`preregistration.json` did not address, per the gap identified in
`SMOKE_TEST.md`.

## Authentication

The judge backend (`reviewgraphen-benchmark run-process-reviewer-constrained
claude`) authenticates as the operator's own Anthropic account — the same
account this Claude Code session itself runs under — using
`~/.claude/.credentials.json`. The operator explicitly approved this choice
(no separate service account is used).

The smoke-test handling in `SMOKE_TEST.md` is the frozen, mandatory
procedure for every real judge call, not just the smoke test:

1. Before each judge call (or each small batch under one sandbox
   invocation), copy `~/.claude/.credentials.json` into a fresh, mode-700
   temporary directory created for that call, used only as the sandbox's
   `credential_home`.
2. Delete that temporary directory immediately after the call returns
   (success or failure), before starting the next call.
3. No artifact, log, diagnostic file, or commit ever contains the
   credential file's path outside `/tmp`, or any substring of its content.
4. Before every commit that touches `benchmarks/m7-head-local-v1/`, run a
   mechanical scan of the staged files for the literal string
   `.credentials.json` and for the exact byte content of the live
   credential file (read fresh from `~/.claude/.credentials.json` at
   scan time, not cached), across every staged file. A match blocks the
   commit; the offending file must be fixed or excluded before proceeding,
   the same abort-not-redact discipline already used for the
   arm/generator forbidden-marker scan in `JUDGE_PROTOCOL.md` section 2.

## Budget

`--max-budget-usd 30` is set on every real judge invocation (the flag
exists on the installed Claude CLI; confirmed in `claude --help` during the
smoke test).

**Rationale, recorded before any real judge call**: 9 units, each with up
to 120,000 bytes of source plus the pooled findings and instructions,
judged by `opus` at `effort: high`. A rough estimate before execution,
based on typical per-token pricing at this model/effort tier and the input
sizes fixed in `units.json`, is approximately $10-15 total for all 9 units
combined. $30 is roughly double that estimate: enough headroom that a
normal run is not cut off mid-way by the cap, while still bounding a
runaway (e.g., a stuck retry loop or unexpectedly large output) to a fixed,
known amount rather than an open-ended one. This is a operator-set safety
ceiling, not a target spend.

**Handling budget exhaustion**, decided before any real call:

- If a judge call fails or is refused because the cumulative spend would
  exceed $30, execution stops. No further judge calls are issued.
- Every unit whose judge call **completed and validated** before the cap
  was reached keeps its result, unchanged.
- The unit whose call was in progress when the cap was reached, or any
  unit whose call has not yet been issued, is recorded as
  `judge_incomplete` and **excluded entirely** from all judged-finding
  metrics (`distinct_issue_worthy_count`, disposition tallies, quality
  distributions) — not partially scored, not backfilled with a partial or
  truncated response. Mixing a partial judgment into per-arm aggregates
  would distort the comparison the same way uncorrected near-duplicates
  would (see `preregistration.json` `usefulness_determination`
  `why_distinct_not_raw`), so the same all-or-nothing-per-unit discipline
  applies here.
- The final report states plainly how many of the 9 units received a
  completed judge call and how many are `judge_incomplete`, and does not
  present metrics computed over the completed subset as if they covered
  all 9 units.
- No retry is issued after a budget-related stop, matching the no-retry
  discipline already in force for the generation phase.

This amendment changes only authentication and spend-control mechanics. It
does not change the judge model, effort, prompt, schema, blinding
mechanism, unit selection, or the generation execution condition, all of
which remain exactly as frozen in `preregistration.json` and
`JUDGE_PROTOCOL.md`.
