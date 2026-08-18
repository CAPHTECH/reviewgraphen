# Candidate-file uniqueness and determinism — verified before any judge call

Status: answers the operator's four specific questions, verified from the
filesystem and hashes, before building the final judge pool. No judge
call has been issued yet.

## 1. Does pool construction reference a candidate.json uniquely per (unit, arm)?

**Yes, by construction, not by convention.** `POOL_SOURCE_MANIFEST.json`
in this directory is a hand-authored, fixed list of exact file paths and
their expected SHA-256 hashes — one entry per (unit, arm) pair with a
judge call. The pool builder (`scripts/build_final_judge_pool.py`, added
alongside this report) reads **only** this manifest; it performs **no
directory glob, no "latest file," no filesystem search of any kind**. It
hashes each referenced file and aborts if the hash doesn't match the
manifest. There is no code path by which it could pick a different file
than the one named in the manifest.

## 2. Is the selection rule, where multiple files exist, deterministic — independent of execution order or filesystem search order?

**Yes.** An exhaustive sweep of every `candidate.json` under every
`m7-head-local-v1-skill-{gen,retry}-*` run directory (9 gen/retry
directories checked, every unit) found exactly **one** case of multiple
files for the same (unit, arm): `head-local-00`/`qwen_skill`, with two
candidates:

| Attempt | `max_output_tokens` | `provider_output_tokens` | SHA-256 | findings |
| --- | --- | --- | --- | --- |
| gen-1 | 65,536 (superseded) | 64,421 | `49d47fb3...8386c` | 4 |
| gen-2 | 131,072 (current) | 72,863 | `a3c8d11f...70421` | 4 |

**The selection is not a new decision made to resolve this report** — it
was already fixed by `RESTART_PROTOCOL_AND_UPSTREAM_HISTORY.md` section
1, written before this pool was ever assembled: `head-local-00` ran
under both the old and current execution conditions; the old-condition
result is superseded and not used for anything past that section's
causal-proof table. gen-2 (131,072 cap, matching every other unit's
condition) is canonical. The manifest hardcodes gen-2's exact path and
hash; gen-1's file is left untouched on disk but is not referenced
anywhere in the pool-construction path.

**Every other (unit, arm) pair has exactly one candidate.json in
existence, confirmed by the same exhaustive sweep** — no other case of
this ambiguity exists anywhere in this experiment's current state.

## 3. What was the file the operator read with 6 findings?

**Not identified — and not reproducible from anything currently on
disk.** Every `claude_skill` artifact for `head-local-00` was checked:

- The only run directory that ever existed:
  `/tmp/m7-head-local-v1-runs/m7-head-local-v1-claude-skill-gen-1/head-local-00/` — one `process-record.json`, no attempt-numbered subdirectories (the earlier failed provider-constrained attempt errored with a 400 **before any output file was ever created** — confirmed in `CLAUDE_SKILL_ARM_AMENDMENT.md`'s commit history — so it left no candidate.json anywhere to be confused with).
- The committed diagnostics copy:
  `benchmarks/m7-head-local-v1/diagnostics/claude-skill-generation-head-local-00-01-02/head-local-00/candidate.json`.
- The `/tmp` extraction file I built while first reporting this arm's
  results: `/tmp/m7-head-local-v1-claude-skill-candidate-head-local-00.json`.

**All three are byte-identical** (SHA-256
`73e032b5d03295fa15a1d3ec0b1c0099c72943ec91a847e119f6a33b878d5f68`), and
this hash matches `raw_response`'s own hash inside `process-record.json`
directly (verified by hashing the raw response bytes independently, not
by trusting the extraction step). There is no fourth copy anywhere.
`git log` shows this file was committed exactly once, never rewritten.

**9 is the only finding count that exists anywhere for
`claude_skill`/`head-local-00`.** I cannot identify what produced "6" —
it does not match this unit's obligation count (8), its `issue_present`
tally (7), or any subset I can construct from the actual data. If you
still have the source you read, I'd want to see it; otherwise this may
have been a miscount or a mixup with a different unit/arm's number while
reading — `head-local-02`'s `claude_skill` count is 6, which is the
closest match in this experiment's actual data.

## 4. Hash/path proof that the pool's 40 findings are genuine successful-attempt output

Every entry in `POOL_SOURCE_MANIFEST.json` carries its source path and
SHA-256. Summary:

| unit | arm | SHA-256 | findings |
| --- | --- | --- | --- |
| head-local-00 | qwen_skill | `a3c8d11f...70421` | 4 |
| head-local-00 | claude_skill | `73e032b5...5f68` | 9 |
| head-local-01 | qwen_skill | `a6c04c77...de43e` | 5 |
| head-local-01 | claude_skill | `d2668a02...9de43` | 5 |
| head-local-02 | qwen_skill | `3eceef21...e20e7` | 4 |
| head-local-02 | claude_skill | `3782984e...e8842` | 6 |
| head-local-03 | qwen_skill | `820f97bf...ae71b` | 1 |
| head-local-04 | qwen_skill | `698b6129...4e2fd` | 2 |
| head-local-05 | qwen_skill | `1927cb68...508c9` | 1 |
| head-local-06 | qwen_skill | `528674eb...568d9` | 3 |

Total: 20 (qwen_skill) + 20 (claude_skill) = **40 findings**, matching
the operator's own confirmed tally. Every referenced file's
`generation-metrics.json`/`transport-record.json` sibling confirms
`failure_class: valid` / `outcome: structured` — none of these ten files
comes from an excluded or failed attempt.

## Rule, fixed before pool construction, for future units if this ever recurs

If a future amendment causes a genuine second valid attempt to exist for
the same (unit, arm) under the **same** execution condition (not yet
observed in this experiment — the one observed case, `head-local-00`, is
resolved by *different conditions*, not by arbitrary choice among
identical-condition duplicates): the canonical attempt is the one
explicitly named in the most recent dated amendment document governing
that unit's re-run (e.g. `RESTART_PROTOCOL_AND_UPSTREAM_HISTORY.md`),
never "most recently modified file" or "first found by glob." Any such
case must be added to `POOL_SOURCE_MANIFEST.json` with its provenance
stated the same way as the `head-local-00` entry above, and reported to
the operator before the pool composition changes as a result.
