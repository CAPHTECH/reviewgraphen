# ReviewGraphen Documentation Bundle Manifest

> Bundle version: Draft v0.1  
> Created: 2026-08-07  
> Baseline: HigherGraphen 0.7.1 / `CAPHTECH/higher-graphen@0f1e1cfe`

## Entry points

- [`README.md`](README.md) — product overview and central thesis。
- [`docs/index.md`](docs/index.md) — reading routes and normative order。
- [`AGENTS.md`](AGENTS.md) — implementation rules for humans and coding agents。
- [`VALIDATION.md`](VALIDATION.md) — offline validation result and known limitation。

## Design documents

The numbered series under `docs/` covers:

```text
00–02  vision, positioning, research foundation
03–04  conceptual model and HigherGraphen mapping
05–06  architecture and ProgramSpace ingestion
07–12  obligations, context/gluing, execution, evidence, coverage, staleness
13–16  CLI, schema/report, storage, security
17–21  evaluation, MVP, backlog, commercial boundary, migration
```

Additional references:

- [`docs/glossary.md`](docs/glossary.md)
- [`docs/source_trace.md`](docs/source_trace.md)

## Architecture decisions

Eight accepted-for-v0.1 ADRs live under [`docs/adr/`](docs/adr/):

1. ReviewGraphen is an Intermediate Tool。
2. Artifact / Review / Evidence Space separation。
3. ReviewObligations define the coverage universe。
4. Minimal context projection with declared loss。
5. LLM output is a reviewable claim。
6. Standalone repository over HigherGraphen。
7. Local-first event log and derived index。
8. Language-neutral core with profile-specific extractors。

## Contracts

[`schemas/`](schemas/) contains:

- ProgramSpace input schema and example。
- ReviewObligation universe schema and example。
- Review report schema and example。
- Local configuration example。
- Schema validation notes。

## Reference scenario

[`examples/double-submit-payment/`](examples/double-submit-payment/) contains:

- accepted ProgramSpace fixture;
- risk-first review plan;
- full review report;
- a small intentionally vulnerable Rust fixture;
- explanation of Node / Relation / Path / Invariant / Gluing behavior。

## Agent integration

[`skills/reviewgraphen/SKILL.md`](skills/reviewgraphen/SKILL.md) defines when and how an agent should use ReviewGraphen, including safety and epistemic boundaries. It explicitly states that this bundle is a design contract and does not claim that the CLI already exists.

## Validation

[`scripts/validate_bundle.py`](scripts/validate_bundle.py) checks:

- JSON and TOML parsing;
- Draft 2020-12 schema validity and examples;
- Markdown relative links and code fences;
- fixture cross-references and state invariants;
- ProgramSpace / obligation / report consistency;
- optional Rust fixture execution when `cargo` is available。

Run:

```bash
python scripts/validate_bundle.py
```

## Artifact status

This bundle is an implementation-ready design baseline, not an implemented ReviewGraphen release. Commands, package names and schemas are proposed contracts until code and compatibility tests establish them.
