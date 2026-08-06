# ADR 0001: ReviewGraphen is an Intermediate Tool

- Status: Accepted for v0.1 design
- Date: 2026-08-07
- Decision owners: CAPH TECH / ReviewGraphen maintainers

## Context

HigherGraphen distinguishes shared core packages, reusable `*graphen` intermediate tools, and concrete domain products. The name `ReviewGraphen` follows the intermediate-tool naming family.

The initial problem arose from code review, but the core objects—review obligation, execution, claim, evidence, verification, decision, coverage, staleness—also apply to architecture, specification, test, policy, and AI-generated artifact review.

Treating ReviewGraphen as a Code Review-only domain product would bind the central model to AST, call graph, PR and programming-language vocabulary. Treating it as a generic HigherGraphen core module would leak review workflow and provider concerns into lower-level packages.

## Decision

ReviewGraphen is a reusable **Intermediate Tool** over HigherGraphen.

```text
ReviewGraphen
  central object: ReviewObligation and Review Graph
  initial profile: Code Review
  future profiles: Architecture, Specification, Test, AI Artifact Review
```

Code Review-specific vocabulary and extractors live in a versioned profile and adapters. ReviewGraphen core remains artifact- and language-neutral.

## Consequences

### Positive

- The product name matches HigherGraphen naming rules.
- Review process semantics can be reused across domains.
- AST/CFG/DFG dependencies remain outside the core model.
- Research can separate general workflow effects from code-specific extraction quality.
- A domain profile can define its own target kinds, properties, evidence policy and gate.

### Negative

- The abstraction boundary is more demanding than a single-purpose code reviewer.
- Initial documentation must distinguish methodology, intermediate tool, profile and report artifact.
- Over-generalization may delay a useful Code Review vertical slice.
- Profile APIs require versioning before multiple profiles exist.

## Alternatives considered

### A. Code Review Domain Product

Rejected as the primary identity. It is simpler initially, but the name `ReviewGraphen` and central model imply a reusable abstraction. Code Review remains the first profile and reference product surface.

### B. Add review concepts directly to HigherGraphen core

Rejected. HigherGraphen should provide generic Space, Context, Evidence, Invariant, Obstruction, Projection and Morphism primitives. Reviewer adapters, obligation lifecycle and code-review policies are not universal core concepts.

### C. A thin CLI over existing `pr-review`

Rejected as insufficient. It would preserve target recommendation but not add a versioned obligation universe, evidence-bound verification, multi-stage coverage, gluing or staleness.

## Invariants

- Every profile declares its artifact vocabulary and required extractor capabilities.
- No Code Review-only type is required by ReviewGraphen core.
- Domain products may use ReviewGraphen without adopting the Code Review profile.
- HigherGraphen core never depends on ReviewGraphen.

## Revisit triggers

- Two non-code profiles cannot be expressed without changing core semantics.
- More than half of core objects remain code-specific after the MVP.
- ReviewGraphen provides no reusable capability beyond a Code Review CLI.
