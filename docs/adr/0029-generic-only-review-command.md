# ADR 0029: Generic-only review command

Status: Accepted

## Context

ADR 0024 exposed `review --fixture double-submit` as an offline vertical slice.
That command executes a substantial authority/evidence/report path, but it
selects both the reviewed program and the reviewer result through a dedicated
fixture branch. Its success therefore does not establish that arbitrary Git
revisions can enter the same orchestration. Presenting it as `review` makes the
fixed demonstration easy to mistake for a working generic reviewer.

The isolated process adapters of ADR 0027 can execute Codex CLI and Claude CLI,
but the product CLI does not yet connect ordinary Git ingestion, deterministic
obligation synthesis, bounded context construction, those non-authority model
records, replay, and an explicitly incomplete authority result.

## Decision

Withdraw the fixed-fixture review command completely. The CLI rejects every
`review --fixture ...` form as an unknown command before reading a path,
materializing a Store, or executing Runtime. The usage surface does not mention
fixtures, and the CLI crate contains no fixed double-submit pipeline or
dependencies needed only by that pipeline.

The only review command that may subsequently be added is a generic command
over a versioned request naming ordinary repository identity and immutable Git
base/target revisions. It must route through the same product API used by its
tests. A named fixture, hidden option, environment variable, executable alias,
or special repository identity must not select a separate review algorithm.

Reference scenarios may remain as test data for lower-level contracts. They may
be supplied to the generic API as ordinary Git input, or used with an injected
test implementation of the same reviewer boundary. They are not product
commands and cannot establish generic end-to-end operation by themselves.

Until generic orchestration is implemented, `review` has no successful CLI
form. This exposes the missing capability instead of substituting a fixed
success. Generic model output remains non-authority under ADR 0027 and cannot
produce a trusted pass without the evidence and authority closures required by
the existing Core/Store contracts.

## Consequences

- ADR 0024's decision to expose `review --fixture double-submit` is superseded.
  Its historical description of the fixed vertical slice remains evidence of
  what that slice exercised, not a current CLI contract.
- A regression test compares all fixed-fixture forms with an unknown `review`
  invocation and requires the same exit 2, empty stdout, and usage text. A path
  shaped fixture argument therefore cannot trigger input access.
- Generic review completion must be demonstrated with ordinary Git input,
  recorded/replayed non-authority model output, and at least one live adapter
  execution. Fixed expected prose is insufficient evidence.
- Schema inspection and validation remain available and unchanged.
