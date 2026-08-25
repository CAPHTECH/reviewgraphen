# ADR 0039: Task-subject context projection boundary

- Status: Accepted for m21; implementation pending
- Date: 2026-08-26

## Context

M21 asks whether an agent can construct a precise working-context set more
efficiently when assisted by ReviewGraphen. Its unit begins at an immutable
pre-change snapshot and a task brief. The realized fix, its diff, and the
retrospective oracle are unavailable to both arms.

The existing generic-review boundary cannot express this treatment.
`reviewgraphen.generic_review_request.v3` is closed and carries no task subject,
and the generic runtime derives `context.subject_windows@3` subjects only from
the caller and callee of an admitted D obligation. Adding an arbitrary subject
to that path would either pretend that a task is a D relation or make a review
request produce context without its obligation/observer semantics.

## Decision

### 1. Use a separate context-projection command

Adopt a separate public library/CLI boundary, option (b), rather than
`generic_review_request.v4`:

```text
reviewgraphen context --request REQUEST --output NEW_OUTPUT_ROOT
```

Its closed, versioned wire contracts are
`reviewgraphen.context_request.v1` and
`reviewgraphen.context_packet.v1`. It does not construct a ReviewObligation,
ReviewPlan, observer invocation, ReviewClaim, Evidence, Verification, Finding,
Decision, human report, or generic-review run. Existing request/run v1–v3
bytes and decoders remain unchanged, and every cross-decode fails.

The request binds one immutable base snapshot, profile and extractor identities,
`context.task_subject_windows@1`, explicit resource limits, and one closed
task-subject outcome:

- `resolved` contains a nonempty sorted unique set of exact accepted
  base-snapshot symbol/artifact IDs;
- `unresolved` or `ambiguous` contains no guessed ID and records the attempted
  task identifiers plus source IDs.

Free-form hints remain in the m21 task brief. They are not a product selector,
StableId preimage, resolver input, or recovery mechanism. M21's separately
frozen, deterministic task binder produces `m21.task_subject_binding.v1` from
accepted base ProgramSpace facts: exact supplied IDs for `symbol_change`, and
the preregistered exact-name/normalized-lexical rules for `symptom_fix`. The
product validates snapshot membership and accepted state; it never asks a
model to resolve a subject. Zero or multiple matches remain typed loss.

`context.task_subject_windows@1` is a new policy. It reuses the subject-first
invariants of `context.subject_windows@3`, not its D-specific type or policy
ID. It reserves all resolved subjects, traverses only the versioned non-D
task-neighborhood relations, and materializes only subject/reached files. The
packet contains canonical source IDs/ranges/hashes; snapshot/profile/extractor/
policy IDs and hashes; exact known denominator counts and sorted-ID-set
commitments; known-versus-unknown cardinality; and reason-partitioned declared
loss counts/digests with recovery references. An unresolved subject produces a
closed empty-source packet with the corresponding loss, never a fabricated
denominator or implicit full-coverage claim.

The packet is a non-authority Projection and carries no coverage claim or
review outcome. Its denominator-cardinality and loss states distinguish known,
partial, and unknown construction without promotion to a claim, evidence,
verification, or acceptance record. Raw task/model prose is not canonical
program state. Program facts remain accepted only through the normal
deterministic ingest boundary.

### 2. Isolate m21 from m20

M21 product work must occur on a dedicated branch and separate worktree forked
from a declared immutable commit, with a separate `CARGO_TARGET_DIR`. M21 pins
the branch commit/tree, `Cargo.lock`, Rust toolchain, schemas, policy/binder
bytes, and final executable SHA-256 before its corpus or model execution. M20
continues from its own sealed worktree, target directory, evaluator bundle, and
fixed executable/source identities; no m21 binary or source path may be mounted
or imported there.

Therefore this product change is not included in the pending m20 seal and does
not delay or invalidate m20 Stage 0/1. Merging implementation into the m20
worktree before m20 becomes terminal is forbidden. A later mainline merge does
not retroactively alter a completed, hash-bound m20 run. M21 remains prohibited
from execution until m20 reaches its already declared terminal boundary.

### 3. Define the fair m21 arm contrast

M20's shared diff/callee core does not apply. M21 starts before the realized
change, so admitting its diff, changed callee, updated tests, or any future-tree
bytes would leak the oracle.

- Arm A receives the task brief, base snapshot, and the frozen minimal
  read-only `list_paths`, `search`, and `read_range` tools.
- Arm B receives the identical brief, snapshot, tools, and limits, plus only
  the sealed task-subject packet described above.
- Optional arm C receives that packet without a model and remains descriptive.

Both model arms have the same total token and active wall-clock ceilings. B's
packet serialization counts as model input, and its arm-specific task binding,
incremental ingest, projection, and packet construction count against B's
wall-clock/cost ledger. A's tool queries/results count against A. A common
immutable checkout or index may be excluded only when byte-identical and
actually shared by both arms; every arm-specific preprocessing cost is charged.
The subject span supplied in a `symbol_change` brief remains removed from the
oracle exploration denominator for both arms. B receives no free m20 core.

This estimand is the end-to-end incremental utility and cost of task-subject
projection, not a comparison of unequal knowledge about a future change.

## Required verification

Before m21 freeze, tests must prove deterministic clean rebuilds; stable packet
bytes and IDs; rejection of foreign snapshots, unknown/nonaccepted/duplicate
subjects, v3 generic-review requests, D-only relations, and future-tree input;
typed unresolved/ambiguous/partial/macro loss; exact denominator/loss closure;
absence of claim/evidence promotion; and identical A/B access to base tools.
Mutations that omit packet/construction cost, admit a hint into resolution, or
leak realized-fix bytes must fail. These tests and the m21 build hash are m21
artifacts only and create no m20 re-seal obligation.

## Consequences

The new command is useful outside m21 without turning context construction into
review. Callers must provide or deterministically derive exact accepted task
subjects; the product will not infer intent from prose. M21 gains an executable,
auditable B arm while retaining declared loss, explicit denominators, and the
fact/claim/evidence authority boundaries.
