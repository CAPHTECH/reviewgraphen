# ADR 0025: M7 Detection Benchmark Contract

- Status: Accepted
- Date: 2026-08-13

## Context

M7 must measure detection rather than the apparent completeness of a report.
The current execution event contract admits only the deterministic fake reviewer;
provider output cannot be represented as an accepted claim, finding, evidence, or
sign-off without a separate execution-contract decision.

## Decision

M7 uses a separate, strict, non-authority artifact family.  A trial has one
immutable input revision and one opaque arm code.  The initial paired arms are
`b1_free_form` and `g3_proxy`; both emit exactly the same candidate
output schema.  G3 proxy alone additionally returns dispositions for the packet IDs it
was given.  No model/provider launcher is part of this contract.

The public trial manifest and candidate output never contain a mutation root,
expected mechanism, ground-truth label, another arm's output, or adjudication.
The private oracle is mounted only for deterministic scoring. Each root binds
the exact tree hash, file SHA-256, stable symbol, line span, and span SHA-256;
the scorer refuses malformed anchors. A clean control
has an empty oracle root set.  A runner must isolate each agent session from the
oracle, scoring outputs, other arms, and mutable repository state; this is a
filesystem/process boundary, not merely prompt text.

Candidate and oracle records are parsed with closed schemas.  Raw response bytes
are retained and hashed by the caller; parse failure and abstention are explicit
outcomes, never converted to an absence claim.  The deterministic matcher only
matches a finding when its cited location overlaps an oracle anchor and it shares
at least one normalized mechanism tag.  It may return unmatched candidates but
never decides semantic truth from rationale prose.  Duplicate findings for one
root yield one detected root.  Unmatched and ambiguous candidates are exported
under blinded identifiers for expert adjudication; adjudication is separate from
the injected-root recall denominator.

`g3_proxy` is not a full G3 condition unless it uses admitted
`ReviewContextEnvelope` artifacts. The initial packet implementation records
the required `non_authority_benchmark_packet` limitation and summaries label it
as proxy rather than G3.

The score is a non-authority research artifact.  It records manifest/candidate/
oracle hashes, root and candidate counts, exact detected root count, parse or
abstention outcome, and matched/unmatched counts.  Pair aggregation groups only
scores with the same `(unit_id, replicate)` and exactly one score from each arm.
It reports paired recall deltas and raw denominators; it does not make a
statistical significance claim.

## Consequences

- M7 artifacts cannot enter ProgramSpace, ReviewSpace, EvidenceSpace, the event
  journal, coverage numerator, report gate, or human acceptance path.
- Provider/model metadata, prompt packet hashes, and budget declarations are
  preserved in the manifest, but cannot be treated as execution authority.
- The schemas are v1 and closed.  A future generic provider execution contract
  requires its own ADR and migration/version decision.

## Protocol v2 hardening

`reviewgraphen.benchmark.mechanism_ontology.v1` is a closed, complete and
arm-neutral vocabulary. It is emitted in both agent inputs; candidates and
private oracle roots use only its IDs. Deterministic scoring remains limited to
source-location overlap plus exact ontology-ID intersection.

A v2 manifest binds an exact source inventory (path, SHA-256, line count) and
expected packet IDs. B1 has an empty expected packet set and must emit zero
obligation results. G3 proxy must emit exactly its complete expected packet set.
Any mismatch is recorded as `protocol_invalid`; raw output is retained, but the
trial has no detection denominator and is excluded from scores and paired
summaries. Oracle roots must bind the manifest input tree hash. Pair summaries
also require equal oracle, input-tree, protocol, and ontology hashes.

Blinded adjudication exports opaque item IDs and candidate content only. The
trial/finding reconciliation is a separate private artifact. Neither public
adjudication input nor adjudication result contains arm, trial, model, provider,
or oracle-root fields.

The pilot still lacks provider token telemetry and a durable raw-response CAS.
These are explicit limitations, not zero-valued metrics.


## Protocol v2 collection and run accounting amendment

Candidate outcome is limited to `structured`, `abstained`, and `parse_failure`.
`protocol_invalid` is not model-authored: only the collector emits it in a
separate `reviewgraphen.benchmark.collection.v1` record. A structured G3-proxy
candidate must return exactly the manifest packet IDs; structured B1 returns no
obligation results. Abstention and parse failure carry no findings or obligation
results, remain protocol-valid, and score zero detections against the oracle
root denominator.

Every prepared run emits a `trial_inventory.v1` before execution. Aggregation is
inventory-driven and reports prepared, valid, protocol-invalid, and missing
trial counts. A pair is eligible only when both expected arms have valid
collections and exactly bound scores. Invalid, missing, mismatched, or
incomplete pairs are excluded with typed reason counts. The legacy score-only
`summarize` interface is refused because it cannot prove that trials did not
silently disappear.

The manifest records provider, model, model revision (`unknown` is explicit),
reasoning effort, prompt-template version, tool-policy version, runner version,
inference settings, and a declared budget whose unknown values are represented
as `null` or the literal `unknown`, never invented numeric values. The
`paired_configuration_hash` excludes arm-specific packet hashes but binds the
input tree, source inventory, source-bundle hash, protocol/ontology versions,
and all shared execution settings. Scores carry this hash and paired summaries
require equality.

The private oracle carries top-level input-tree and source-bundle hashes even
for empty-root controls. Scoring requires equality with the manifest and checks
each root path, file hash, tree hash, and line range against the manifest source
inventory. Span SHA-256 and symbol validity cannot be recomputed without source
bytes at score time; they are therefore covered by the source-bundle hash and
must be verified when a new oracle is authored from the prepared source bundle.

Blind adjudication exports only unmatched or ambiguous findings from the
deterministic detailed-match record. Its public DTO contains locations and
controlled mechanism IDs only—no candidate-local ID, rationale, severity, arm,
model, or oracle data. A separate private reconciliation record is strictly
schema-checked and unique by both opaque item ID and `(trial_id,
finding_local_id)`.

The candidate schema expresses local outcome/disposition constraints, but JSON
Schema cannot express candidate-to-manifest packet equality, finding-reference
existence, manifest hash recomputation, oracle-to-inventory equality, or
inventory completeness. Those cross-record constraints are enforced by the
Rust validators and are required for collection, scoring, and aggregation.


## Verified adjudication export amendment

Blind export does not accept a caller-supplied detailed-match record. Its public
API requires the manifest, candidate, and private oracle, validates their
cross-record bindings, recomputes the deterministic detailed match internally,
and binds the resulting manifest, candidate, and oracle hashes before selecting
findings. Therefore a forged match list cannot mark an unmatched or ambiguous
finding as exactly matched and suppress expert adjudication. Exactly-one matches
are omitted; zero-match and multiple-match findings are exported through the
sanitized DTO.

Run status counts partition the complete prepared-trial inventory exactly:
`valid_collections + protocol_invalid_trials +
collection_binding_invalid_trials + missing_trials == prepared_trials`.
Collection/manifest binding failures are counted separately and exclude their
pair with a typed reason; they cannot disappear into the valid or missing
buckets.
