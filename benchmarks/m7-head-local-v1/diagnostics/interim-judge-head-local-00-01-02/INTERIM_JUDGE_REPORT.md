# Interim exploratory judge pass — head-local-00/01/02 (qwen_skill)

Status: **exploratory, not the primary result.** Per
`preregistration.json` `judge.interim_analysis_guardrail` (committed
before this analysis ran), this does not replace, and is reported
separately from, the single blind judge pass over all 9 units run after
generation fully completes. Generation of the remaining units was not
paused for this analysis.

## What was run

Blind judge protocol (`JUDGE_PROTOCOL.md`), applied unchanged, to the 13
findings qwen_skill produced across `head-local-00`, `head-local-01`,
`head-local-02` (32 total location claims, per
`../interim-mechanical-verification/`). One judge call per unit (3 calls
total), `opus`/`high`, `--json-schema` provider-constrained,
`--no-session-persistence`, `--tools ""`, `--max-budget-usd 3` per call (a
newly-added, newly-verified flag — see below). Blinding: content-hash
`finding_id` (arm/generator/local_id excluded from the preimage),
hash-sorted pooling, forbidden-marker scan before packet construction
(passed — no arm/generator/model identifier found in any judge-visible
file), `truth.json` never mounted into the judge's sandbox.

**Stage 1's mechanical verification results were deliberately withheld
from the judge**, per operator instruction, so the two checks remain
independent observations rather than one being anchored to the other.

## Two implementation defects found and fixed before any real judge call

1. **Path-prefix mismatch (would have broken judge-side location
   verification).** qwen_skill's findings reported locations as
   `agent_input/source/<path>` (head-local-00/01) or `source/<path>`
   (head-local-02) — the exact strings the model saw in its own prompt.
   The judge packet's `sources/` directory was built at `<path>` (no
   prefix). Sent as-is, the judge would have found 0/32 locations
   resolvable inside its own sandbox — not because the findings were
   wrong, but because of a packaging mismatch this analysis introduced.
   Fixed by normalizing `agent_input/source/` and `source/` prefixes to
   nothing before pooling; verified all 32 locations resolve inside the
   built packet before any judge call. Logged in
   `path_normalization_log.json` (32 entries, all mechanical stripping,
   no content change).
2. **`--max-budget-usd` was never wired through the frozen judge
   pathway.** `AUTH_AND_BUDGET_AMENDMENT.md` committed to this flag on
   every real judge call, but `run-process-reviewer-constrained claude`
   never passed it. Fixed in `crates/reviewgraphen-reviewer/src/process.rs`
   / `crates/reviewgraphen-benchmark/src/main.rs` (commit `bb15015`),
   verified end to end before use: a normal call with the flag succeeds
   unchanged, and a deliberately impossible budget ($0.0001) is genuinely
   rejected by the Claude CLI (`Error: Exceeded USD budget (0.0001)`, exit
   1, no record, no cost incurred). All 31 existing reviewer-crate tests
   still pass.

Also found and fixed before any real call: the packaged
`output-schema.json` carried a `$schema: "https://json-schema.org/draft/
2020-12/schema"` meta-reference the Claude CLI's own `--json-schema`
validator could not resolve (`Error: --json-schema is not a valid JSON
Schema: no schema with key or ref "..."`). Stripped `$schema`/`$id` from
the copy used only for this CLI argument; the canonical schema file in the
repo is untouched.

## Stage 2 results

**Disposition tally (raw, n=13):**

| Disposition | Count |
| --- | --- |
| `issue_should_be_created` | 9 |
| `should_not_be_created` | 4 |
| `unable_to_determine` | 0 |

**Quality distribution (n=13):**

| Field | Result |
| --- | --- |
| `specificity` | 13/13 `file_and_line_identified_and_relevant` |
| `reproduction_conditions_stated` | 10/13 true |
| `false_positive_suspected` | 2/13 true |
| `design_intent_confusion_suspected` | 3/13 true |
| `unresolvable_location` | 0/13 true |

**Duplicate clustering:** every judge asserted `duplicate_of: []` for
every finding. **13 raw findings → 13 distinct clusters — no duplication
found**, within or across these 3 units (unsurprising across units, since
each unit reviews disjoint source files; notable within a unit, since it
means the judge did not consider any of qwen_skill's own findings for a
single unit redundant with another).

