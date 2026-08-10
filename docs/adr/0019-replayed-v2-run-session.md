# ADR 0019: Revision-Bound Replayed V2 Run Sessions

- Status: Accepted
- Date: 2026-08-10
- Scope: defines the editable replay boundary for a durable V2 journal. It
  supplements ADR 0014's replay admission boundary and preserves ADR 0017 and
  ADR 0018's distinction between exact store-visible bounds and
  allocator-opaque semantic state.

## Context

A durable V2 prefix can be validated by the store but cannot be made editable
by exposing an unchecked envelope append or a mutable `EventLog`. Replaying an
authority-bearing prefix also needs the exact non-serializable admissions that
were present when its records were created. Durable marker cleanup can fail
after `sync_data`; reporting the old in-memory aggregate then would be stale.

## Decision

`EventJournal::replayed_v2_session` acquires and retains the journal's
exclusive writer lock. It accepts explicit `EventAdmissions` and calls the
core V2 replay boundary; it never synthesizes admissions from persisted
metadata. Missing, wrong, stale, V1, corrupt, and empty prefixes are refused
before an editable session is returned.

`ReplayedV2RunSession` exposes only fallible read-only `aggregate`/`tail_hash`
and `append_command(EventCommand)`. A command is appended to a cloned core log,
then exactly its resulting envelope is durably appended through the held
writer. The clone replaces session state only after a receipt. No mutable log
or unchecked envelope escapes.

Writer durability is typed as confirmed or uncertain. A rollback-confirmed
failure leaves the old prefix healthy. Any post-sync, marker-cleanup,
rollback-cleanup, or poisoned uncertainty makes the session `SessionUncertain`;
reads and retries refuse. Restart/recovery alone reconstructs the durable
prefix.

`EventReplayLimits` has exactly `max_events` and `max_canonical_bytes`: both
are checked `u64` limits before output-log cloning. Canonical bytes are the
exact sum of canonical envelopes. V2 manifest/genesis, homogeneous-version,
structural envelope/payload, and D2 decode bounds remain independent.

This ADR deliberately makes no replay `max_working_bytes`, heap, allocator, or
RSS claim. `ReviewAggregate` uses standard-library `BTreeMap`s and serde
decode/apply has allocator-opaque transient state; no stable exact allocation
contract exists. This matches ADR 0017/0018's allocator-opaque core-replay
limitation and does not change their exact store-visible buffer contracts.
Future working limits require a crate-owned allocation representation and an
explicit versioned accounting contract, never inferred node sizes or RSS.

## Consequences

V2 replay callers retain/rebuild exact admissions and discard uncertain
sessions. They receive deterministic count/canonical-byte refusal, while
ordinary process resource controls cover semantic replay until a future
allocation contract exists.
