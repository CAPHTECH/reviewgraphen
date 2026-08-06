# ADR 0008: Language-Neutral Core and Profile-Specific Extractors

- Status: Accepted for v0.1 design
- Date: 2026-08-07

## Context

The initial Code Review profile requires AST, symbol, call, test and dependency facts. These facts differ substantially across Rust, Dart, TypeScript, Swift and other languages. Building a universal internal CPG before validating ReviewGraphen would be excessive. Encoding Rust-specific syntax into core records would make future profiles and languages difficult.

## Decision

ReviewGraphen core defines a language-neutral ProgramSpace contract and extractor capability model. Language/tool adapters produce typed accepted facts plus completeness and limitations.

```text
Language adapter
  -> facts + provenance + capability report + unresolved regions
  -> ProgramSpace lift

Review profile
  -> required capabilities and rules
```

The MVP implements manual JSON ingestion first and a bounded Rust adapter second. It does not claim a complete language-independent CPG.

## Consequences

### Positive

- Core obligations and review states are reusable.
- Multiple analyzers can contribute facts without one parser monopoly.
- Capability gaps are explicit and gateable.
- A profile can require only the relations needed for a property.
- Research can measure extraction quality separately from review workflow.

### Negative

- Normalizing different language semantics is difficult.
- Generic relation names may become too weak.
- Adapter inconsistency can produce incomparable coverage.
- Cross-language repositories require composite capability reports.

## Alternatives considered

### A. Build a full CPG implementation in ReviewGraphen

Rejected for MVP. Mature external analyzers should be adapted where possible.

### B. Use files and text only

Rejected. Relation/path obligations require semantic structure.

### C. Let the LLM infer all relations

Rejected as accepted fact extraction. LLM-derived relations may be candidate facts with provenance, not the deterministic baseline.

### D. Rust-specific core

Rejected. Rust is an initial adapter, not the identity of ReviewGraphen.

## Invariants

- Every fact records extractor/tool version and source scope.
- Unresolved relation is not omitted without a completeness record.
- Profile rules declare required/optional capabilities.
- Capability absence cannot be converted to `not_applicable` unless the rule says so.
- Language-specific payloads live behind extension fields or adapter schemas.

## Revisit triggers

- Generic contract cannot represent a second language without semantic loss.
- A standard external interchange format provides sufficient code graph semantics.
- Performance requires a shared normalized graph engine.
