# Open questions requiring user scope decisions

These are the only decisions large enough to change the architecture, schedule,
or interpretation. Smaller choices have recommendations in the other artifacts
and should be resolved in the ADR by the implementation team.

## Q1. Is the first useful release allowed to remain non-authority?

**Recommended answer: yes.** Candidate A should generate proposed/unreviewed
claims, typed evidence/verification outcomes (often unsupported/inconclusive), a
short Markdown projection, and canonical audit JSON while retaining
`trusted_pass=false`. Human acceptance should remain a later explicit
Core/Store operation that binds claim, evidence, verification, snapshot, and
human authority.

Choosing “the same command must also record human acceptance” materially expands
the slice into durable Store admission, current-snapshot freshness, conflict,
recovery, and possibly gluing/gate work. It adds authority/security risk and
should be a separate ADR and vertical slice, not a CLI flag on report rendering.

**Decision needed before U0:** non-authority first release, or authority-bearing
human-decision release.

## Q2. Which semantic wedge defines the first product market?

**Recommended answer: changed explicit `unsafe` boundaries.** It is the smallest
general Relation fact `syn` can extract exactly without pretending to resolve a
method receiver, trait alias, lock type, error conversion, or control-flow
property. It tests the missing semantic, projection, verifier, report, and
quickstart seams together.

Choose public-API compatibility instead if the intended first users are library
maintainers and pinning rustdoc/toolchain/comparator configuration is acceptable.
That alternative has stronger mechanical anchors and likely greater prevalence,
but is estimated at 11–14 rather than 8–10 terra/high units and introduces cfg,
feature, re-export, macro, and toolchain identity into the denominator.

**Decision needed before U0:** unsafe-boundary Relation, or public-API
compatibility Invariant. Do not implement both in the first slice.

## Q3. What evaluation cost/claim level is authorized?

**Recommended answer: authorize a two-stage design with a hard census stop.**
Freeze the evaluator-only histories and eligibility contract before
implementation. Enumerate and mechanically establish presence first. Run the
confirmatory model study only if at least 83 positive units and 20 controls
exist; otherwise publish feasibility results without population precision/recall
claims.

The confirmatory design uses three replicates per arm per unit and blind root
adjudication, so it is materially more expensive than a smoke evaluation. If
that cost is not authorized, the correct claim ceiling is “clone-reproducible
mechanism and bounded case evidence,” not “reduces missed Rust defects.”

**Decision needed before holdout enumeration/model procurement, not before U0:**
confirmatory population study if the census passes, or feasibility-only evidence.

## No decision requested

- Do not make `direct_calls` complete by assertion; retain partial/unknown facts.
- Do not use LLM prose as parser/resolver/evidence or run reviewer-supplied shell.
- Do not revive a named product fixture; deterministic mode should always
  abstain generically and exercise the ordinary Git path.
- Do not treat Cargo-test success, judge approval, or confidence as human
  acceptance.
- Do not rewrite historical M7–M19 results after the new slice; any successor
  result is a new frozen experiment.
