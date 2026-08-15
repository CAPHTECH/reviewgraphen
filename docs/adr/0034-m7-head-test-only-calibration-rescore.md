# ADR 0034: Post-hoc test-only rescore of the M7 HEAD verifier calibration

- Status: Accepted
- Date: 2026-08-15
- Amends: ADR 0033 for the additive rescore only

## Context

The first `m7-head-v1` calibration attempt is frozen in commit `e64e3c3`.
It produced 1 verified, 6 not-verified, and 13 unverifiable results. Of the 19
non-verified results, 13 stopped at unified-diff handling: nine test or fix
patches failed `git apply --numstat`, and four failed `git apply --check`.

Those failures conflate two capabilities:

1. identifying a known correctness defect and expressing a discriminating
   regression test; and
2. emitting byte-perfect unified diffs for both test and production changes.

The production experiment needs the first capability as its measurement
instrument. Requiring the second made diff formatting a construct-irrelevant
bottleneck. Removing the production-fix patch requirement is therefore a
construct-validity correction, not a change to the continuation threshold in
response to an undesired count.

This decision is nevertheless post-hoc: it was made after the attempt-1 result
was known. The original preregistration, report, process records, and mechanical
results remain immutable. This ADR does not describe the rescore as if it had
been planned before attempt 1.

## Decision

Create an additive `test-only-rescore-1`. Do not regenerate any model output.
All twenty units reuse their one existing semantic attempt and exact recorded
`test_patch` bytes.

### Deterministic test-source reconstruction

The rescorer may reconstruct a test source without invoking `git apply` only
when the recorded test patch has exactly this closed shape:

- one `diff --git` section naming the same path on both sides;
- the path equals the selected allowed new-test target in the frozen inventory;
- `new file mode 100644`, `--- /dev/null`, and `+++ b/<path>`;
- one hunk beginning at `-0,0 +1`; and
- after the hunk header, only added lines beginning with `+`.

The source is the ordered payload of those added lines, joined with LF and
terminated with one LF. Declared hunk line counts are recorded but do not gate
reconstruction. Multiple sections or hunks, context/deletion lines, path drift,
or any other shape is reconstruction failure. There is no manual repair.

### Test-only success endpoint

A unit succeeds only if the exact reconstructed test:

1. builds and fails at runtime on the known defective parent; and
2. builds and passes on the canonical fix revision.

Compilation failure, selector absence, timeout, or failure on the canonical fix
does not count. The production fix patch is neither parsed nor applied.

### Threshold fixed before rescore

The rescore continues to production only at 13 or more successes out of 20.
Because test-only evidence is easier than the former joint test-and-fix
endpoint, the old null success probability 0.25 is not reused. A conservative
null of 0.40 gives:

`P[Binomial(20, 0.40) >= 13] = 0.021028927477771152`.

At reference success probability 0.75, the pass probability is
0.8981881430772773. Thirteen successes also impose an operational minimum
observed rate of 65%, appropriate for an instrument intended to evaluate every
production finding.

These values are frozen in
`benchmarks/m7-head-v1/rescore-preregistration.json` before any test-only
rescore result is computed.

## Production trust-root limitation

The calibration has canonical fix revisions because its units are historical
known defects. A current-HEAD proposed finding has no canonical fix revision by
default. Therefore the same two observations cannot yet classify a production
finding as verified. A later production phase must define an independent,
trusted source of canonical fixes before model arms are run. A model-authored
change cannot certify itself merely by making its own generated test pass.

Passing this calibration does not silently resolve that missing production
trust root.

## Consequences

- Attempt 1 remains a valid, immutable measurement of the original joint
  test-and-fix generator.
- The additive rescore measures discrimination by an already-recorded
  regression test, not unified-diff formatting or generated-fix quality.
- No second semantic draw is introduced.
- If fewer than 13 units pass, the experiment stops before HEAD review.
- If 13 or more pass, production work still requires an explicit canonical-fix
  trust-root decision consistent with the two-observation endpoint.
