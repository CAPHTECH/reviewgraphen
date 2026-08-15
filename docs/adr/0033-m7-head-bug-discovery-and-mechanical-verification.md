# ADR 0033: M7 HEAD Bug Discovery and Mechanical Verification

- Status: Accepted
- Date: 2026-08-15

## Context

ADR 0031 established that the known-fix recall design is infeasible on the
available FSL population: 28 strict-presence units are fewer than the 30-unit
calibration sample and far fewer than the conservative powered final `n=83`.
Incremental candidate expansion cannot repair that design.

Reviewing the current FSL HEAD removes the known-fix denominator and its
selection problem, but also removes recall ground truth. Model dispositions are
not evidence that a reported bug is real. In particular, a generated failing
test proves only that code disagrees with the generated expectation; it does not
prove that the expectation matches project intent.

## Decision

`m7-head-v1` is a new additive experiment. It never rewrites a pilot, real-v1,
real-v2, or full-reviewgraphen result.

### Calibration before production

One fixed Codex CLI test/fix generator is calibrated on the twenty positive
parents in `m7-real-v1`. Each generator packet contains only:

- the complete selected production files from the defective parent;
- Cargo manifests and bounded, target-independent test-style examples;
- an opaque target hint containing the oracle path, symbol, line range, and
  mechanism tag; and
- a closed output schema and fixed instruction.

It contains no fix OID or tree, diff, commit/issue/branch material, regression
test, test selector, presence output, prior candidate output, or control
snapshot. Source bodies are serialized into the prompt by the no-tools process
adapter; the provider cannot invoke a shell or read the repository.

The generator proposes exactly one new integration-test file and a production
Rust fix patch. The harness, not the model, supplies the allowed test targets
and exact command. A calibration unit succeeds only when:

1. the generated test patch changes exactly its selected new test path;
2. the production patch changes only the unit's selected production paths and
   leaves the generated test byte-identical;
3. the exact generated test exists after patching and both parent and fix test
   targets build;
4. parent plus generated test fails at runtime, not at build or timeout;
5. canonical fix plus the same generated test passes; and
6. parent plus generated test plus the proposed production fix passes.

Raw provider response, parsed proposal, patches, commands, output bytes,
statuses, and hashes are retained. Infrastructure failure may be retried with
the identical packet; semantic or protocol failure receives no repair attempt.

The preregistered production threshold is at least 9 successes out of 20. At
the boundary of an unusable generator success probability 0.25,
`P[Binomial(20, 0.25) >= 9] = 0.0409252`; at success probability 0.60 the pass
probability is 0.9434736. Fewer than nine mechanically successful units stops
the experiment before HEAD review.

### HEAD review

If calibration passes, the immutable FSL HEAD tree and a complete inventory of
production Rust files are frozen before model execution. Every production file
is primary-owned by exactly one deterministic size-bounded packet. Cross-file
context may be duplicated, but ownership and bytes are recorded so no file is
silently sampled away. B1, G3-proxy, and full ReviewGraphen receive identical
source/spec bytes and differ only in their preregistered review scaffold.

Each arm emits non-authority proposed findings. Findings are normalized and
linked without arm labels before verification. The common calibrated generator
receives one opaque finding at a time with bounded source context and the same
test/fix proposal contract. Arm identity and other-arm output are prohibited.

### Mechanical verification and specification support

Every proposed finding is classified into exactly one of:

- `verified`: generated test builds, fails on frozen HEAD at runtime, and passes
  after the test-immutable proposed production fix;
- `not_verified`: a well-formed attempt contradicts one of those observations;
  or
- `unverifiable`: abstention, unsupported test boundary, invalid proposal,
  infrastructure exhaustion, or another typed inability to execute the test.

Mechanical verification establishes a reproducible code/test disagreement; it
does not establish intended behavior. Verified findings therefore receive a
separate specification classification. `spec_mechanically_supported` requires
a content-hashed `.fsl` source and an executable existing FSL claim/scenario
whose checked result entails the generated expectation. Exact text citations
without that executable link are `spec_text_cited_only`; no matching normative
source is `spec_not_found`. A model assertion alone cannot select the first
class.

### Reported estimands

Recall is not estimated. The report publishes per arm:

- reported, verified, not-verified, and unverifiable counts;
- verified/report precision with an exact interval;
- linked verified-bug overlap and arm-unique discoveries;
- spec-support classes; and
- disagreement between original model disposition and mechanical outcome.

Zero verified bugs is reported as zero, with the calibrated verifier sensitivity
and reviewed source inventory needed to interpret that observation. It is not
reported as absence of bugs.

## Consequences

- The experiment measures verified yield and precision, not recall.
- Test generation is itself measured before it is used as a verifier.
- A passing generated test after a patch cannot silently become accepted project
  intent; specification support remains an explicit, stronger classification.
- No upstream issue, branch, commit, PR, or message is created.
