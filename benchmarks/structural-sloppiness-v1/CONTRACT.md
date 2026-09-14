# Structural-sloppiness analyzer contract

Status: experimental benchmark API v1; non-authoritative.

## Callable API

`reviewgraphen_benchmark::structural_sloppiness` exposes:

```rust
analyze(&ProgramSpace) -> Result<AnalysisReport>
validate_report(&ProgramSpace, &AnalysisReport) -> Result<()>
flat_projection_canonical_bytes(&ProgramSpace) -> Result<Vec<u8>>
```

`analyze` accepts only an already validated `ProgramSpace`. `validate_report`
recomputes the complete analysis and rejects any stale input binding or changed
report field. The input hash is SHA-256 of the full canonical `ProgramSpace`,
not a selected-source projection.

## Fixed predicate and eligibility

The contract ID is `changed_input_consumer_bridge_mismatch@1`. An eligible gate
is an accepted `changed_by(S,C)` relation whose extraction method is
`reviewgraphen.ingest.git.changed_structure.v1`, where:

- `S` is an accepted artifact with `kind == "function"` and `public == true`;
- `C` is an accepted change artifact with the same extraction method and
  the producer's per-kind path shape: `added` has an empty base and non-empty
  target; `deleted` has a non-empty base and empty target; `modified` and
  `type_changed` have the same non-empty path at both endpoints; `renamed` and
  `copied` have two non-empty endpoints.

The `changed` marker is deliberately not an eligibility condition: its absence
is one of the wiring failures under test. Each eligible gate evaluates exactly:

```text
S.changed == true
OR exists accepted contains(X,S) where X.changed == true
```

`X` may be any marked artifact and need not equal `C`. Only one incoming hop is
examined; reversed, wrong-target, or transitive paths do not satisfy the gate.
False yields a `consumer_bridge_mismatch` candidate limited to this predicate
on this supplied graph. It does not assert a source defect or obligation
applicability.

## Report boundaries

`AnalysisReport` keeps `observed_facts`, `candidate_claims`, and `obstructions`
as separate arrays. Eligible/excluded counts and stable exclusion reasons are
reported. Zero eligible gates is `not_exercised`. Partial/missing/unknown
capabilities and input limitation IDs remain visible and create an extraction
obstruction without suppressing observations of accepted facts.

Every report binds analyzer/contract versions and contract hash, snapshot/base/
target, profile, rule-set and extractor-set hashes, source IDs and locations,
and the full input hash. Stable output arrays are ordered by their stable IDs.
`scope.information_loss` includes
`report_projection_omits_unrelated_program_facts`: it describes report
projection loss, while the full input hash still binds omitted facts.

Authority is fixed to `classification == "non_authority"` with `accepted`,
`verified`, `human_accepted`, and `sign_off` all false.

## Flat ablation

The flat projection retains complete artifact records, capability declarations,
limitations, and relation kind/provenance counts, but erases relation endpoints
and IDs from the relation records. Capability/limitation source sets remain
unchanged and may retain opaque relation-ID evidence references, but expose no
erased endpoints. Its bytes must therefore remain identical under endpoint-only
rewiring that preserves that inventory. This narrow claim is not anonymization.
The declared-loss projection is an ablation of relational information, not a
static-analysis baseline.

The closed JSON contract is
`schema/structural-sloppiness-report-v1.schema.json`. Any semantic change to
this API, predicate, or report requires a new major version; v1 has no migration
or authority inheritance.
