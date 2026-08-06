# ADR 0006: Use a Standalone Repository over HigherGraphen

- Status: Accepted for v0.1 design
- Date: 2026-08-07

## Context

ReviewGraphen reuses many HigherGraphen primitives, but it also introduces a substantial operational surface: code extractors, review profiles, LLM adapters, verifiers, local store, CLI, benchmark harness and provider/security policies.

Placing all of this in the HigherGraphen repository would couple release cadence, dependencies and review-domain decisions to the generic framework. Keeping ReviewGraphen completely independent would duplicate core types and weaken semantic interoperability.

## Decision

Create ReviewGraphen as a standalone repository and Rust workspace that depends on published or pinned HigherGraphen crates.

```text
CAPHTECH/higher-graphen   generic higher-structure substrate
CAPHTECH/reviewgraphen    review-centered intermediate tool
```

HigherGraphen may retain compatibility wrappers or reference pointers during migration, but review-specific implementation ownership moves to ReviewGraphen.

## Consequences

### Positive

- Independent release and dependency cadence.
- Heavy parser/provider dependencies do not enter HigherGraphen core.
- ReviewGraphen can maintain profiles, evaluation assets and security policy coherently.
- HigherGraphen remains dogfoodable through a real downstream tool.
- Commercial/public boundaries can differ without contaminating core.

### Negative

- Cross-repository version compatibility must be managed.
- Local development and atomic changes are harder.
- Some documentation and fixtures may be duplicated during migration.
- Compatibility wrappers need CI across repositories.

## Alternatives considered

### A. Add ReviewGraphen under `tools/` in HigherGraphen

Rejected for the target architecture. It is acceptable for a short prototype, but the expected dependency and release surface is too large.

### B. Fork/copy HigherGraphen primitives

Rejected. It would create semantic drift and duplicate validation.

### C. One monorepo for all `*graphen` tools

Deferred. It may reduce coordination cost, but current tools have different maturity and commercial boundaries.

## Invariants

- Dependency direction is ReviewGraphen → HigherGraphen only.
- ReviewGraphen run manifests pin HigherGraphen versions/interpretation hashes.
- Generic improvements are proposed upstream; review-only types stay downstream.
- Compatibility adapters declare information loss.

## Revisit triggers

- Cross-repository coordination cost exceeds dependency isolation benefits.
- HigherGraphen adopts a stable plugin workspace designed to host independent tool releases.
