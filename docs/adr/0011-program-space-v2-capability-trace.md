# ADR 0011: ProgramSpace v2 Capability Trace Contract

- Status: Accepted for v0.1 design
- Date: 2026-08-08
- Scope: `reviewgraphen.program_space.input` schema, the `Extraction`/
  `Limitation`/`CapabilityState` capability-trace contract, and the
  capability-gap synthesis semantics that consume that trace
  (`capability_gap_reason` and the capability-gap obligation it feeds),
  including the one obligation-schema change that contract requires:
  `schemas/reviewgraphen.obligation.schema.json` gains a `riskImpact` enum
  (the existing `severity` values plus `unknown`) so a capability-gap
  obligation's `risk.impact: "unknown"` is schema-valid, and `risk.impact`
  now references it instead of `severity`; `severity` itself is unchanged
  everywhere else it is used. No other property of that schema, no change
  to `schemas/reviewgraphen.report.schema.json` (its `obstruction.kind` was
  already a free string), no change to any other obligation's synthesis,
  and no Adapter count change.

## Context

The current `reviewgraphen.program_space.input.v1` schema and the core
`Extraction` type (`crates/reviewgraphen-core/src/program.rs`) record
`capabilities` as a bare map from capability name to `CapabilityState`
(`complete` / `partial` / `missing` / `unknown`). The state has no source
trace: nothing in the input says which accepted facts, adapters, or
limitations a declared state is grounded in, so a `partial` or `unknown`
declaration cannot be independently checked against the ProgramSpace it
travels with.

Two related gaps compound this:

1. `Limitation.source_ids` is not required to be non-empty today:
   `Limitation::new` validates `description` and each `related_capabilities`
   entry for non-emptiness, but leaves `source_ids` optionally empty,
   unresolved against the ProgramSpace, and unchecked for circularity.
   Nothing prevents a limitation chain whose only sources are other
   limitations, i.e. a trace that never grounds in an accepted fact.
2. Obligation synthesis's capability-gap reason (`capability_gap_reason` in
   `crates/reviewgraphen-core/src/synthesize.rs`) already distinguishes
   `partial` from everything else, but folds `missing`,
   `unknown`, and an **undeclared** capability (absent from the map entirely,
   the `_` arm) into the identical `capability_missing:{capability}` reason
   string. A reviewer reading the coverage denominator cannot tell "the
   adapter tried and failed" from "no adapter ever declared this capability".

Duplicate JSON object keys are also a live risk: `serde_json`'s default map
deserialization silently keeps the last occurrence of a repeated
`capabilities` key, so a producer bug that writes the same capability twice
with different states is accepted without a trace, contradicting the
`additionalProperties: false` typo-detection posture already documented in
`schemas/README.md`.

Finally, there is no schema-version boundary today: any JSON that happens to
parse against the current shape is accepted, so a future field/semantics
change has no way to fail closed on old input, and no migration path exists
between input schema versions (the obligation schema already has this
precedent for `.v1`/`.v2` — see `schemas/README.md`).

## Decision

### 1. Preserve v1, advance the generic path to v2

`reviewgraphen.input.schema.json` and `reviewgraphen.input.example.json`
(currently v1 content) are copied verbatim to
`reviewgraphen.input.v1.schema.json` and `reviewgraphen.input.v1.example.json`
before any v2 change lands, mirroring the existing
`reviewgraphen.obligation.schema.json` / `reviewgraphen.obligation.v1.schema.json`
split. The generic `reviewgraphen.input.schema.json` /
`reviewgraphen.input.example.json` paths then advance to v2 content, so the
stable path used by `scripts/validate_bundle.py` and CI always validates the
current schema version without a script change.

### 2. v2 schema ID

The new schema's `$id`/`schema` discriminator is `reviewgraphen.program_space.input.v2`.
It is a self-contained strict schema, not a `$ref`-based delta over v1, so a
v1 record with a bare `capabilities` map is a structural rejection under v2,
not a silently-accepted subset.

### 3. `CapabilityDeclaration` replaces the bare state

`capabilities` becomes a map from capability name to:

```json
{
  "state": "partial",
  "source_ids": ["snapshot:double-submit-v1", "artifact:function:submit_payment"]
}
```

