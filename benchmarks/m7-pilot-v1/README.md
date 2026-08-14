# M7 Pilot v1

This directory is a narrow, deterministic Rust pilot for evaluation plumbing.
Each public unit has a `base` and `head` revision and is independently
compilable. `public/` contains only packets and source material intended for a
review condition. `private/` contains the oracle and must never be included in
a reviewer packet.

The oracle is checked in for reproducibility, so this checkout is not an
isolation boundary. A trial runner must materialize a reviewer-only directory
from one `public/<case>/` packet, omit `private/` and repository-parent paths,
and use fork-none reviewers that receive only that emitted packet. A reviewer
with arbitrary repository access can inspect the oracle and invalidates a
blind trial.

`commitments.v1.json` pins the canonical private-oracle bytes. Run
`scripts/verify.sh` to compile and format-check every public revision and to
check that commitment.



## Protocol v2 compatibility

The checked-in v1 raw candidates, scores, summary, private oracle files, and
commitments are frozen historical evidence. They predate top-level oracle
`input_tree_hash`/`source_bundle_hash`, truthful execution configuration,
collector records, and the complete trial inventory. They must not be silently
rewritten or rescored as protocol v2. A future corpus version must prepare new
manifests with an explicit execution-config JSON file, emit `inventory.json`,
collect every expected trial, and use `summarize-run` with inventory,
collections, and scores.

Cross-record eligibility is determined by the Rust validator, not JSON Schema
alone. In particular, oracle roots must match manifest source paths, file
hashes, tree hash, and line bounds; the top-level source-bundle hash covers the
source bytes used to verify authored symbol/span anchors.
