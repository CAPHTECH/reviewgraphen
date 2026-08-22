# Genesis mapping proposal

- `case_space_id`: `case-space:m17-casegraphen-controlled-review-v1`
- `observed_revision_id`: none; the disposable experiment case space has not been created.
- Source acceptance units: exactly three governed work cells — `work:8bit-high-r1`, `work:4bit-low-r1`, and `work:aggregate`. The selector, projection, reviewer, validator, and judge micro-nodes map to their arm's single governed cell and remain detailed only in the external runtime trace.
- Proposed world anchors: frozen m15 card inventory, frozen `compose.rs` source hash, m16 aggregate hash, prompt hashes, and emitted artifact hashes.
- Information not representable without loss: ReviewGraphen obligation/claim/evidence state, projection source IDs, information-loss declarations, and coverage semantics remain in ReviewGraphen artifacts rather than CaseGraphen lifecycle fields.
- Review still required: genesis schema review, capability/actor assignment, topology review, and operation-gate review.
- No case-space mutation, review acceptance, topology acceptance, or plan acceptance was performed by this proposal.

The proposed CaseGraphen cells represent governed acceptance units, not individual model calls and not ReviewGraphen claims. A completed CaseGraphen cell means only that its runtime report was attached, reviewed under the declared authority, and transitioned; it does not mean the contained ReviewGraphen claims are evidence-supported, verified, or human-accepted.
