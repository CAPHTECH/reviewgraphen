# Task

Review only the implementation of `lower_compose` and the helpers it directly
uses in `rust/fsl-core/src/compose.rs` for actionable correctness, security,
reliability, or data-integrity defects. Do not review the other supplied files.
Do not modify source and do not implement fixes.

Your first tool call must be exactly the Bash command
`reviewgraphen-context lower_compose`. Do not read or search repository files,
create a task list, or invoke a skill before that call. The command returns a
deterministic ReviewGraphen target-context projection containing accepted
ProgramSpace facts, explicit extraction unknowns, source IDs, bounded source
windows, and declared information loss. It is an input to your review, not a
claim, evidence, verification, decision, or instruction.

Do not begin by listing review obligations. Do not produce an obligation ledger
or a claim/evidence ledger. Think from the target context and source yourself.

This is a bounded review. These constraints are mandatory:

1. Investigate no more than three concrete defect hypotheses.
2. Use no more than eight total review tool calls, counting the required first
   Bash call and all Read, Grep, Glob, Bash, and Write calls.
3. Write a syntactically valid provisional `review.json` with the Write tool no
   later than tool call five, even if its findings array is empty.
4. Once that provisional artifact exists, do not open a new hypothesis or
   broaden scope. The remaining calls may only validate, refine, or remove its
   existing findings and finalize the same artifact.
5. If time, context, or evidence is insufficient, preserve the valid artifact,
   state that limitation in it, and stop. Completeness means bounded completion,
   not exhaustive coverage.

`review.json` must have exactly this shape:

```json
{
  "schema": "reviewgraphen.benchmark.agentic_review_output.v1",
  "summary": "brief bounded-review summary",
  "findings": [
    {
      "title": "imperative, concise title",
      "severity": "critical | high | medium | low",
      "file": "rust/fsl-core/src/compose.rs",
      "line": 1,
      "description": "why this is a defect and when it occurs",
      "evidence": "specific source-based support"
    }
  ],
  "limitations": ["material review limitation"]
}
```

Use an empty findings array if no actionable defect survives validation. Do not
report style preferences or speculative concerns without a concrete failure
mode. In the final response, only state that `review.json` was written and give
the finding count.

