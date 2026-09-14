# ADR 0042: Experimental responsibility-family reinspection planning

- Status: Experimental, implementation authorized for a local trial
- Date: 2026-09-14

## Context

The structural-sloppiness research identified five independently implemented
path validators with a shared base contract and purpose-specific constraints.
Two cross-member mutations survived six of ten endpoint/test cells before a
shared conformance fixture and zero afterward. A scratch state table then
selected one changed member for focused reinspection, while function anchors
avoided one unrelated same-file comment edit.

The current ReviewGraphen CLI and the experimental structural-sloppiness v1
analyzer cannot represent a responsibility family or emit change-bound
reinspection obligations. Merely treating similar code as one accepted family
would violate the fact/inference and human-acceptance boundaries.

## Decision

Add an experimental, benchmark-only v1 library and narrow CLI that compare two
externally supplied responsibility-family states. Inputs bind a declared
decision basis, common contract, versioned extractor, members, structural
anchors, purpose-specific constraints, source IDs, snapshots, and unknowns.

The planner emits a deterministic non-authoritative reinspection plan. It
distinguishes common-contract, decision-basis, extractor, member-anchor,
purpose-constraint, member-addition, and member-removal changes. Snapshot change
alone does not reopen an unchanged member. Obligation IDs bind the before/after
states and the exact changed member. Output order is stable and validation
recomputes the complete plan.

Neither input nor output becomes an accepted Program fact, verified evidence,
human decision, or sign-off. The tool does not discover duplicate code, infer
that members share a responsibility, execute tests, or claim semantic
equivalence. Candidate discovery and family acceptance remain external.

Keep this independent of the main `reviewgraphen` CLI and all persisted product
schemas. A semantic change requires a new major version.

## Verification and limits

Test unchanged states, common and member-specific changes, add/remove, extractor
and decision changes, stable ordering and IDs, malformed/duplicate input,
authority boundaries, and stale/tampered plan rejection. Calibrate validation
with one deliberate plan mutation.

The initial real-data check is the five-member path-policy family only. It does
not establish utility for other families, moves/splits/merges, macros,
cross-language anchors, or overlapping-family gluing.
