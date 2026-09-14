# ADR 0052: Production profile v2 excludes benchmark trees

Status: Accepted

## Context

`rust.production.v1` excludes `benches` and `tests` path components but not the
equally conventional repository-level `benchmarks` component. ReviewGraphen's
own research snapshots therefore dominated a v4 self-review Node denominator,
even though they are not shipped production code. Changing v1 in place would
change stable profile hashes and exclusion identities.

## Decision

Add `rust.production.v2`, preserving v1 byte-for-byte. V2 changes one matcher:
`path.test_component@2` recognizes `benches`, `benchmarks`, and `tests`.
Production v4 accepts either the exact `(v1, 1)` or `(v2, 2)` pair and its
checked-in request example selects v2. V2 does not infer exclusions from Cargo
metadata or filesystem state; the denominator change remains an explicit,
hashed review-profile choice.

The v4 per-file admission ceiling is 16 MiB and the checked-in example uses a
128 MiB total ceiling. These remain closed bounds, but admit ReviewGraphen's
8.5 MiB frozen evaluator fixture so the repository can review its own tree.

V2 keeps the existing rule set and context policies. V2 exclusions bind the
new profile ID and hash, so they cannot replay as v1 exclusions. V2 is not
backported into the v2/v3 generic request families.

## Consequences

Self-review and other v4 users can exclude conventional benchmark trees
without relabeling the immutable v1 contract. Repositories whose deployable
code intentionally lives below `benchmarks/` must continue selecting v1 or
define another versioned profile.
