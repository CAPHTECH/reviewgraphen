# m21 context construction v1

Preregistered, execution-disabled evaluation of working-context construction.
It compares the same pinned Qwen model in A (minimal read tools) and B (the
same tools plus a task-bound, subject-first ReviewGraphen packet). C converts
that packet directly and is descriptive only. This benchmark is independent
of m20 and neither consumes nor modifies its units, packets, freezes, or data.

The oracle is derived mechanically from a historical fix after selection. The
model sees only the immutable base tree and frozen task brief. The oracle is a
realized-fix context oracle, not correctness truth or proof of necessity.

Run the stdlib-only verification (no model calls):

```sh
python3 -m unittest discover -s evaluator/tests -v
python3 -m evaluator dry-run evaluator/reference_vectors/task.v1.json /tmp/m21-request.json --arm A \
  --repository /absolute/base-repository --repository-id frozen-id --base-oid FULL_BASE_OID
```

Execution remains prohibited until every `TBD-before-freeze` field is pinned,
the 60-task manifest exists, all gates pass, and an independent custodian seals
the evaluator. Dry-run emits exact HTTP request bytes and `http_sent:false`.

Oracle construction uses its separately pinned repository-local debug binary.
Arm B uses only the release `reviewgraphen context` binary and complete product,
toolchain, and extractor identity pinned (but not frozen) in
`preregistration.json`; evaluator code contains no duplicate pin authority.
The evaluator constructs the closed request from an exact accepted base symbol;
task prose and hints never enter the product request. Both arms receive the same
single-commit shallow base export, with source remotes, ancestors, and future
objects absent. Product request, packet, manifest, snapshot/tree closure, schema
validation, and deterministic bytes are checked before its revision-free sealed
projection envelope reaches B.
