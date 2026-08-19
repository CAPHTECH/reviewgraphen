# Harness bring-up log

Recorded because two of these were real bugs that my own guards caught, and
because the first one made a **zero-server-contact** failure that could
otherwise be mistaken for a trial.

## 1. First launch: refused, zero generation requests

`scripts/make_scratch.sh` refused with
`pinned archive unexpectedly contains benchmarks/`. The guard was right and
my assumption was wrong: the pinned revision `05573bb` predates
`benchmarks/m8-impl-local-v1` but **not** `benchmarks/m7-head-local-v1`, so
the archive carried the entire prior campaign — including its
preregistration, its results, and its judge protocol.

That is not the acceptance test, but it is the experimental setup, and an
agent that reads it learns it is being measured. `benchmarks/` is not a
cargo workspace member, so it is now removed from every working copy rather
than tolerated.

`claude` then also failed to launch inside the sandbox (`exit 127`,
`timeout: failed to run command 'claude'`) because its install directory was
not on the sandbox `PATH`.

**No generation request was issued.** `stream.jsonl` was 0 bytes and the
binary never started. This is a harness bring-up failure before the
experiment began, not a trial, and it does not count against the
preregistered maximum of 6 trials. The no-retry standing order governs
upstream failures; nothing upstream was contacted.

## 2. Sandbox probes

Two minimal probes, trivial prompts, no task content — not trials.

| probe | result |
| --- | --- |
| 1 | 4 turns, model `qwen3.8:27b-mlx`, but **Bash unusable**: `EROFS: read-only file system, mkdir '/tmp/claude-1000/-tmp-m9-brt'`. Claude Code needs a writable scratchpad; `--ro-bind / /` had made `/tmp` read-only. Notable that the agent reported the tool failure accurately rather than claiming success. |
| 2 | 2 turns, `cargo --version` executed inside the sandbox, returned `cargo 1.95.0 (f2d3ce0bd 2026-03-21)` — the pinned toolchain. |

Fix: `--tmpfs /tmp/claude-1000` inside the sandbox, so the client gets a
writable scratchpad that exists only for the trial.

## 3. Confirmed before the first trial

- The real repository is invisible inside the sandbox (`--tmpfs` over it).
- `benchmarks/` is absent from every working copy.
- The acceptance test is absent from every working copy, checked by name.
- `cargo` runs, on the pinned 1.95.0 toolchain, in a private target dir.
- The model reported in the transcript is `qwen3.8:27b-mlx`.
- The client reports `maxOutputTokens: 32000`, matching the declared
  deviation in `preregistration.json`.
