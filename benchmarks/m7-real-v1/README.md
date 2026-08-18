# M7 real regression corpus v1

This corpus contains 20 regression-fix pairs selected from the read-only FSL
history dated 2026-07-26 through 2026-08-08. Each positive is a fix commit's
first parent; its matched control is the fix commit itself. One replicate and
the B1/G3-proxy arms produce 80 prepared trials.

Every selected regression has machine-recorded evidence that the exact added
Rust integration test fails after being backported to the parent and passes at
the fix. Private location roots come only from production-code fix hunks. The
public snapshots contain the same selected production paths for both revisions
and exclude tests, commit messages, issue metadata, branch names, diffs,
oracles, candidate outputs, and previous pilot artifacts.

The `m7-real-production-paths-blind-redacted.v1` projection removes final
inline `#[cfg(test)]` modules, replaces contiguous production-comment blocks
containing issue references, and redacts residual issue numbers and Git branch
refs, all without changing line counts. Manifests declare this meaningful
information loss as `line_preserving_blind_metadata_and_test_redaction`;
private roots bind the projected bytes rather than the unredacted source bytes.

The eight-ID `mechanism_ontology.v1` was sufficient for all selected defects;
no ontology extension was made. Control findings remain unlabeled and require
blind adjudication under ADR 0026. They are not false positives and this corpus
does not estimate repository-wide precision.

The corpus is a retrospective sample of bugs already discovered and fixed. Its
detection rate therefore does not estimate discovery on unknown future bugs.

The measured replicate, raw model outputs, compact tool-call records, exact
blind-adjudication inputs, decisions, and research report are under
[`results/replicate-1`](results/replicate-1/). Full Codex transcripts are not
checked in; their hashes are bound by the execution records, while every shell
command is retained in JSONL. Full adjudication source copies are also omitted
because the exact bounded excerpts actually supplied to the adjudicator are
included.

## Attribution

The `public/snapshot-*/snapshot/rust/...` trees under this corpus are
unmodified source excerpts from [FSL](https://github.com/ymm-oss/fsl), a
separate public repository, used as real-code review targets. FSL is
licensed Apache License, Version 2.0; see [`/NOTICE`](../../NOTICE) at
the repository root for the full third-party attribution, including the
exact file list and scope.
