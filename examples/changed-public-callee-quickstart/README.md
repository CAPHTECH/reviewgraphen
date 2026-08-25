# Changed public callee quickstart

Run these commands from the top-level repository clone. The request is
provider-free: `deterministic.abstain@1` records an abstention and does not
make a claim, run a verifier, or provide authority for a sign-off.

```text
cargo build --locked -p reviewgraphen-cli
# exit 0
target/debug/reviewgraphen review --request examples/changed-public-callee-quickstart/request.v3.json --artifacts .reviewgraphen-quickstart-output
# exit 0
python3 examples/changed-public-callee-quickstart/verify.py --expected examples/changed-public-callee-quickstart/expected-hashes.json --output-root .reviewgraphen-quickstart-output
# exit 0
target/debug/reviewgraphen review --request examples/changed-public-callee-quickstart/request.v3.json --artifacts .reviewgraphen-quickstart-output
# exit 20
python3 examples/changed-public-callee-quickstart/verify.py --expected examples/changed-public-callee-quickstart/expected-hashes.json --output-root .reviewgraphen-quickstart-output
# exit 0
```

The second review command refuses the already-existing output root before any
ingest, observer, or artifact write. `verify.py` is read-only and checks the
literal request and artifact hashes, canonical JSON bytes, file lengths, and
the exact artifact set.

For the second-run immutability check, capture every artifact's bytes and
nanosecond mtime before the second review command and compare them afterward;
the command must exit 20 and both maps must remain exactly equal. This check is
performed by the repository's quickstart integration test; `verify.py` remains
strictly read-only and independently rechecks the pinned bytes after either
invocation.

Do not use a bare `cargo test --workspace` as the ingestion test command in
this repository: the ten `tests/m2.rs` cases fail without the ADR 0012 test
environment. Use `mise run test-ingest`, which supplies that contract.
