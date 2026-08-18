# m8-impl-local-v1

Can `qwen3.8:27b-mlx` do **implementation** work when given a
ReviewGraphen-style methodology, the way `m7-head-local-v1` showed it could
do review work?

Three questions only:

1. Does it terminate?
2. Does it produce code that compiles?
3. Do the tests pass?

`preregistration.json` is frozen before the first generation request and is
the authority for the task, the skill hash, the arms, the success criteria,
and the stopping rules. Results are recorded in `RESULTS.md` and under
`runs/`; the preregistration is never edited after freezing.

## Layout

| Path | What it is |
| --- | --- |
| `preregistration.json` | Frozen design. Read this first. |
| `task/TASK.md` | The specification, byte-identical in both arms. |
| `task/review_flag_order.rs` | Harness-owned acceptance test. Red on the pinned revision, green after a correct change. Never given to a candidate as editable. |
| `task/PINNED_REVISION` | The revision every scratch copy is made from. |
| `skill/IMPLEMENTATION_SKILL.md` | The frozen implementation methodology, derived from `docs/03,06,07,08,09,10` and ADR 0011. |
| `schemas/` | The candidate output schema both arms must satisfy. |
| `scripts/build_packets.py` | Builds the two packets. The only difference between them is the skill body. |
| `scripts/run_generation.py` | One request, no retries, raw SSE captured, metrics derived. |
| `scripts/make_scratch.sh` | Fresh tracked-files-only copy of the pinned revision under `/tmp`. |
| `scripts/apply_and_verify.py` | Applies a candidate's edits to a scratch copy and runs build + test. |
| `scripts/make_reference_candidate.py` | Harness self-test with a known-correct solution. Not an arm. |
| `scripts/extract_candidate_json.py` | Reused byte-for-byte from `m7-local-factorial-v3` (ADR 0037 extraction contract). |
| `runs/` | Per-arm result directories. |

## Isolation

The model never gets a shell, a filesystem, or an agent loop. It emits one
JSON object describing an edit; the harness applies that edit to a scratch
copy created fresh under `/tmp` by `git archive`. The real worktree is never
a write target, so a silent truncation cannot leave anything half-edited.
