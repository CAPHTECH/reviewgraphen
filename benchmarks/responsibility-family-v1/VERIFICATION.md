# Responsibility-family verification record

Date: 2026-09-14. Working tree based on Git snapshot
`c0747614c2c20a51771f6f6495f196682e3912fd`, with the experimental decision
support, shared path fixture and severity-wire follow-up applied but not
committed.

## Decision support

The checked five-member candidate produced 33 deterministic obligations. The
assessment expanded to an exact obligation closure and proposed
`shared_conformance_test`: 12 supports, 6 opposes, and 15 unresolved. Two
proposal runs were byte-identical with SHA-256
`9801c9a7bf9bfa917fda6c56c47509463e09840ba2fee2bb518bb0498f5d4821`.
`decision-validate` exited 0. Changing only the M6 candidate anchor caused the
old assessment to be refused with exit 4.

This is a deterministic proposal, not family acceptance. The assessment's
Evidence and Verification IDs remain external references rather than admitted
product-store objects.

## Shared-conformance implementation

The selected change adds one eight-case fixture and one crate-local test at
each of the five endpoints. It changes no production predicate, signature,
typed error, dependency or runtime path.

Focused tests passed at all five endpoints. Complete owning-package results:

- core: 463 library tests plus integration groups 24, 2, 11, 3, 40 and 65;
- ingest: 71 library tests plus integration groups 19, 8 and 41;
- benchmark: 54 library tests plus three one-test integration targets.

One fresh mutation-sensitive check was executed in an isolated clone. Replacing
only M6's `.` comparison with `./` made
`mapping_path_obeys_shared_snapshot_relative_contract` fail on path `.` with
exit 101. The source working tree was not mutated for that check.

The earlier isolated A25/A27/A28 experiment reported 4/10 caught and 6/10
surviving before the fixture, then 10/10 caught and 0/10 surviving afterward.
Those raw logs remain in scratch storage and were not promoted here, so this
file records received historical evidence plus one newly reproduced mutation;
it does not claim a new ten-cell rerun.

## Candidate-discovery boundary

The earlier direct-Jaccard screen reported 1,466 production free functions and
416 candidates; its known path-policy pair ranked 269th and all frozen top-five
natural candidates were rejected after source reading. Exact-shape pairs were
excluded, including potentially useful exact responsibility duplicates. This
screen was inspected but not rerun after the current changes.

[R] ReviewGraphen is presently useful after candidate enumeration: it makes the
decision denominator, evidence gaps, selected maintenance response and future
reinspection state explicit. It has not shown useful unknown-family discovery.
The next discovery experiment should use an external Type-1/2/3 clone detector
without dropping exact clones, then feed held-out candidates into this decision
path. This inference fails if a policy-aware extractor demonstrates materially
better ranking on an untouched corpus.

## Exact-clone follow-up: Severity wire text

An external exact-match scan found two exhaustive mappings over the same core
`Severity` type: core planning canonical JSON and store index-v5 projection
JSON. This candidate was not part of the earlier screen because that screen
discarded exact-shape pairs. Direct source reading established the same five
wire strings and distinct surrounding writers but found no separate reason for
the mapping itself to change.

ReviewGraphen expanded the two-member candidate into 15 obligations and
proposed `shared_validator`: 14 supports, one opposed separation rationale and
no unresolved obligations. `decision-validate` exited 0. The proposal SHA-256
was `97e6e25643312a153bf643bb2c29e084bd624b9def8f4cab39ce72d08e68734a`.
This is still a non-authoritative proposal; ADR 0044 records the implementation
decision separately.

The implementation added allocation-free `Severity::as_str`, removed both
duplicate matches and retained the core planning and store projection writers.
The five-variant contract test passed. A deliberate `critical` to
`critical-mutant` change made that test fail with exit 101, and the mutation was
then reverted. Core's complete 464-test library target and its integration
targets passed. Store's 202-test library target had two simulated-crash tests
interfere in the parallel full run; both passed when rerun individually, and a
subsequent single-threaded run passed all 202 library tests plus four integration
tests.

The reinspection plan bound the pre-change two implementations to the
post-change one shared implementation. Its denominator changed from 2 to 1 and
it emitted exactly three obligations: two `member_removed` and one
`member_added`. The plan validated and had SHA-256
`f040fb421965e3789ef9e1c2758c8380767826027f28c3d7bd1313dd5a923e3c`.

This follow-up demonstrates the useful composition currently supported:
external candidate enumeration, ReviewGraphen decision closure, an explicit
maintenance decision, mutation-sensitive verification, and snapshot-bound
reinspection. It does not demonstrate that ReviewGraphen itself discovers
unknown duplicates.

## Accepted-anchor exact-body discovery

The benchmark-only `discover-exact` command was then run against the preserved
c074761 ProgramSpace (input SHA-256
`02fa8e49b5b3be356e5f8de5272a1133159183c906155bd3502f218a782f83ba`).
Two runs were byte-identical with report SHA-256
`99a48b1290b8e7db9a474d6d4e5d6f6bb15cd2c1d1ef275e2f5e0e0cb8d14659`.
The report contained:

- 23,369 accepted Rust symbol anchors;
- 1,861 eligible functions under the declared profile;
- 51 exact-body groups containing 123 members;
- 3,080 non-function symbols, 12,658 out-of-profile paths, 1,072 accepted
  test functions and 4,698 symbols with unknown test scope excluded.

The command independently rediscovered the two severity mappings as one exact
body group, ranked 26 of 51 by minimum member span. This establishes candidate
discovery by ReviewGraphen for this one known item; it does not establish useful
ranking or general recall.

Direct reading of the first five ranked groups exposed an upstream fact loss.
Ranks 1 and 4 are standalone `#[cfg(test)]` helpers even though their accepted
`test_function` fact is false. Ranks 2 and 5 are exact v4/v5 projection copies
where compatibility-specific separation remains a plausible reason to keep
them distinct. Rank 3 is a same-file duplication of exclusion-ID derivation and
is a plausible decision-stage candidate. These are source readings, not five
completed responsibility-family assessments.

[R] Exact-body enumeration is useful as a cheap candidate source, but not yet
as an unattended prioritizer. The inference fails if held-out assessment shows
that the remaining ranked groups are predominantly unrelated, or if fixing the
test-scope fact materially changes the ranking and candidate yield.

The rank-3 exclusion-ID group was then registered without changing its source
membership. ReviewGraphen expanded it into 15 obligations and proposed
`shared_conformance_test`: eight supports, one opposed separation rationale and
six unresolved obligations covering compatibility, performance and shared
implementation safety. The proposal validated and had SHA-256
`3f365890224053c9f72b486660f1ba3b6f3d7e281eaec4b83babb271fe972192`.

ADR 0046 keeps the legacy-D and rule-neutral implementations separate and adds
one cross-path conformance test. The test passed on the current implementation.
Changing only the rule-neutral preimage key `candidate_key` to
`candidate_key_mutant` made it fail with exit 101 and distinct StableIds; the
mutation was reverted. This is the first candidate that the new ReviewGraphen
enumerator surfaced before a responsibility-family assessment and that led to
a verified maintenance response.
