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

The implementation admits treatment facts only through the pinned
`reviewgraphen context` request-to-packet command. The request contains one full
base commit SHA, extractor identity, and exact accepted symbol IDs; it has no
task prose, hint, fix, D relation, or model-produced resolver output. The
evaluator reads pins only from the preregistration and, before context execution,
observes the binary digest, source commit/tree, rustc/cargo versions, schema-bound
profile/version/extractor identity, and the committed rule-set material digest.
It then verifies canonical request/packet/manifest, packet identity hash, base
snapshot/tree closure, product schema, and packet extractor echo before using the
packet. Packet bytes count through the common input-token budget.
The model-facing envelope removes the raw base commit field but preserves the
validated product context, source windows, IDs, hashes, losses, and denominators.
Product denominator commitments, declared losses, support-loss summaries,
latent cardinality, and unknowns pass unchanged into the authenticated harness
measurement. Each run retains its random HMAC key only in scoring scope; public
scoring rejects supplied measurements. These pins remain explicitly unfrozen.
