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

Oracle and treatment construction invoke only the repository-local
`target/debug/reviewgraphen` binary pinned by SHA-256 in `evaluator/product.py`.
No CLI path, product audit, packet JSON, or artifact root is accepted from the
command line. Both arms require evaluator-only repository/base arguments and
receive the same single-commit shallow base export; source remotes, ancestors,
and future objects are absent. Arm B remains typed-unavailable until the
separately reviewed product task-subject entry replaces the rejected D path.
