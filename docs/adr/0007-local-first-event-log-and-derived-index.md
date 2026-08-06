# ADR 0007: Local-First Event Log and Derived Index

- Status: Accepted for v0.1 design
- Date: 2026-08-07

## Context

ReviewGraphen must preserve partial runs, claims, rejected decisions, evidence and staleness across time. A mutable database row model makes history and authority transitions hard to audit. Storing only JSON reports loses intermediate state. A hosted database would add security and operations before the core hypothesis is validated.

## Decision

Use a local-first append-only event log as the durable workflow record, a content-addressed artifact store for large payloads, and SQLite as a rebuildable derived index.

```text
.events.jsonl / segmented event stream   durable state transitions
artifacts/<hash>                         source excerpts, raw output, traces
index.sqlite                             query/cache; disposable and rebuildable
reports/                                 projections; not canonical state
```

Parallel workers write temporary result artifacts. The orchestrator commits validated events in stable order.

## Consequences

### Positive

- Complete audit history and deterministic replay.
- Partial/failed runs remain inspectable and resumable.
- Reports and indexes can be regenerated.
- Local source code need not leave the machine.
- Content hashes support integrity and deduplication.

### Negative

- Event schema evolution and replay testing are required.
- Cross-record transactions need an orchestrator protocol.
- Large event histories require compaction/snapshot strategy later.
- Direct manual editing is unsafe.

## Alternatives considered

### A. SQLite as sole source of truth

Rejected. It is convenient but mutation history and rebuildability become optional rather than inherent.

### B. One report JSON per run

Rejected. It cannot robustly represent retries, conflicts, decisions and incremental invalidation.

### C. Hosted event service first

Rejected for MVP due to tenant, secret, availability and cost complexity.

### D. Git commits as the event store

Rejected. Binary/raw artifacts, high-frequency execution events and concurrency are a poor fit.

## Invariants

- Events are immutable after commit.
- Every aggregate transition validates against prior state.
- Artifact hash is verified on read/import.
- SQLite can be deleted and rebuilt without semantic loss.
- Report files are projections, never the sole canonical record.
- Redaction/deletion leaves a tombstone and declared loss where policy requires.

## Revisit triggers

- Event volume makes replay operationally unacceptable.
- Multi-user hosted operation requires a transactional distributed log.
- A stable HigherGraphen event-store primitive satisfies the same requirements.
