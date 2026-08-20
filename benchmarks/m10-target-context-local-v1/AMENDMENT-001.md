# AMENDMENT-001 — projection-use detector excludes help probes

Date: 2026-08-20. Frozen while `projection-1` was in progress, before its
first edit, before any mechanically checked outcome, and before any later
trial started.

The agent ran `reviewgraphen-context --help`. The original detector counted
any Bash command containing the executable name, so it would have called this
intervention use even though the wrapper returned usage error 64 and no
projection was shown. That contradicts the preregistered rule, which requires
an invocation "using a task-selected symbol."

`check_projection_use.py` now counts only a command that passes either the
supported short selector `visit_block` or its exact accepted ProgramSpace
label. A help probe remains in the transcript but does not satisfy the gate.
Nothing about the task, prompts, projection bytes, backend, model, order,
timeout, verifier, or outcome thresholds changes. This amendment was made
from observed tool syntax before seeing code or outcome, not in response to a
trial result.
