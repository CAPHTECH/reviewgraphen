# Durability finding: a raw CAS object written before its registration event makes every later attempt fail permanently

- Status: diagnosis only. No fix proposed, no code changed.
- Date found: 2026-08-29
- Scope: `run_fresh_fake_attempt` (`crates/reviewgraphen-runtime/src/lib.rs`,
  lines 218–246). The same shape exists in `record_human_decision`
  (`crates/reviewgraphen-runtime/src/m4_verification.rs:356–366`) and is noted
  at the end.
- How found: a local `qwen3.8-flash-next` reviewing one supplied
  ReviewGraphen Node obligation, `reasoning_effort: medium`, function-only
  window. The claim was then verified against the source by the orchestrator.
  **The model's claim is not the evidence; the code below is.**

## Finding

`run_fresh_fake_attempt` writes the reviewer's raw bytes into the
content-addressable store **before** it appends the event that registers them:

```rust
223:  let raw_receipt = store.put(&cas_hash, ..., Cursor::new(&raw))?;   // durable, outside the journal
...
228:  if raw_receipt.existed
229:      && session.aggregate()?.artifact_registration_count_for_cas_hash(&hash) == 0
234:      { return Err(RuntimeError::OrphanRawCas { hash }); }
...
244:  let registration_receipt = session.append_command(
245:      EventCommand::artifact_registered(registration.clone()))?;
```

The CAS write at 223 is durable and is not part of the journal transaction. If
`append_command` at 244 fails for any reason — a full disk, a lock, a crash
between the two — the process ends with the object on disk and no registration
event.

**Every subsequent attempt on the same input then fails permanently.** On the
retry, `store.put` reports `existed == true` because the object is already
there, and `artifact_registration_count_for_cas_hash` is still `0` because the
event never landed, so the guard at 228–234 returns
`RuntimeError::OrphanRawCas`. The guard that exists to detect the inconsistent
state is the same thing that makes it permanent: there is no path that either
completes the missing registration or discards the orphaned object.

The state is known to the codebase — `crates/reviewgraphen-runtime/src/lib.rs:2142`
asserts `Err(RuntimeError::OrphanRawCas { .. })` for a deliberately orphaned
hash — so the *detection* is tested. The *recovery* is not, because there is
none.

## What is established, and what is inference

**Established by reading the source:**

- The CAS write precedes the registration append, and is not rolled back.
- The orphan guard's two conditions are exactly what a post-failure retry
  produces.
- No code path clears an orphaned CAS object or back-fills the registration.

**Inference, not established:**

- Whether `append_command` can actually fail in production in a way that leaves
  the CAS object behind. The failure was not induced; the argument is
  structural. A test that injects an append failure between 223 and 244 would
  settle it.
- Whether fail-closed is the intended behaviour here. `OrphanRawCas` may be a
  deliberate refusal to touch an ambiguous store, in which case the finding is
  about the absence of a documented recovery procedure rather than about the
  guard.

## A second instance of the same shape

`record_human_decision` (`m4_verification.rs:356–366`) mints a decision that
advances `basis`, then appends it. If the append fails, `basis` has already
moved while the caller sees `Err`. This was reported by the same run and has
the same structure — a non-journal mutation before the journal write — but was
not traced as far as the CAS case and is recorded here only as a pointer.

## Not done, deliberately

- No fix. The right shape (write-ahead the registration, or make the CAS write
  part of the same transaction, or add a recovery entry point) is a design
  decision with durability-model consequences, and this document is the
  evidence step, not the decision.
- No test added. An injection test belongs with the fix.
