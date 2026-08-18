# AMENDMENT-004 — verification flakiness, and probe indeterminacy

Date: 2026-08-19.
Status: written **during** the replication series, after seeing trial 1's
verification result and **before** seeing any verification result from
trials 2-10. That asymmetry is the whole point of writing it now, and it is
stated first rather than buried.

## 0. Exactly what had been seen when this was written

| Fact | State |
| --- | --- |
| `rep-methodology-1` generation | complete |
| `rep-methodology-1` verification | seen: `tests_failed` |
| `rep-methodology-1` probes | seen: both `fail` |
| `rep-baseline-1` | generation in flight |
| trials 2-5, both arms | not started |

So the rules below are **post-hoc with respect to one trial and pre-hoc with
respect to nine.** They are written to be uniform and symmetric precisely so
that the one trial already seen cannot be selectively rescued by them.

## 1. The verification harness is flaky, and the flake is unrelated to the code under test

`rep-methodology-1` reported `tests_failed`, with exactly one failing test:

```
git::cargo_admission_tests::a_valid_trusted_executable_is_admitted_and_its_real_version_is_reported
panicked at crates/reviewgraphen-ingest/src/git.rs:3130:14:
a valid absolute, executable, regular-file path must be admitted:
CargoToolFailure { kind: Unavailable,
  diagnostic: "failed to run allow-listed cargo --version: Text file busy (os error 26)" }
```

`ETXTBSY` — executing a file still open for writing. That test copies the
admitted `cargo` binary and runs the copy; under back-to-back cargo activity
the write handle is not always closed before the exec. It lives in
`git.rs`'s cargo-admission tests and has **nothing** to do with
`rust.rs`'s block-scope handling, which is the only thing any candidate may
change. The same suite passed 67/67 on this machine earlier in the session
against the pinned revision and against both completed task-2 candidates.

**A harness race must not be recorded as a model failure.** Doing so would
manufacture a false negative and, because the flake is arm-blind, would add
pure noise to a 5-versus-5 comparison that is already underpowered.

### 1.1 Rule — uniform post-series re-verification

After the series ends (complete or stopped), **every** completed trial is
re-verified once, from its stored candidate, in a quiescent phase with no
other cargo process running. The procedure is byte-identical to the
in-series one; only the contention differs.

- The **post-series verification is the reported one**, for all trials
  alike.
- The in-series verification is retained and reported alongside it.
- **Any trial whose two verifications disagree is flagged `flaky_verification`
  and reported with both outcomes and the failing test named.** It is not
  quietly resolved in either direction.
- This applies to all trials in both arms without exception. It cannot be
  invoked for one trial and skipped for another.

If a trial fails post-series verification on a test that *is* plausibly
downstream of `rust.rs` — anything in `tests/m2.rs`, the acceptance suite,
or the rust-extraction unit tests — that is a **genuine** failure and is
reported as one.

## 2. The probes are not reliable enough to carry the conclusion they were built for

`AMENDMENT-003.md` section 1 used two probes to correct the `_ => {}` claim.
Two facts now undermine how much weight those probe results can bear:

1. During probe development the harness reference fix produced **`fail`,
   then `pass`, then `pass`** on the macro probe across three runs of the
   same code. That inconsistency was noticed and not explained.
2. `rep-methodology-1` produced `fail` on **both** probes, on a candidate
   whose treatment of the probe's input is behaviourally equivalent to the
   pinned revision's — both leave the macro item unhoisted and
   non-conservative. Pinned measured `pass`. Identical behaviour cannot
   legitimately give opposite results.

**Therefore a single probe run is not evidence.** That applies to the
measurements already recorded in `AMENDMENT-003.md` section 1 as much as to
anything measured from here on.

### 2.1 Rule — triplicate probes, unanimity required

In the post-series quiescent phase, each probe is run **three times** on
each tree.

- Three identical results: that value is the recorded result.
- Any disagreement across the three: recorded as **`indeterminate`**, and
  **no claim is made from it in either direction.**
- The same triplicate procedure is re-applied to the four trees
  `AMENDMENT-003.md` section 1 reported from single runs — pinned, reference
  fix, task-2 treatment, task-2 control. **If those come back
  `indeterminate`, section 1's correction is itself downgraded to
  unestablished, and that is reported.**

### 2.2 What section 1 of AMENDMENT-003 can still be relied on for

Only this: the blind judge's `_ => {}` finding is **not established**. That
conclusion needs no probe — it follows from the judge having had no
execution environment, so its assertion was a Claim from the start. The
probes were an attempt to settle it as Evidence, and they have not settled
it. Both the original claim and its attempted refutation are now recorded as
unestablished, which is a worse-looking and more accurate position than
either.

## 3. Everything else in AMENDMENT-003 stands

The decision rules of sections 3-6, the alternating order, the
no-replacement rule, the stop conditions, and the judging protocol are
unchanged. The syntactic `foreign_item_catchall` measure of section 4 is
unaffected by any of this, because it is read off the emitted diff and
involves no execution at all — which is now its main virtue.
