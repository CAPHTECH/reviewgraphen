# m12 convergent ReviewGraphen-first review

This follow-up tests whether an agentic source review that previously failed to
terminate can produce a valid artifact when the review target and convergence
policy are explicit, without asking the model to enumerate obligations first.

The model must invoke the frozen `lower_compose` ReviewGraphen projection as its
first tool call.  The review is limited to `lower_compose` and its directly used
helpers in `rust/fsl-core/src/compose.rs`.  At most three defect hypotheses and
eight recorded review tool calls are permitted.  A provisional `review.json`
must be written by the fifth tool call; after that, the model may only validate
or remove existing findings and finalize the same artifact.

The repository's existing `reviewgraphen-methodology` skill is intentionally
not installed in the isolated checkout: that skill says not to call a
ReviewGraphen tool and requires obligation enumeration before defect review,
which conflicts with this experiment's intervention.

The source snapshot, projection, validators, and profiler are content-addressed
dependencies of `../m11-review-agentic-local-v1/`.  Their hashes are frozen in
`preregistration.json` before the first generation request.

