# M7 HEAD Local v1

> **Status: preregistered, not yet executed.** Nothing in this directory
> reflects a real generation or judge call. `units.json` and
> `preregistration.json` are frozen; execution requires operator approval.

This experiment redirects the local-LLM benchmark program from re-detecting
known, previously-planted defects (`m7-local-factorial-v1/v2/v3`) to
searching for **previously undiscovered** defects in the live
`/home/rizumita/github/fsl` HEAD. It answers a narrower, more directly
useful version of the question `m7-local-factorial-v3` was built for: does
ReviewGraphen, running a local model (`qwen3.8:27b-mlx`), surface findings
on real, unexamined code that a second, independent model would judge worth
a maintainer's attention — and does the ReviewGraphen scaffold change that
relative to unscaffolded qwen?

`fsl` is read-only throughout. No issue, branch, commit, patch, or pull
request is ever created. There is no ground truth here (unlike the
`m7-real-v1`/`m7-local-factorial-*` known-target lines): a blind, cross-family
Claude judge (opus, high effort) labels each pooled finding
`issue_should_be_created`, `should_not_be_created`, or `unable_to_determine`,
following the non-authority disposition design of ADR 0035
(`docs/adr/0035-m7-head-cross-family-issue-worthiness-study.md`), without
that ADR's matched-pair calibration gate (there is no known-defect set to
calibrate against for live-HEAD findings).

- **Units**: 9, deterministically selected from FSL's Rust production
  source at HEAD `e589014`. See `UNIT_SELECTION.md` for the algorithm and
  `units.json` for the exact, hash-pinned result.
- **Arms**: `qwen_b1` (raw source, no scaffold) vs `qwen_full` (source +
  ReviewGraphen scaffold) — the same file set per unit, both arms. `qwen_b1`
  is included as the necessary comparator; the operator's stated interest is
  primarily in `qwen_full`.
- **Generation condition**: unchanged from `m7-local-factorial-v3`'s closed,
  frozen condition (LM Studio, `reasoning_effort=none`, sequential,
  3,600,000 ms idle timeout, zero retries). See `preregistration.json`
  `generation_execution_condition`.
- **Judging**: blind, cross-family, isolated (`--no-session-persistence
  --tools ""`), pooled findings from both arms per unit with arm identity
  removed before the judge ever sees them. Full prompt and blinding
  mechanism in `JUDGE_PROTOCOL.md`.
- **Predecessors preserved, not rewritten**: `m7-head-issue-v1` (interrupted
  before its own calibration ran; its ADR, preregistration, and scripts are
  read for design precedent only) and `m7-local-factorial-v3` (closed with a
  failed 4/4 schema gate; its execution condition and failure taxonomy are
  reused here unchanged).

Known limitations (no ground truth, single non-calibrated judge, both
generator and judge are LLMs, small exploratory n, four oversized files
excluded from scope, an untested judge backend, and an expected-low
generation completion rate carried over from `m7-local-factorial-v3`) are
listed in full in `preregistration.json` `known_limitations`.
