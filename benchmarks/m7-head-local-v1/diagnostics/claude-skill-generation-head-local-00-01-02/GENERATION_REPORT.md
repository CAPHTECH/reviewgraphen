# claude_skill generation — head-local-00/01/02

Status: generation complete for this arm's full committed scope (3/3
units, per `CLAUDE_SKILL_ARM_AMENDMENT.md`). These findings are not yet
judged — they are added to the same per-unit pool as `qwen_skill`'s
findings for the single primary blind judge pass, run after `qwen_skill`
generation completes over all 9 units. Nothing here is a judged result.

## What ran

`reviewgraphen-benchmark run-process-reviewer claude` (unconstrained —
see "Schema incompatibility" below), model `opus`, effort `high`,
`--no-session-persistence`, `--tools ""`, one call per unit, sequential,
against the operator's own Anthropic account via a fresh mode-700
temporary credential directory deleted immediately after each call
(`scripts/run_claude_skill_trial.sh`). Ran concurrently in wall-clock
time with the ongoing `qwen_skill` 9-unit generation (different backend,
different network endpoint — no interference; `qwen_skill` reached
`head-local-06` while these 3 calls were in flight).

Packets: byte-identical to `qwen_skill`'s (`instruction.txt` SHA-256
`a43d6984f9b58d5e47256afed68cddc1598ae84d6fc564e05f1cf462228d670a` for
all 3 units, verified before use), reused unmodified from
`/tmp/m7-head-local-v1-skill-prepared/`.

## Schema incompatibility found and resolved (zero cost)

The first attempt used `run-process-reviewer-constrained` (provider
`--json-schema` enforcement, same mode the judge uses). It failed with a
`400` **before any generation began** — no tokens billed:

```
API Error: 400 tools.0.custom.input_schema: input_schema does not
support oneOf, anyOf, or anyOf at the top level
```

`candidate-output.schema.json` has a top-level `allOf` (the
`abstained`/`parse_failure` conditional constraint on `findings` and
`obligation_results`), which Claude's tool-input-schema mode rejects
outright — a structural incompatibility, not a metadata issue like the
judge schema's earlier `$schema`/`$id` problem, and not fixable by
stripping fields without weakening what the schema actually enforces.

**Resolution:** switched to `run-process-reviewer` (unconstrained),
which matches the precedent already in use for every other generation
arm in this benchmark family — `qwen_b1`/`qwen_full`/`qwen_skill` all run
via the unconstrained `run-process-reviewer-codex-profile`, never the
`-constrained` variant; schema conformance is checked downstream via
`reviewgraphen-benchmark validate candidate`, per ADR 0037's
no-repair/no-normalization extraction contract, not enforced
provider-side. Only the judge backend uses `-constrained`, and only
because the judge's own output schema has no top-level `allOf`. This is
not a new mechanism invented for this arm — it is the pre-existing
generation-path contract, applied correctly.

## Results (raw, unjudged)

| Unit | Elapsed | Outcome | Findings | Obligations | Disposition tally |
| --- | --- | --- | --- | --- | --- |
| head-local-00 | 538.6s | `structured` | 9 | 8 | issue_present: 7, inconclusive: 1 |
| head-local-01 | 523.2s | `structured` | 5 | 8 | issue_present: 5, issue_absent: 2, inconclusive: 1 |
| head-local-02 | 611.8s | `structured` | 6 | 8 | issue_present: 6, inconclusive: 2 |

3/3 completed, 3/3 schema-valid (verified mechanically via `validate
candidate`, not just visual inspection — each unit's extracted
`candidate.json` in this directory hashes and validates independently).
0/3 hit the 8-obligation cap's upper bound in a way that suggests
truncation (all 3 used exactly 8, the cap itself, but every one's final
`obligation_results` array is well-formed and closed — no partial/cut-off
entries).

## Forbidden-marker scan: real self-reference check, not just asserted

The raw `candidate.json` for each unit legitimately contains the literal
string `local_id` (a schema field name — expected, matches the same
false-positive already observed and explained when scanning
`candidate-output.schema.json` itself). That is not what ships to the
judge. What the judge actually sees per `JUDGE_PROTOCOL.md` section 4 is
a reduced view (`locations`, `mechanism_tags`, `severity`, `rationale` —
no `local_id`, no `packet_id`). Scanning that exact reduced shape,
reconstructed from each unit's real `candidate.json`:

- head-local-00: 9 findings' rationale text — **no forbidden marker**,
  including no self-reference to `claude`/`opus`/`anthropic` in any
  capitalization.
- head-local-01: 5 findings — **clean**.
- head-local-02: 6 findings — **clean**.

This is an empirical result about this specific run, not a general
guarantee — `opus` did not self-reference here, but the scan (and the
abort-not-redact discipline behind it) stays in place for the actual
final judge-pooling step regardless.

## Budget

Same disclosed limitation as `INTERIM_JUDGE_REPORT.md`'s Spend section:
`--output-format text` does not surface an exact cost/usage field, and
the temporary credential directory was deleted immediately after each
call per the mandatory security procedure, before any per-call cost
figure could be captured. What is known: each call carried an enforced
`--max-budget-usd 2.66` ceiling (the CLAUDE_SKILL_ARM_AMENDMENT.md
per-call figure, $8 total ÷ 3), and none of the 3 calls failed or was
refused for exceeding it — all 3 completed normally with a full,
well-formed `structured` outcome, which would not happen if the budget
had been exhausted mid-response. **Worst-case exposure for this arm's
generation phase is capped at $7.98** (3 × $2.66); actual spend is very
likely well under that, consistent with how the interim judge calls
behaved under their own ceiling, but no exact billed figure is claimed.
The Anthropic account's own usage dashboard is authoritative for an exact
figure, not this report.

**This budget is entirely separate from, and did not draw on, the $30
judge cap** (`AUTH_AND_BUDGET_AMENDMENT.md`) — that cap's remaining
balance (at least $21 of $30, per `INTERIM_JUDGE_REPORT.md`) is
unaffected by anything in this report.

## Next step (not run yet)

These 3 units' findings join `qwen_skill`'s findings in the same
per-unit pool for the single primary blind judge pass, run after
`qwen_skill`'s remaining units complete — per
`CLAUDE_SKILL_ARM_AMENDMENT.md` "Pooling." This report is a generation
record only; no disposition, quality rubric, or `distinct_issue_worthy_count`
exists yet for `claude_skill`.
