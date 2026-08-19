# AMENDMENT-005 — the probe harness was wrong, and so was my correction

Date: 2026-08-19, written during the replication series (trial 8 in flight).

## 1. Answer to the question asked: `fail` means the probe DETECTED the condition

`PROBE ... =fail` is the **first reading**. Exit semantics, from
`scripts/run_replication.sh`:

```bash
if cargo test ... --test "$probe" > log 2>&1; then
  printf 'pass\n' > "$runs/$trial/$probe.result"
else
  printf 'fail\n' > "$runs/$trial/$probe.result"
fi
```

`pass` = `cargo test` exit 0 = the probe's assertion **held**.
`fail` = non-zero = the assertion **was violated** (or the tree did not build).

The probe asserts the *desired* behaviour:

```rust
assert!(!...any calls relation from caller to module target...,
  "a foreign module containing an unenumerable macro item must conservatively
   block naked-call resolution in that block");
```

So `fail` on a tree that builds means the naked call **was** resolved — the
fail-open condition is present. Confirmed directly in
`runs/replication/rep-baseline-3/m8_foreign_macro_probe.log`, on a tree that
compiled and passed all 120 tests:

```
test unenumerable_foreign_item_conservatively_blocks_naked_call_resolution ... FAILED
panicked at ...m8_foreign_macro_probe.rs:174:5
```

That is an assertion failure, not a build failure. **The polarity is not
inverted anywhere.**

## 2. Therefore AMENDMENT-003 section 1 is withdrawn. The judge was right.

`AMENDMENT-003.md` section 1 claimed the `_ => {}` finding was overstated,
on the strength of a table in which the pinned revision "passed" both
probes. **That measurement was an artefact. The claim it supported is
withdrawn.**

Measured now, each three times on freshly built trees:

| tree | macro probe | meaning |
| --- | --- | --- |
| pinned revision | **fail, fail, fail** | fail-open |
| harness reference fix (`_ => conservative_unresolved = true`) | **pass, pass, pass** | fail-closed |
| replication candidates with `_ => {}` that compiled | fail | fail-open |

The probe discriminates cleanly and stably. **A change that routes
unrecognized `syn::ForeignItem` variants to a no-op leaves a naked call in
that block matched to a module-level function; routing them to
`conservative_unresolved` does not.** That is exactly what the blind judge
described, from reading alone, with no execution environment.

**The judge's finding stands. My refutation of it was wrong.** I relayed a
correction that was itself incorrect, and this withdraws it.

## 3. Root cause: a shared `CARGO_TARGET_DIR` served stale libraries across trees

Every scratch tree is a copy of the same workspace, so every one builds a
package named `reviewgraphen-ingest` version `0.1.0`. All of them were built
into one shared `CARGO_TARGET_DIR`. Cargo served a previously-built library
for a *different* source tree.

Demonstrated deliberately, same shared target dir, nothing about the source
changed between the two blocks:

```
pinned, built fresh          -> FAILED, FAILED, FAILED
(reference-fix tree built in the same target dir)
pinned, immediately after    -> ok, ok, ok
```

The pinned tree's result flipped because a different tree's compiled library
was reused. This is a **harness defect that can produce wrong measurements**,
and it explains every inconsistency seen: the reference fix reading
fail-then-pass-then-pass during development, and the impossible "pinned
passes while behaviourally identical candidates fail".

### 3.1 Scope — what this does and does not put in doubt

- **Generation results are untouched.** No cargo runs during generation.
  Elapsed times, token counts, model identity, packet hashes, failure
  classes, and the emitted candidates are all unaffected.
- **Every `compiles` / `tests_pass` verdict in this experiment was produced
  through the shared target dir and is therefore not trustworthy as
  recorded.** That includes task 1, the completed task-2 pair, and the
  in-series replication verdicts. They are not assumed wrong — they are
  assumed *unverified*.
- The emitted diffs, and the syntactic `foreign_item_catchall` measure read
  off them, are unaffected: no execution is involved.

### 3.2 Fix, applied uniformly

Every post-series verification and every probe run uses a **private,
per-tree `CARGO_TARGET_DIR`**, so no tree can ever be served another tree's
artifacts. This costs a full rebuild per tree and is worth it.

Applied to **all** trials in **both** arms, and to the reference trees,
without exception. It cannot be invoked selectively.

`AMENDMENT-004.md`'s rules stand and are strengthened by this: uniform
post-series re-verification, triplicate probes with unanimity required, and
`flaky_verification` flagged wherever in-series and post-series disagree —
now with the mechanism that caused the flakiness identified and removed.

## 4. What this episode is evidence of

Recorded because it is the sharpest instance in this program of the thing
the methodology is about.

- The blind judge, with **no execution environment**, produced a Claim from
  reading code and citing the crate's own doctrine. It was right.
- I had a compiler and a test runner, produced what I presented as Evidence,
  and was wrong — because the instrument was broken and I ran it once.
- A single run of a mechanical check is not Evidence. It is one observation
  from an instrument of unknown reliability. `AMENDMENT-004.md` section 2.1
  already required triplicate; this shows why that is not pedantry.

The correct posture toward the earlier `RESULTS-002.md` section 4.1 claim is
the one it originally had: the judge found something real that 120 tests did
not. That is restored.

## 5. Not changed

Series conditions stay frozen. No retry on upstream failure. The
`AMENDMENT-003.md` decision rules, alternating order, no-replacement rule,
stop conditions, and judging protocol are all unchanged. No generation
request is re-run.