**`distinct_issue_worthy_count` (high-confidence: `issue_should_be_created`
AND `file_and_line_identified_and_relevant` AND
`unresolvable_location=false` AND `reproduction_conditions_stated=true`
AND `false_positive_suspected=false` AND
`design_intent_confusion_suspected=false`): 7 of 13** (7 of the 9
`issue_should_be_created` findings clear every quality gate; 2 do not —
one because it lacks a stated reproduction condition, one flagged
`design_intent_confusion_suspected` despite the `issue_should_be_created`
call).

## Stage 1 vs Stage 2 agreement

**Full agreement on location validity, from two independent checks that
never saw each other's output:** Stage 1 (mechanical): 32/32 locations
resolved to real files with valid, non-degenerate ranges. Stage 2 (judge,
blind to Stage 1): 13/13 findings rated
`specificity: file_and_line_identified_and_relevant`, 0/13
`unresolvable_location`. Neither check flagged a single location the
other check would have contested. This is the answer to the operator's
specific question ("did the judge doubt a location Stage 1 found
mechanically fine") — no, zero instances.

## The four `should_not_be_created` findings, with the judge's stated reasoning

All four are substantive engineering disagreements, not dismissals — the
judge confirmed the underlying code facts in every case and disagreed
only on whether they constitute an actionable defect:

- **head-local-00** (`db.rs` separator-collision claim): confirmed the
  cited code is correct and the call sites are right, but concluded the
  separator token is a deliberately improbable choice and no user-visible
  impact was established (`design_intent_confusion_suspected: true`).
- **head-local-02** (Z3 version-pin claim): confirmed both cited locations
  exactly, but concluded the finding's premise about `z3::full_version()`
  is incorrect (`false_positive_suspected: true`,
  `design_intent_confusion_suspected: true`).
- **head-local-02** (LSP keyword-list claim): confirmed the factual
  observation holds, but concluded the list is a deliberately curated
  subset, not an omission (`design_intent_confusion_suspected: true`).
- **head-local-02** (`didSave` version-staleness claim): confirmed the
  code asymmetry exists, but concluded the finding's premise about
  `DidSaveTextDocumentParams` is wrong (`false_positive_suspected: true`).

## Spend

**Exact billed cost was not captured — stated plainly rather than
estimated as if precise.** `run-process-reviewer-constrained claude` uses
`--output-format text` (the raw candidate JSON only, per the existing
non-authority record design); it does not surface a cost/usage field, and
the temporary credential directory (which might have held ephemeral
session-cost data) was deleted immediately after each call per the
committed security procedure, before this gap was noticed. What is known:
total prompt bytes across the 3 calls ≈ 311,487 (input) and 12,563
(output); at a rough ~3.7 bytes/token heuristic this is on the order of
~84,000 input / ~3,400 output tokens combined, **but this is a byte-based
estimate, not a billed figure.** Each call carried an enforced
`--max-budget-usd 3` ceiling (verified real, not a no-op); no call
approached that ceiling based on response time and size. For an exact
figure, the Anthropic account's own usage dashboard is authoritative, not
this estimate. **Remaining budget for the final 9-unit pass:** treating
the interim spend conservatively as at most 3 × $3 = $9 (the enforced
ceiling, not the actual — actual is very likely well under this), at
least $21 of the $30 cap remains available, comfortably covering the
$10-15 estimate for the full 9-unit pass.

## My own observations (separate from the judge's disposition)

Two things stood out to me, reading the notes above the mechanical/judge
agreement: first, the judge's disagreements were never "this location
doesn't exist" or "this is vague" — every rejection was a substantive
"I checked the same code and read it differently," which is the kind of
disagreement a human maintainer could actually use (agree, push back, or
ask a clarifying question), unlike a rejection for vagueness which gives a
maintainer nothing to work with. Second, `unable_to_determine` never
appeared at all across 13 findings — the judge reached a confident
disposition every time, for both directions; whether that generalizes to
the remaining 6 units (with different code and possibly weaker findings)
is not established by n=13 and should not be assumed.

This is my own reading, not a judge output, and is not part of the primary
or interim formal results above.
