# Posted record — ymm-oss/fsl issues from m7-head-local-v1's 5 cross-arm findings

Status: all 5 drafts posted, operator-approved, `gh issue create --body-file`
used verbatim (no rewriting at post time). Each entry below records the
issue number, URL, creation timestamp (from GitHub's own `createdAt`,
not a local clock), and the SHA-256 of both the draft file and the
actually-posted body — fetched back from GitHub after posting, not
assumed — so a later body edit on GitHub can be detected against what
was posted here. `gh issue create`/`gh issue view` were the only GitHub
write/read operations used; no branch, commit, patch, or pull request
was created against `fsl`, per the still-standing constraint in
`upstream-issues/CONSTRAINT_CHANGE.md`.

Duplicate check before posting: `gh issue list --search` for terms
specific to each finding (`column_member`, `rewrite_compose_statements`,
`source_origin`, `init_write_key`/"assigned more than once init",
LSP keyword/completion terms) against `ymm-oss/fsl`'s issues (all
states). No clear duplicate found for any of the 5; one tangentially
related closed issue (#475, a different db-lowering double-assignment
regression tied to rename/split/merge, already fixed) was checked and
is not the same claim as finding #1.

| # | Draft | Issue | URL | Created (UTC) | Draft SHA-256 | Posted-body SHA-256 | Match |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `01-db-column-member-collision.md` | #817 | https://github.com/ymm-oss/fsl/issues/817 | 2026-08-18T08:05:08Z | `f9570833b00bd2fea5dfea17b17a1ac52e4eb192fe8f1f53b7b01a019c4e71f3` | `f9570833b00bd2fea5dfea17b17a1ac52e4eb192fe8f1f53b7b01a019c4e71f3` | identical |
| 2 | `02-compose-alias-panic.md` | #818 | https://github.com/ymm-oss/fsl/issues/818 | 2026-08-18T08:05:25Z | `1defd5ba5b0cc29a2fd67a090ff56380d24b6edf62ded27dfb1f00535f2ae2c0` | `1defd5ba5b0cc29a2fd67a090ff56380d24b6edf62ded27dfb1f00535f2ae2c0` | identical |
| 3 | `03-source-origin-hardcoded-prefix.md` | #820 | https://github.com/ymm-oss/fsl/issues/820 | 2026-08-18T08:05:40Z | `0b15838e448b35d5517b1d3f1f327b897127e4898f316ff44c2a175166598a76` | `0b15838e448b35d5517b1d3f1f327b897127e4898f316ff44c2a175166598a76` | identical |
| 4 | `04-init-write-key-coverage-granularity.md` | #821 | https://github.com/ymm-oss/fsl/issues/821 | 2026-08-18T08:06:04Z | `ec8994e32ce3200bd5d808d513ddbb4b5f239c6fb4a45970b872442b4061cebe` | `ec8994e32ce3200bd5d808d513ddbb4b5f239c6fb4a45970b872442b4061cebe` | identical |
| 5 | `05-lsp-keywords-missing-use.md` | #822 | https://github.com/ymm-oss/fsl/issues/822 | 2026-08-18T08:06:17Z | `0370effc542b8b3b6dcb69e20fd9b5bc50ff2588b6cb25c51ad6a503b1c5c7b6` | `0370effc542b8b3b6dcb69e20fd9b5bc50ff2588b6cb25c51ad6a503b1c5c7b6` | identical |

All 5: `labels: []`, `assignees: []`, `milestone: None` — confirmed by
`gh issue view --json labels,assignees,milestone` after posting, not
assumed from the create command's absence of flags.

## Note on issue #819

Issue numbers jump from #818 to #820 in the table above. #819 was
created by the operator (`rizumita`) directly, at essentially the same
timestamp (`2026-08-18T08:05:37Z`), on an unrelated topic (FIFO oracle
cleanup hardening) — confirmed by `gh issue view 819`, not assumed. This
is a coincidental interleaving with the operator's own concurrent
activity, not a duplicate, error, or conflict from this posting run.

## No anomalies during posting

No permission error, no forced issue template, no duplicate-detection
prompt, no unexpected redirect. All 5 `gh issue create` calls returned a
plain issue URL on the first attempt.
