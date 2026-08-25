# C3 handoff: `context.subject_windows@2` / `context.subject_windows@3`

The C7 integration point is intended to be:

```rust
prepare_subject_windows_v2(
    aggregate: &ReviewAggregate,
    obligation_id: StableId,
    caller_artifact_id: StableId,
    callee_artifact_id: StableId,
) -> Result<ContextSubjectWindowsSessionV2, ContextError>
```

`caller_artifact_id` and `callee_artifact_id` are explicit ProgramSpace artifact
IDs. Before any source request, C3 verifies that the obligation is the
`relation.changed_public_callee@1` / `rust.callee_contract_review@1` substantive
D obligation with one relation target, that the accepted relation has one
target, and that these IDs exactly equal its accepted source and sole target.
Wrong or non-accepted IDs fail with `ContextSubjectBindingErrorV2`; they are
never converted into a `MissingSource` subject loss.

After that gate, the session mirrors the v1 ordered resolver protocol: callers
obtain ordered source requests, submit exact registered bytes, then finish to
receive the window projection. It must retain both validated subject endpoints
as an admitted window or a typed subject loss.

Policy identity is `context.subject_windows@2`; its required canonical hash is
`sha256:7c4ceca165588cd38b28cc6882bb68a1ff4040dbb19ee34dfd792216d70bbf26`.

## Request-v3 integration

New D executions use only this separate Core entry point:

```rust
prepare_subject_windows_v3(
    aggregate: &ReviewAggregate,
    obligation_id: StableId,
    caller_artifact_id: StableId,
    callee_artifact_id: StableId,
    accepted_file_bound: usize,
) -> Result<ContextSubjectWindowsSessionV3, ContextError>
```

`accepted_file_bound` is the already validated
`reviewgraphen.generic_review_request.v3.ingest.max_files`; it is not a caller
policy override. C7 must dispatch request-v2 exclusively to the v2 entry point
and request-v3 exclusively to this v3 entry point. Core performs no automatic
conversion or cross-decode.

V3 commits the complete accepted-file, reached-file, materialized-source, and
support-anchor denominators by known count plus sorted-ID-set SHA-256. Only the
union of valid subject files and reached files becomes source candidates.
Support rejection detail is partitioned into one exact count/digest summary per
reason, while the callee and caller remain two ordered admitted-or-lost outcome
records. Its fixed policy hash is
`sha256:932bfa18c5d286c63196366d6d2dc1aaf402f50baa1f1ab5f075b1007be55dd8`.
