# Diagnostics finding: an unfinished registration is reported as evidence tampering

- Status: diagnosis only. No fix proposed, no code changed.
- Date found: 2026-08-29
- Scope: `m4_verification.rs:220`, and the error variant it returns,
  `M4RuntimeError::FixtureOutputMismatch` (`m4_verification.rs:58–59`).
- How found: a local `qwen3.8-flash-next` reviewing one supplied
  ReviewGraphen Node obligation, `reasoning_effort: medium`, **module-wide
  window**. The claim was then verified against the source by the
  orchestrator. This finding is only reachable with a window wider than the
  function: the error's documented meaning is 160 lines above the use site.

## Finding

The stage dispatch maps three terminal stages to three errors:

```rust
218:  VerificationAttemptStageV3::Complete       => Err(M4RuntimeError::AlreadyComplete),
219:  VerificationAttemptStageV3::BundlePartial  => Err(M4RuntimeError::ResumeRequired),
220:  VerificationAttemptStageV3::InputRegistered => Err(M4RuntimeError::FixtureOutputMismatch),
```

The first two are faithful. The third is not. `FixtureOutputMismatch` is
declared as:

```rust
58:  #[error("fixed verifier output differs from the Store-sealed fixture execution")]
59:  FixtureOutputMismatch,
```

`InputRegistered` means the CAS input is present and the journal registration
is absent — an unfinished write, with **no output produced and therefore no
output compared**. Returning `FixtureOutputMismatch` for it tells the caller
that a sealed fixture execution disagreed with a fresh verifier run, which is a
data-integrity or tampering signal.

A caller that pattern-matches on `FixtureOutputMismatch` to decide "the store
or the harness has been tampered with" will reach that conclusion from an
interrupted write. In a system whose stated purpose is auditable provenance,
an error variant that means *the evidence disagrees* being raised when *no
evidence was produced* is a defect in the diagnosis, independent of whether
the refusal itself is correct.

## What is established, and what is inference

**Established by reading the source:** the mapping at 220, the declared meaning
at 58–59, and that `InputRegistered` involves no output comparison. The two
neighbouring arms are semantically faithful, so the mismatch is specific to
this arm rather than a convention of the dispatch.

**Inference, not established:** that any caller actually branches on
`FixtureOutputMismatch` to detect tampering. No such caller was traced. If
none exists, the finding is about a misleading message rather than a wrong
decision — still worth fixing, but lower.

## Why this one is worth recording beyond the defect

`InputRegistered` is the *same* durability state described in
[the orphan-CAS finding](durability-finding-orphan-raw-cas-is-unrecoverable-after-a-failed-registration-append.md):
input written, registration missing. Two independent reviews, over different
files and different context windows, arrived at the same underlying condition
from different directions — one at how it is created and never recovered, one
at how it is reported. That the codebase has a named stage for it, a named
error for it, and a test for it, while having neither a recovery path nor a
faithful message, suggests the state is understood as a boundary case but not
as a durability contract.

## Not done, deliberately

- No fix. Adding an `InputRegisteredIncomplete` variant is trivial; deciding
  whether it should be an error at all, or a resumable state like
  `ResumeRequired`, is not, and it interacts with the orphan-CAS finding.
- No caller survey. Establishing whether anything branches on the variant is
  the next step if this is picked up.
