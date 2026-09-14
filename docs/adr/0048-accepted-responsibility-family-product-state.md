# ADR 0048: Persist accepted responsibility-family product state

- Status: Accepted
- Date: 2026-09-14

## Context

The benchmark can enumerate candidates and produce a non-authoritative
maintenance proposal, but accepted family membership previously lived outside
ReviewGraphen. Reinspection cannot be governed across snapshots unless the
accepted contract, intentional differences, denominator, evidence, and
verification basis are retained together without promoting similarity into a
Program fact.

## Decision

Add the new, closed `reviewgraphen.responsibility_family_state.v1` product
schema and the matching Core type. It is separate from ProgramSpace and can be
created only with:

- a human decision ID and a proposal hash;
- nonempty Evidence and Verification ID sets;
- a common-contract ID/hash and extractor ID/version;
- sorted members with anchors, source IDs, and purpose constraints;
- distinct implementation and endpoint denominators;
- explicit unknowns and the fixed non-sign-off authority ceiling.

Core performs semantic validation and canonical encoding. Store persists those
canonical bytes through the existing verified CAS and revalidates them on read.
The production CLI exposes the schema through `schema list`, `schema print`,
and `schema validate`; it does not gain a family-acceptance command. External
human authority remains responsible for creating the accepted record.

This is a new v1 type, so no migration exists. Benchmark candidate and proposal
schemas remain non-authoritative and cannot be decoded as this state.

## Consequences

ReviewGraphen can retain accepted responsibility families and the information
needed for snapshot-bound reinspection without treating clone detection as
acceptance or Verification. CAS storage alone does not create an index,
workflow event, human sign-off, or proof that the common contract is correct.
