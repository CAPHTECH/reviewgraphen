# ADR 0032: M7 Regression-Test Form Closure

- Status: Accepted
- Date: 2026-08-15

## Context

ADR 0031 defines a mechanical presence oracle for a regression test added by a
fix. The first frozen enumerator operationalized "added" only as a newly named
Rust integration-test function. The retained run found 28 presence-eligible
units, below the 30-unit calibration threshold.

That operationalization is incomplete. A fix can add a regression assertion by
strengthening an already named integration test. Treating every parent failure
after test backport as presence would also be unsound: the original test may
already fail, or the backport may merely fail to compile.

## Decision

The original frame and presence results remain immutable. An additive candidate
supplement may admit a strengthened existing integration test only when the
same exact test selector establishes all three observations:

1. the unmodified parent test builds and exits zero;
2. after only the fix's test-support paths are backported, the parent test
   builds and exits nonzero at runtime; and
3. the fix test builds and exits zero.

A build failure, timeout, absent selector, originally failing parent test, or
production-source backport is not presence. Commands, revisions, test-support
paths, exit statuses, and raw-output hashes are retained. The supplement is
ordered by the already frozen split hash and is constructed before any reviewer
outcome is observed.

Inline unit tests are recorded by the recall audit but are not yet admitted.
They share files with production code, so a trustworthy test-only extraction
boundary must be specified before their use. Commits whose subject is not
classified as a fix are also recorded but remain ineligible: presence of a new
test for a new feature does not establish a pre-existing defect.

This ADR does not relax ADR 0031's 30-unit calibration/untouched-holdout split
or its power contract. If the additive supplement is still insufficient, a
successor statistical design requires a separate ADR before reviewer outcomes
are observed.

## Consequences

- The presence oracle covers both newly named and strengthened regression tests
  without counting compile failures or pre-existing failures.
- Existing candidate, presence, benchmark, and model-result artifacts keep their
  original meaning.
- The audit distinguishes extraction recall from semantic fix classification;
  feature commits are not relabeled as bug fixes to inflate the denominator.
