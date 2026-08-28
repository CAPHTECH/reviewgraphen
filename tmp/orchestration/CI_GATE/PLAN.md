# CI gate repair — plan

Status: implementation and focused verification complete; full `fast` is
removed from the completion condition because of the separately owned nextest
timeout observation.

## Parallel read phase (completed)

1. Gate/caller/contract reading established fail-fast stage order and searched
   for tolerance mechanisms.
2. Bundle scanning used the validator's own roots and link-resolution semantics
   to enumerate every broken link, not only the first fail-fast result.
3. Formatter/workspace exploration separated product files from external source
   evidence and verified the candidate product set is formatted.
4. A full `scripts/` source-selection sweep found two same-class whole-tree
   selectors, both in `ci.sh` (rustfmt and coverage), and no others. The
   bundle validator's bounded bundle-root traversal is retained as a distinct
   document-validation contract.

## Decision gate — blocked on content owner

The original choice must now resolve five retained targets, not one:

1. Publish/commit a complete, reviewable evidence package providing the four
   m18 JSON targets *and* resolve the absent m17 `RESULT.md`; then retain the
   rationale citations unchanged.
2. Revise the frozen m20 rationale to point only at retained, reviewable
   evidence (or remove/recast claims whose source cannot be retained), including
   the m17 citation.

Neither is selected here. Do not alter m20 evidence/protocol prose or add
benchmark data until directed.

## Implementation after approval

1. Add one deterministic product-source selection helper to `scripts/ci.sh`,
   called by both `run_rustfmt()` and `run_coverage()`:
   tracked product Rust files in the current `crates/` workspace product roots,
   excluding the documented `tests/fixtures` external-source-evidence class.
   Do not use a per-file failure list; do not call `cargo fmt --all`, which
   includes the protected double-submit fixture.
2. Add a focused regression harness for the selection if existing tests cannot
   prove both sides of the boundary: an unformatted product file must fail;
   untracked/benchmark/external-fixture Rust must not affect the result.
3. Align `DEVELOPMENT.md` with the exact formatter selection and its evidence
   rationale.
4. Apply only the user-selected evidence-link resolution, then re-run the
   validator's full link scan to confirm zero retained broken links.

## Verification order

1. Run every focused fault injection listed in `REQREC.md` §8, recording
   command, injected fault, exit code, and observed diagnostic.
2. Run each restored clean stage; retain exit codes.
3. Record, but do not treat as this repair's completion condition, any clean
   `./scripts/ci.sh fast` attempt with admitted Cargo. `nextest --workspace`
   remains a minutes-scale, separately owned performance issue.
4. Inspect `git status --short` after every temporary injection cleanup to show
   no evidence/product mutation remains.
5. Run `gpt-5.6-sol high` review over `REQREC.md` and the final diff, covering
   requirement compliance, hidden assumptions, regression, scope expansion,
   architecture invariants, and test gaps. Convert any valid finding into a
   recorded constraint and rerun the full verification column.

## Pitfalls

- The current bundle validator stops at the first missing link; a one-error
  green assumption would miss four additional broken retained citations.
- Any whole-worktree `find` reacts to untracked agent worktrees. Tracked
  `git ls-files` is required for pollution immunity.
- `crates/` is not by itself sufficient: it contains external test fixture
  source that must remain evidence, not product formatting input.
- `run_coverage()` currently has the same whole-worktree source-presence scan;
  repairing only rustfmt would make fast and heavy/scheduled modes diverge.
- A successful `test-ci-admission.sh` contains an internal negative test, but
  does not by itself prove that `run_fast` propagates a failed stage; the
  injection must exercise the caller's fail-fast behavior.
- Fault injections must be isolated and cleaned before final verification so
  reviewed-artifact and dirty-worktree checks do not produce accidental noise.