`source_ids` must be non-empty and every entry must resolve to an ID that
exists in the same ProgramSpace: a repository, snapshot, artifact, relation,
context, invariant, evidence, or limitation ID — exactly the union of
`ProgramSpace::known_ids` and `known_evidence_ids`. An empty or unresolved
`source_ids` is a validation error at the same boundary
`Extraction::new`/`revalidated` already occupies, not a silently accepted
declaration.

`AdapterDescriptor.id` and `ProfileDescriptor.id` are plain `String`, not
`StableId`, and neither is a member of that known-ID set, so an adapter or
profile identifier is never a resolvable `source_ids` target under this
ADR. Making adapter provenance an explicit capability source requires
widening `AdapterDescriptor.id` to `StableId` and registering adapters in
`ProgramSpace`'s known-ID set — out of scope here and left to a future ADR.

### 4. Limitation source trace and non-circularity

v2 is the first version to require `Limitation.source_ids` to be non-empty —
`Limitation::new` does not enforce that today. Every entry must also resolve
within the ProgramSpace, exactly like a capability's `source_ids`. A
limitation's source set must not be self-referential (containing its own ID)
and must not form a cycle consisting only of other limitations — every trace
must ground out in at least one non-limitation fact: a repository, snapshot,
artifact, relation, context, invariant, or evidence ID. `related_capabilities`
may only name capabilities present as keys in the same
`Extraction.capabilities` map; a limitation cannot claim relevance to an
undeclared capability.

### 5. Cross-field completeness contract

`Extraction::new`'s existing per-capability check is extended from a
one-directional contradiction check to a required-correspondence contract:

| State | Related limitations |
| --- | --- |
| `complete` | Forbidden (unchanged). |
| `partial` | Required: at least one source-backed limitation, none of kind `capability_missing`. |
| `missing` | Required: at least one source-backed limitation of kind `capability_missing`. |
| `unknown` | Required: at least one source-backed limitation (kind unconstrained). |

Today `partial`/`missing`/`unknown` only forbid a specific contradiction;
none require a related limitation to exist. v2 requires every non-`complete`
state to be justified by at least one limitation whose `source_ids` resolve,
closing the gap where a producer declares `partial`/`missing`/`unknown`
without recording why.

### 6. Parse boundary: `MigrationRequired` / `UnsupportedSchema`

Normal `ProgramSpace` parsing (`ProgramSpace::from_json_slice` and any typed
extractor funnel) accepts only `reviewgraphen.program_space.input.v2`. A
recognized `reviewgraphen.program_space.input.v1` discriminator returns a
typed `MigrationRequired` error (not a generic validation failure) naming the
detected version. Any other or missing schema discriminator returns a typed
`UnsupportedSchema` error. Neither typed error is a `DomainError::Validation`
string; both are distinguishable variants a caller can branch on.

### 7. Explicit v1→v2 migration API

`migrate_program_space_v1_to_v2(input: &[u8]) -> Result<(ProgramSpace,
MigrationRecord)>` is a separate, explicitly invoked function — not the
normal parse path. It returns the fully built, validated v2 `ProgramSpace`
together with its `MigrationRecord`, not a bare `Extraction`. It never
invents a `source_ids`/`related_capabilities` association the v1 record did
not declare, and it keeps every migrated capability consistent with point
5's per-state contract instead of applying one blanket rule:

- Every migrated capability's `source_ids` is conservatively set to
  `[snapshot_id]`, the only source a bare v1 state can honestly be
  attributed to.
- A `complete` capability gets no synthesized limitation, honoring point 5's
  zero-related-limitation rule for `complete`; its lost v1 source trace is
  recorded only in the `MigrationRecord`, never as a ProgramSpace
  limitation.
- A `partial` capability gets one deterministic synthesized `ProjectionLoss`
  limitation related to it, satisfying point 5 without a false
  `capability_missing` claim.
- A `missing` capability gets one deterministic synthesized
  `CapabilityMissing` limitation related to it, satisfying point 5's
  `missing` requirement.
- An `unknown` capability gets one deterministic synthesized `Unknown`-kind
  limitation related to it — `Unknown`, not `ProjectionLoss`, because
  `unknown` means the v1 record never established a completeness result at
  all, which `LimitationKind::Unknown` already exists to express.
