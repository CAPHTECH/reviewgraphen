# qwen_skill / claude_skill treatment text: location after working-tree removal

Status: append-only record. Nothing in `preregistration.json`,
`QWEN_SKILL_ARM_AMENDMENT.md`, `CLAUDE_SKILL_ARM_AMENDMENT.md`, or any run
record is changed by this note. Those documents describe the treatment as it
was when measured, and that description remains accurate.

## What changed

`.claude/skills/reviewgraphen-methodology/SKILL.md` was removed from the
working tree by operator decision, because it was still being offered to
agents as an invocable skill and competed with the current methodology skill
(`.claude/skills/obligation-review/`) for the same routing triggers.

**The treatment text itself is not lost.** It remains in git history at the
commit that froze it, and both pinned hashes reproduce exactly from there:

```bash
git show a092c32:.claude/skills/reviewgraphen-methodology/SKILL.md
# whole file  -> 1ddb0ed18503eb481b5936b9063e9c919f326d9a185dd1dab048db7b03fd2842
# body only   -> a43d6984f9b58d5e47256afed68cddc1598ae84d6fc564e05f1cf462228d670a
#   (body = awk 'BEGIN{c=0} /^---$/{c++; next} c>=2{print}')
```

Both were verified against this file's own recorded values immediately before
the removal commit.

## Effect on this experiment's artifacts

- `scripts/build_skill_packets.sh` reads `$root/.claude/skills/reviewgraphen-methodology/SKILL.md`
  and gates on the body hash. It is **left unmodified** — it is a
  preregistered experiment script and editing it would be a larger change to
  the record than the path break it would repair. It therefore no longer runs
  as written. To rebuild packets, materialize the body from the `git show`
  above first; the gate then passes unchanged, because the bytes are the same
  bytes.
- `benchmarks/m9-agentic-local-v1/` and `benchmarks/m10-target-context-local-v1/`
  pre/post-loop tree manifests record the whole-file hash `1ddb0ed1…` against
  `./.claude/skills/reviewgraphen-methodology/SKILL.md`. Those manifests are
  historical records of a tree state during a completed run, not re-runnable
  checks; they are accurate for the tree that existed then. A re-run of those
  loops today would produce a manifest without that line.
- Generation and judging conditions, unit selection, failure taxonomy, and
  every recorded outcome are unaffected: no trial is re-run, re-scored, or
  reclassified by this note.

## Current methodology skill

New review work uses `.claude/skills/obligation-review/`, which is a revision
of this treatment text, not a copy of it — it moves obligation enumeration off
the reviewer for the budget reason measured in this very experiment
(`OUTPUT_CAP_STOP_AND_PROBE.md:27-28`). It is not a substitute treatment for
any `qwen_skill` or `claude_skill` trial and must never be used to reproduce
one.
