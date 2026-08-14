# Generic review orchestration verification

Measured on 2026-08-14 against implementation commit `3c9352e`.

## Outcome

The product review surface is now generic. It accepts an ordinary Git
repository plus immutable base/target revisions, ingests and synthesizes the
obligation universe, plans work, constructs source-bound contexts, invokes an
isolated reviewer, parses only closed structured output, and emits an
explicitly non-authority run record. The former fixed `double-submit` command
is absent; all `review --fixture ...` forms are rejected before input access.

This measurement did not detect a bug. The full Codex run executed all five
derived obligations and produced zero proposed claims: two responses were
valid abstentions and three were rejected as malformed because they contained
both an abstention and claims. This establishes execution of the generic path,
not detection effectiveness, soundness, or safety.

## Mechanical boundaries

- The only successful CLI form is implemented at
  `crates/reviewgraphen-cli/src/lib.rs:47-89`; fixed-fixture rejection is tested
  at `crates/reviewgraphen-cli/src/lib.rs:319-337`.
- Generic orchestration and semantic re-derivation start at
  `crates/reviewgraphen-runtime/src/generic.rs:298` and
  `crates/reviewgraphen-runtime/src/generic.rs:346`.
- The authority ceiling is constructed with `trusted_pass = false` at
  `crates/reviewgraphen-runtime/src/generic.rs:1130-1153`.
- Codex tool-bearing features are explicitly disabled at
  `crates/reviewgraphen-reviewer/src/process.rs:23-40` and the JSONL event
  allow-list rejects tool and unknown events at
  `crates/reviewgraphen-reviewer/src/process.rs:632-670`.
- The selected CLI executable's actual version is observed rather than guessed
  at `crates/reviewgraphen-reviewer/src/process.rs:594-630`.
- The ordinary-Git replay/negative test is at
  `crates/reviewgraphen-runtime/src/generic.rs:1331-1421`.

## Live Codex measurement

Input was an ordinary temporary Git repository. The base revision
`63e3e987df00ffcaa4fdacd98b9a64043b29a299` contained `pub fn submit()`;
target `7384ae4b67a4ccdd87971141f051a181c0697941` contained
`pub async fn submit()`.

Codex CLI 0.147.0 ran `gpt-5.6-sol` with high reasoning under the bwrap and
zero-tool policy. The event allow-list passed. Counts were denominator 5,
planned 5, executed 5, abstained 2, malformed 3, structured 0, provider
failure 0, deferred 0, and proposed claims 0. The result remained
`non_authority`, `incomplete`, and `trusted_pass=false`.

Replaying the five recorded raw responses rebuilt the exact same canonical run
bytes. Both live and replay SHA-256 were
`7441eec2a78e066d4819761109def98425be21cd6a3a1a59883b0ed31a3c1fb8`.
The checked-in `codex-full-run.json` is that canonical live output.

## Claude limitation

The current executable reported Claude Code 2.1.231, correcting the earlier
environment observation of 2.1.227. Adapter and boundary tests passed, but the
live invocation failed before a model response with `Failed to authenticate:
OAuth session expired and could not be refreshed`. Therefore successful live
Claude output and Claude replay were not verified.

## Verification gates

The following passed after the implementation and boundary corrections:

- `cargo fmt --all --check`
- `cargo build --workspace`
- `cargo test --workspace` with the trusted Cargo executable explicitly set
- `cargo clippy --workspace --all-targets -- -D warnings`
- `python3 scripts/validate_bundle.py`
- `git diff --check`

Existing M7 benchmark/result bundles were not modified.