- Each synthesized limitation has a stable ID derived from the capability
  name, its state, and the source schema version; its own `source_ids` of
  `[snapshot_id]`; and `Severity::Info`, since the limitation records a
  migration artifact rather than an assessed review risk.
- Every pre-existing v1 limitation is carried over with its `id`, `kind`,
  `description`, and `severity` unchanged and an empty `related_capabilities`
  set. The preserved v1 schema never declared `related_capabilities` on a
  limitation, so there is nothing to carry from it, and migration never
  guesses which v1 limitation belongs to which capability. If a
  carried-over limitation's `source_ids` was empty (legal under v1, not
  under point 4), it is likewise backfilled with `[snapshot_id]`. A
  nonempty `source_ids` is carried through as the same set, unchanged —
  migration itself never inspects, prunes, or repairs it. This is not an
  exemption from validation: the migrated `Extraction` still passes through
  `ProgramSpaceBuilder::build` (detailed below), which rejects a dangling or
  limitation-cycle-only nonempty `source_ids` exactly as it would reject one
  from any other construction path.

Every synthesized limitation, every backfilled `source_ids`, and every
carried-over limitation's original v1 trace (empty or not) is recorded in
the returned `MigrationRecord` — schema `reviewgraphen.program_space.migration.v1`,
`schemas/reviewgraphen.migration.schema.json` — so the loss stays visible to
the caller instead of being silently absorbed into an otherwise-normal v2
value.

The migrated facts and `Extraction` still pass through the same
`ProgramSpaceBuilder::build` contract every other construction path uses.
A carried-over limitation's nonempty `source_ids` that turns out dangling,
self-referential, or part of a limitation-only cycle (points 3–4) is not
special-cased by migration: `build` rejects it exactly as it would reject
any other invalid `ProgramSpace`, `migrate_program_space_v1_to_v2` returns
that typed error, and no `MigrationRecord` is returned. A migration either
fully succeeds with its declared losses or fails closed with none.

`schemas/reviewgraphen.migration.schema.json` pins `source_schema`/
`target_schema` to the exact v1/v2 discriminators and constrains each
`synthesized_limitation` loss's `state`/`limitation_kind` pair to the three
rows in this section's table via a nested `oneOf`, so a mismatched pairing
or a `complete`-state synthesized limitation is schema-invalid. JSON Schema
cannot express this decision's cross-field semantics beyond per-field
shape: that every backfilled `source_ids` equals exactly `[snapshot_id]`,
that a loss's `capability`/`limitation_id` resolves within the same
migrated `ProgramSpace`, and that `losses` is ID-ordered are guarantees
`migrate_program_space_v1_to_v2` itself enforces, not the schema.

### 8. Duplicate capability key rejection

The `capabilities` map uses a custom `Deserialize` boundary (a `Visitor`
walking JSON map entries directly, not `serde_json::Map`/`BTreeMap`'s default
last-write-wins behavior) that rejects a repeated JSON key in the
`capabilities` object as a typed error before any value is discarded.

### 9. Partial-capability obligation synthesis

`materialize` in `synthesize.rs` keeps producing the concrete obligation
grounded in already-extracted facts for a non-`complete` capability, carrying
the existing qualification-ID linkage to its justifying limitation(s)
(point 5).

Separately, for every rule whose required capabilities are not all
`complete`, synthesis adds exactly one capability-gap obligation for that
rule — one per rule, not one per capability, so a rule missing two
capabilities still gets a single gap obligation whose applicability reasons
name every missing capability. It is added to the denominator alongside any
concrete obligation the same rule also produced, not instead of it, and
carries the same qualification-ID linkage (point 5) to the limitation(s)
justifying its missing capabilities.

A gap obligation's ID binds its origin rule and fixed snapshot target, but
that alone is capability-state-independent: it would keep the same ID
whether a required capability is `partial` or later becomes `missing`. So
the ID additionally binds the exact set of capability-gap reason tags
grounding that gap. A capability-state change that alters which reasons
apply therefore mints a new, distinct gap obligation ID rather than silently
keeping the old one attached to a materially different gap.

