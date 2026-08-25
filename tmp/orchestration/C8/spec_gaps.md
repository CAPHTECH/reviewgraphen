# C8 specification/API gap

Status: resolved; C8 consumes C7's validated v2 audit schema and read-only
semantic decoder.

The C8 human-report manifest is required by ADR-0038 §8.4 to be a deterministic
projection of a *validated* complete `reviewgraphen.generic_review_run.v2`
canonical audit JSON document.  The currently available public runtime API only
defines `reviewgraphen.generic_review_run.v1` (`GenericReviewRun` in
`crates/reviewgraphen-runtime/src/generic.rs`).  The required v2 canonical type,
closed JSON schema, and semantic validator/replay API are C7-owned and are not
available without editing the explicitly forbidden runtime crate.

This initial API gap is resolved: C7 now provides the v2 schema and
`decode_and_validate_generic_review_run_v2` / `validate_generic_review_run_v2_semantics`.

## Remaining blocking data-model gap

The C7 `reviewgraphen.generic_review_run.v2` schema's closed `observations`
union has only `kind = "deterministic_abstain"`.  It carries no proposed-claim,
malformed-output, or provider-failure row.  Consequently C8 cannot produce the
required §8.4 projection snapshots for proposal, malformed, and provider-failure
states from a validated canonical audit, nor can its manifest truthfully include
the required proposed claims.

Options considered:

1. Add report-owned proposal/malformed/provider fields or accept them as a
   second report input. Rejected: they would not be derivable from the audit
   JSON/hash and would turn the report into a state-minting input rather than a
   projection.
2. Infer those states from text or missing fields. Rejected: this fabricates
   review claims/status and conflicts with the closed v2 schema and deterministic
   validation boundary.
3. Emit an abstention-only C8 report and omit the required state tests. Rejected:
   it does not meet the C8 verification contract.

C7 extended the validated run-v2 observation union with closed proposed-claim,
malformed, and provider-failure variants. C8 now projects each only from the
canonical audit bytes and retains audit hash/source closure.
