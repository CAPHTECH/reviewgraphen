# Protocol

The unit is `(repository, base commit, realized fix commit, task kind, brief,
profile, oracle)`. The frozen repositories are reviewgraphen, fsl, and
casegraphen. Mechanical selection takes 10 `symbol_change` and 10 `symptom_fix`
units per repository (60 paired tasks); explicit symbol subjects are removed
from both selected and oracle exploration line sets.

The model identity for A/B is `Qwen3.8-27B-MLX-4bit`, temperature 0, one
trajectory, no retry. Each has cumulative limits of 65,536 serialized input
tokens, 24,000 raw output tokens, 1,800 seconds, 32 tool calls, and 16,384 bytes
per tool response. B's serialized packet costs input tokens and its
ingest/projection costs wall time. A/B share `list_paths`,
`search_text_or_identifier`, and `read_range`; no shell, log, network, tests,
compiler, future objects, oracle, or other arm output is exposed. C makes no
model call.

Primary outcomes are line-level F1, tokens per covered oracle line, elapsed
seconds, and seconds per covered oracle line. Zero coverage has infinite
efficiency cost. Declared loss is an honesty metric and never changes F1.
Reports contain paired tables plus median/IQR and win/tie/loss overall and by
repository/stratum. All inference is descriptive: no p-value, significance,
confidence interval, power claim, defect truth, or generalization.

Until the server exposes its tokenizer endpoint, input accounting is frozen to
`utf8_bytes_ceiling_div_4.v1`; this deterministic approximation is deliberately
declared in every harness measurement and cannot be presented as server usage.

The deterministic oracle forms the union of base symbols touched on the old
side, stable-key-matched base symbols touched on the fix side, and uniquely
base-resolved non-local references in admitted fix hunks. It excludes rather
than guesses unresolved or ambiguous semantics. Canonical JSON, sorted IDs,
source traces, exclusion ledgers, exact hashes, and typed failures are required.

The implementation admits product facts only through the repository-local
ReviewGraphen CLI whose executable SHA-256 is pinned in the evaluator. The
evaluator creates the request and private artifact root, verifies canonical
audit/manifest bytes, re-derives Git tree and snapshot identities from full
OIDs, and invokes the product schema validator. Each harness execution creates
a distinct measurement type and random HMAC key retained only by that run's
scoring scope; public scoring rejects supplied measurements. Arm B's product
wiring remains disabled until the reviewed task-subject entry exists and cannot
fall back to D.