`capability_gap_reason` stops collapsing `missing`, `unknown`, and an
undeclared capability into the shared `capability_missing:{capability}`
string: `partial`, `missing`, `unknown`, and undeclared (absent from the
`capabilities` map) each get their own reason tag, so a reviewer can tell
adapter failure, adapter-reported unknown, and an undeclared capability
apart.

### 10. Obligation ID stability contract

A semantic concrete obligation's ID (derived from target/property/context,
independent of capability state) does not change when the capability state
backing it changes — `partial` → `complete` for the same capability must not
mint a new ID for an obligation whose target/property/context is unchanged;
only its `applicability` and qualification IDs change. The qualified universe
ID (`universe.id`) does change, because capability state is already a
qualifying input to that identity per the existing `schemas/README.md`
contract. This ADR does not change that existing universe-ID input set; it
only adds `CapabilityDeclaration.source_ids` as data the universe ID's
existing capability qualification already covers.

### Compatibility

- v1 schema, example, and `Extraction`'s current bare-`CapabilityState` shape
  remain available under explicit `.v1` names, matching the obligation
  schema precedent.
- `scripts/validate_bundle.py` and CI continue to validate the generic path,
  which now means v2; no script changes are required by this ADR beyond what
  the generic-path content swap already implies.
- Producers currently emitting v1 must call the explicit migration API or
  regenerate a v2 record; there is no implicit runtime fallback.

### Rollout

1. Land the v2 schema, `.v1` preservation, and typed `MigrationRequired`/
   `UnsupportedSchema` parse errors together with the `Extraction`/
   `CapabilityDeclaration`/`Limitation` trace-resolution changes.
2. Land the explicit migration API and `MigrationRecord` in the same change
   set, since the parse boundary in step 1 makes v1 input otherwise
   unusable.
3. Update the checked-in v1 example/fixtures only by copying them to `.v1`
   names; author new v2 fixtures separately rather than mutating the
   preserved v1 ones.

### Tests

- Round-trip: a v2 record with resolved `source_ids` on every capability and
  limitation parses and re-serializes to identical canonical bytes.
- Rejection: unresolved capability/limitation `source_ids`, a
  limitation-only circular trace, a `related_capabilities` entry naming an
  undeclared capability, and each cross-field contract row in point 5 (e.g.
  `missing` with no `capability_missing` limitation, `partial` with a
  `capability_missing` limitation, `complete` with any related limitation)
  each fail with a distinguishable typed error.
- Duplicate key: a `capabilities` object with a repeated JSON key is
  rejected before deserialization completes.
- Schema-version boundary: v1 input on the normal parse path returns
  `MigrationRequired`; an unknown/missing discriminator returns
  `UnsupportedSchema`; neither silently degrades to a partial parse.
- Migration: migrating a v1 fixture produces a v2 `Extraction` whose
  synthesized limitation kind matches the source capability state (none for
  `complete`, `ProjectionLoss` for `partial`, `CapabilityMissing` for
  `missing`, `Unknown` for `unknown`); synthesized/backfilled `source_ids`
  and the `MigrationRecord` are deterministic across repeated runs on the
  same input, and pre-existing v1 limitations keep their original
  `related_capabilities` unchanged.
- Synthesis: a `partial` capability with grounded facts produces both its
  concrete obligation (stable ID, unchanged across a later `partial` →
  `complete` transition for the same target/property/context) and its
  origin rule's capability-gap obligation; a rule missing two capabilities
  produces exactly one gap obligation naming both; a capability-state change
  that alters a rule's gap reasons mints a new gap obligation ID rather than
  reusing the old one; `missing`, `unknown`, and undeclared capability
  reasons are pairwise distinguishable in the coverage denominator.

## Consequences

### Positive

- A capability declaration is independently checkable against the
  ProgramSpace it travels with, instead of being an assertion with no trace.
- `missing`/`unknown`/undeclared capability gaps are distinguishable in the
  coverage denominator, closing a real gap in the current
  `capability_gap_reason` collapse.
- Duplicate-key and schema-version drift fail closed instead of silently
  keeping the last value or accepting an unversioned shape.
- v1 producers get an explicit, loss-declaring migration path rather than
  silent acceptance or an unrecoverable break.

### Negative

