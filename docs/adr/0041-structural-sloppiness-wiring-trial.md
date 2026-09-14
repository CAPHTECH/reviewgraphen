# ADR 0041: Experimental producer/consumer wiring analysis

- Status: Experimental, implementation authorized for a local trial
- Date: 2026-09-13

## Context

ReviewGraphen's historical ingestion defect before `7b40d48` supplied accepted
`changed_by` facts but not the marker/containment form consumed by obligation
synthesis. Hand-built fixtures could satisfy the consumer while real inputs did
not. This is a concrete calibration case for structural-sloppiness analysis.

## Decision

Add a non-authoritative benchmark-only analyzer of the exact named contract
`changed_input_consumer_bridge_mismatch@1`. For an accepted changed-structure
producer edge from a public free function to its change artifact, evaluate the
actual consumer's predicate: own `changed:true`, or a one-hop incoming `contains`
edge from an artifact with `changed:true`. A false result is a candidate about
the supplied ProgramSpace, not proof that source code is defective or that a
review obligation should be generated.

Keep observations, candidate claims and extraction limitations separate. Bind
the report to the full canonical input hash, snapshot, versions, rule/contract
hash and source IDs. Report eligible and excluded counts, including a distinct
not-exercised result when the eligible set is empty. Partial extraction remains
visible without hiding an actual predicate result on accepted input facts.
No result grants accepted/verified/sign-off authority.

The new report/schema/API/CLI are experimental v1, additive and independent of
all product schemas and persisted state. Semantic changes require a new major;
there is no automatic migration, inheritance of authority, or mutation of input.
The analyzer runs no model or target code. A separate local harness runs pinned
historical/current ingestion against an immutable real target diff.

A flat inventory that erases relation endpoints is a declared information-loss
ablation. It tests dependence on relational information, not superiority over
other static analyzers or the necessity of HigherGraphen's implementation.

## Verification and limits

Test the exact consumer branches, misleading unrelated markers, wrong endpoint
and reversed edges, provenance selection, partial and absent input, deterministic
reporting, input binding and schema. Calibrate tests with deliberate failures.
Run actual old/fixed/current ingestion on the same real source changes, retaining
nonexercising cases. The historical case is calibration, not held-out efficacy.
This trial does not cover semantic contract drift, arbitrary dead code,
test effectiveness, all code quality, or attribution of defects to AI authors.
