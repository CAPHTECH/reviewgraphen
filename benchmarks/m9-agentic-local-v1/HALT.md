# m9-agentic-local-v1 — halted during bring-up

Halted 2026-08-19T15:37 +09:00, before any valid trial, because the local
server's backend changed underneath the experiment. Full record:
`../BACKEND-CHANGE-2026-08-19.md`.

## State at the halt

| item | state |
| --- | --- |
| `preregistration.json` | frozen and committed before any request |
| harness | brought up and verified (`BRINGUP.md`) |
| valid trials completed | **0 of 6** |
| `skill-1` | started 15:27:58, killed mid-loop by the halt — **void** |
| trials 2-6 | never started |
| provider requests since the halt instruction | **none** |

`skill-1` was terminated by me, not by the model, the server, or its own
timeout. It is not a `loop_incomplete_*` outcome — that classification is
for a loop that ended on its own — and it does not count against the
preregistered maximum of 6 trials. Its artifacts are kept as a record that
it existed and are not to be analysed as a result. Which backend it ran
against is not determinable and is not being determined.

## What is not affected

The preregistration, the prompts, the sandbox recipe, the verifier, and the
loop analyser are all complete, committed, and hash-pinned. Nothing about
them needs revisiting for a restored backend. If the operator restores the
previous backend, m9 resumes at trial 1 unchanged.

If instead the new backend becomes the condition, m9 needs a fresh
preregistration rather than an amendment: the model would differ, so the
existing document's execution conditions would be false, and its results
could not sit alongside m8's.

## One harness change this argues for, not yet made

`scripts/run_trial.sh` does not capture `/v1/models` before each trial.
m8's `run_generation.py` did, and that listing is the one signal that would
have detected this change. Capturing **and gating on** a hash of it is the
concrete fix, recorded in `../BACKEND-CHANGE-2026-08-19.md` section 3.2.
It is not implemented here because implementing it now would presume an
answer to a question the operator has not yet decided.
