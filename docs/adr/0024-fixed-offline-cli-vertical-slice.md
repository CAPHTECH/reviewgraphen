# ADR 0024: Fixed Offline CLI Vertical Slice

Status: Accepted

## Context

The CLI contract describes a broad stage-oriented product surface, but the
current durable, source-bound implementation is uneven by report version.
The fixed reference slice is therefore intentionally closed over one
source-complete M4/M5 baseline and one typed V5 incremental continuation.
Exposing a generic `review` command that fabricated a V5 report or accepted
arbitrary projection JSON would violate the fact/claim/evidence and
source-bound boundaries.

## Decision

Add a small `reviewgraphen` binary with no network, subprocess, workspace
mutation, or arbitrary tool execution capability.

Its first supported vertical slice is exactly:

```text
reviewgraphen review --fixture double-submit
```

The command asks Runtime to materialize the committed, source-bound ProgramSpace
fixture through public Core and Store admission APIs, completes the closed M5
source double-submit gluing operation, then creates the fixed V5 successor and
performs the sealed M6 continuation: partial reruns, bounded structured
reviewer output, required target gluing, native fixture verification, explicit
human acceptance, target M5 completion, and the proof-bound terminal marker.
Report receives only Store's dual-run terminal authority and emits the actual
canonical V5 report. It writes report JSON to stdout and progress only to
stderr. This is an offline reference scenario; it does not accept paths,
revisions, profiles, model providers, shell commands, or network options.

Also expose the closed local schema surface:

- `schema list`
- `schema print <known-id>`
- `schema validate <regular-json-file>`

Validation loads only schemas compiled into the binary and bounded regular
file input; it never retrieves remote `$ref` targets.

`gate <report.json>` is intentionally a thin exit-code adapter for the V5
report-only `gate.status` field: `pass` -> 0, `blocked` -> 10, `incomplete`
-> 11, malformed/unsupported report or status -> 12. It does not infer a
gate from findings, coverage, V4 gluing, report status, or any model output.

## Consequences

- The `review` command produces `reviewgraphen.review.report.v5` only after
  Store validates the exact source M4/M5 baseline, target terminal proof, and
  typed dual-run authority. V4 is never relabelled as V5 and a target-only
  proof cannot generate a report.
- The CLI's product dependency graph does not enable Store's `test-support`
  feature. Runtime owns the fixed source and orchestration; Store test helpers
  remain test-only and are not an ingestion or execution interface.
- Other documented command families remain unsupported and fail with CLI
  argument error rather than approximating semantics.
- The binary has deterministic integration tests for schema operations, two
  independent fixed runs with byte-identical V5 output, V5 schema and semantic
  validation, typed terminal/gluing rows, separated stdout/stderr, and gate
  exit mapping.