- Every capability declaration now requires at least one resolvable
  `source_id`, which is a stricter producer obligation than v1's bare state.
- `partial`/`missing`/`unknown` now require an accompanying limitation,
  which existing hand-written fixtures without one will fail until updated.
- Migration is lossy by design (point 7); consumers relying on a lossless
  v1→v2 upgrade must instead read the `MigrationRecord`.
- Two additional typed error variants (`MigrationRequired`,
  `UnsupportedSchema`) and one new custom deserializer are new surface area
  to keep deterministic under `crates/reviewgraphen-core`'s existing
  boundary tests.

## Alternatives considered

### A. Keep `capabilities` as a bare state map and add trace only to `Limitation`

Rejected. A `complete`/`partial`/`missing`/`unknown` declaration with no
`source_ids` of its own is still an unauditable assertion even if every
related limitation is traced; the capability declaration itself is the
claim that needs grounding.

### B. Silent last-value-wins on duplicate capability keys (default `serde_json` map behavior)

Rejected. It contradicts the existing `additionalProperties: false`
typo-detection posture in `schemas/README.md` and would let a producer bug
silently downgrade or upgrade a capability's declared state.

### C. Implicit v1 fallback inside the normal parse path

Rejected. A silent fallback hides the schema-version boundary from the
caller and cannot express `MigrationRecord`'s information loss; AGENTS.md's
prohibition on silent schema migration (`docs/14_report_and_schema_contract.md`
§18: "schema migrationをsilentに行わない") applies directly.

### D. Best-effort inference of v1 capability sources from co-located facts

Rejected. Guessing an association a v1 record never declared would fabricate
provenance instead of declaring the loss, contradicting the
"AIが生成した構造をconfidenceだけでacceptedへ昇格しない" boundary and this
project's general preference for explicit unknowns over inferred ones.

### E. Collapse `missing`/`unknown`/undeclared into one denominator reason permanently

Rejected as the status quo. It is exactly the gap this ADR closes; keeping
it would leave the coverage denominator unable to distinguish "an adapter
tried and failed" from "no rule input ever named this capability".

## Invariants

- Every `CapabilityDeclaration.source_ids` and `Limitation.source_ids` entry
  resolves to an existing ID in the same ProgramSpace.
- No limitation's source trace is self-referential or grounds out only in
  other limitations.
- `related_capabilities` only names capabilities declared in the same
  `Extraction.capabilities` map.
- `complete` has zero related limitations; `partial`, `missing`, and
  `unknown` each have at least one source-backed related limitation, with
  `missing` requiring and `partial` forbidding a `capability_missing` kind.
- Normal parsing accepts only v2; v1 input yields `MigrationRequired` and an
  unrecognized schema yields `UnsupportedSchema`, never a partial parse.
- Migration never infers an association absent from the v1 source: it
  synthesizes at most one deterministic limitation per capability, of the
  kind point 7 maps to that capability's state (none for `complete`), and
  carries pre-existing v1 limitations over with their `related_capabilities`
  unchanged. Every synthesized limitation, backfilled `source_ids`, and
  unresolved original v1 trace is recorded in a `MigrationRecord`.
- A duplicate JSON key in `capabilities` is rejected before any value from
  that key is retained.
- A semantic concrete obligation's ID does not change when only the
  capability state backing it changes; `universe.id` does change, since
  capability state remains a qualifying input to universe identity.
- A capability-gap obligation is one per rule with a missing capability
  requirement, not one per missing capability; its ID additionally binds
  the exact capability-gap reason set backing it, so a capability-state
  change that alters those reasons never reuses the prior gap's ID.
- `missing`, `unknown`, and undeclared capability-gap reasons are
  distinguishable strings, never collapsed into one shared reason.

## Revisit triggers

- A second schema needs the same `CapabilityDeclaration`/source-trace shape,
  suggesting the pattern should move to a shared cross-schema type instead of
  being defined once for `program_space.input`.
- Empirical use shows the required-limitation rule for `partial`/`missing`/
  `unknown` produces excessive fixture friction relative to the trace
  benefit, motivating a narrower required-correspondence rule.
- A future schema version needs a second migration hop (v2→v3); the
  migration API and `MigrationRecord` shape defined here should be
  generalized rather than duplicated per version pair.
