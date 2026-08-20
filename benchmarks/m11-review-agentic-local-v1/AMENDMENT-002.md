# Amendment 002 — stop replication, run one control diagnostic

Status: written after `reviewgraphen-1` timed out and before any control model
request.  This is a result-informed deviation, not part of the frozen balanced
comparison.

`reviewgraphen-1` complied with the first-tool intervention but timed out at
5,400 seconds without `review.json`.  Its retained stream reports 9 model
calls, 422,652 input tokens, 107,447 output tokens, 99.86% thinking share, and
no suspected truncated tail.  The original six-trial series therefore stopped
without issuing `control-1`.

The remaining two ReviewGraphen trials and the balanced comparison are stopped.
One `control-1` trial is now run under the unchanged source, prompt, backend,
90-minute cap, validation, and zero-retry condition.  Its sole question is the
operator's confound check: can the previously failing no-ReviewGraphen review
task complete under the current server and agentic-loop condition?  It is a
single post-result diagnostic and must not be presented as a randomized,
balanced, or statistical comparison with `reviewgraphen-1`.

If the control produces a valid report, that report is eligible for the frozen
blind Codex judgment.  A timeout or invalid report has no candidate quality to
judge.  The Codex disposition remains an unreviewed, non-authority claim.

