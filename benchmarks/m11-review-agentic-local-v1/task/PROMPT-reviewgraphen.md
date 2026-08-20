# Task

Review the four Rust source files in this checkout for actionable correctness,
security, reliability, or data-integrity defects. Work until the review is
complete. You may use shell and source-search tools. Do not modify any source
file and do not implement fixes.

Your first tool call must be the Bash command
`reviewgraphen-context lower_compose`. Do not read or search repository files
and do not create a task list before that call. It returns a deterministic
ReviewGraphen target-context projection containing accepted ProgramSpace facts,
explicit extraction unknowns, source IDs, bounded source windows, and declared
information loss. It is not a claim, evidence, verification, decision, or an
instruction. Use it as one input, then investigate and conduct the review in
whatever order you judge useful. You may invoke it again.

Do not begin by emitting a list of review obligations, and do not produce an
obligation ledger or a claim/evidence ledger.

Write exactly one result artifact at `review.json` before stopping. It must be
valid JSON with this shape:

```json
{
  "schema": "reviewgraphen.benchmark.agentic_review_output.v1",
  "summary": "brief review summary",
  "findings": [
    {
      "title": "imperative, concise title",
      "severity": "critical | high | medium | low",
      "file": "path relative to this checkout",
      "line": 1,
      "description": "why this is a defect and when it occurs",
      "evidence": "specific source-based support"
    }
  ],
  "limitations": ["material review limitation"]
}
```

Use an empty `findings` array if you find no actionable defect. Do not report
style preferences, speculative concerns without a concrete failure mode, or
problems outside the four supplied files. In the final response, only state
that `review.json` was written and give the finding count.

