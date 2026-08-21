# Judge records

`01-findings.json`, `codex-result/process-record-3.json`, and
`codex-result/judgment.json` preserve the three-finding provisional artifact
that existed at tool call nine.  They are diagnostic only.

`final-01-findings.json`, `codex-result/final-process-record.json`, and
`codex-result/final-judgment.json` are the authoritative blind judgment inputs
and output for the two-finding artifact finalized at tool call twelve.

The first two provider attempts produced no judgment because the frozen m7 JSON
Schema used keywords outside the current Codex structured-output subset.  See
`../AMENDMENT-002.md` for the transport-only compatibility change.
