# ReviewGraphen Schemas

> Status: Draft v0.1  
> JSON Schema dialect: Draft 2020-12

## Files

| File | Schema ID / purpose |
| --- | --- |
| `reviewgraphen.input.schema.json` | `reviewgraphen.program_space.input.v2` — current language-neutral ProgramSpace ingestion input; this stable generic path is used by the validation script。 |
| `reviewgraphen.input.v1.schema.json` | Preserved `reviewgraphen.program_space.input.v1` legacy contract。 |
| `reviewgraphen.obligation.schema.json` | `reviewgraphen.review_obligations.v2` — current versioned obligation universe and obligations; this stable generic path is used by the validation script。 |
| `reviewgraphen.obligation.v1.schema.json` | Preserved `reviewgraphen.review_obligations.v1` legacy contract。 |
| `reviewgraphen.report.schema.json` | `reviewgraphen.review.report.v1` — end-to-end review report。 |
| `reviewgraphen.report.v2.schema.json` | Accepted authority-free `reviewgraphen.review.report.v2` D2 design contract (ADR 0018); it requires declared tail metadata, fixed proposed/unreviewed claims, and zero verified/accepted coverage, while source confirmation remains outside JSON Schema and v1 stays frozen。 |
| `reviewgraphen.report.v2.example.json` | Local-only, claim-free fake-reviewer abstention example; it demonstrates explicit null model fields, no-tools trace, honest partial coverage, recoverable and nonrecoverable per-view losses, and no fabricated review claim。 |
| `reviewgraphen.migration.schema.json` | `reviewgraphen.program_space.migration.v1` — explicit v1→v2 `MigrationRecord` output of `migrate_program_space_v1_to_v2`。 |
| `reviewgraphen.input.example.json` | Current v2 double-submit ProgramSpace fixture, with source-traced `CapabilityDeclaration` capabilities。 |
| `reviewgraphen.input.v1.example.json` | Preserved `reviewgraphen.program_space.input.v1` double-submit ProgramSpace fixture。 |
| `reviewgraphen.obligation.example.json` | Current v2 nine obligations: five concrete Node/Relation/Path/Invariant obligations plus four `capability_gap.origin_rule@1` obligations (one per rule requiring the fixture's `partial` `concurrency_model`)。 |
| `reviewgraphen.obligation.v1.example.json` | Preserved reviewed v1 obligation fixture。 |
| `reviewgraphen.report.example.json` | Claim、evidence binding、verification、decision、finding、gluing、coverageの参照report。 |
| `reviewgraphen.migration.example.json` | The exact canonical `migrate_program_space_v1_to_v2` output for `reviewgraphen.input.v1.example.json`: 5 `capability_source_backfill` losses, 2 `synthesized_limitation` losses (the fixture's two `partial` capabilities), and 1 `carried_limitation_trace` loss for its one nonempty-source v1 limitation。 |
| `reviewgraphen.migration.example.sha256` | SHA-256 of `reviewgraphen.migration.example.json`'s canonical bytes, checked byte-for-byte against a real `migrate_program_space_v1_to_v2` run in `tests/m1.rs`。 |
| `reviewgraphen.extraction_report.v1.schema.json` | `reviewgraphen.extraction_report.v1` — `reviewgraphen-ingest`'s public M2 adapter completeness/obstruction report shape (`ExtractionReport`)。 |
| `reviewgraphen.extraction_report.v1.example.json` | A frozen, self-consistent `ExtractionReport` instance from a real M2 fixture ingest。 |
| `reviewgraphen.extraction_report.v1.example.sha256` | SHA-256 of the example's own canonical bytes (self-consistency only — see below, this is not byte-compared against a fresh ingest run)。 |
| `reviewgraphen.config.example.toml` | CLI/local runtime configuration example。 |

## Validation layers

JSON Schemaはshape、required field、enum、basic rangeを確認します。次はruntime cross-record validatorが担当します。

- ID uniqueness。
- target/source/reference existence。
- coverage numerator ≤ denominator。
- universe `obligation_ids`と実体の一致。
- illegal lifecycle/disposition/verification transitions。
- accepted claim/findingにrequired decisionがあること。
- verificationにverifierとvalid evidenceがあること。
- stale recordをfresh coverageへ含めないこと。
- projection `source_ids`の解決。
- context/gluing consistency。

Schema validation成功だけをsemantic validityとみなしません。

`reviewgraphen.report.v2.example.json` is intentionally claim-free: it records
one fake-reviewer abstention and therefore cannot be mistaken for a normative
review conclusion. Its execution identity, raw-artifact registration, raw
hash/size, and execution body hash are self-consistent under the bundle's
float-free local reference encoding. That is not proof that the core emitted
canonical bytes; runtime must still perform the cross-record checks that JSON
Schema and this example cannot express.

The report-only gate cannot confirm its own tail or source closure. No
source-bound fixture is checked in yet: it must eventually be generated from
the actual core canonical event/CAS output and actual v3 index rebuild, not
authored as arbitrary JSON. The current example and validator deliberately
make no source-proof, confirmed-tail, CAS-presence, denominator, or lifecycle
claim.

`python3 scripts/validate_bundle.py` includes the v2 schema/example pair in its
actual gate. It additionally checks canonical preimages/order, exact internal
claim/raw-registration sets, authority/status/coverage constraints, nonempty
losses, omission/duplicate/reorder mutations, a valid zero-attempt
`unsupported_input`, rejection of unsupported-with-attempt and reserved
`failed`, actual UTF-8 byte and canonical-body caps, strict non-standard JSON
constant rejection, finite `[0,1]` D2 floats, per-view loss rules, and checked
`u64` add/multiply reference arithmetic. It does not model Rust allocator
capacity or claim the normative runtime `J+I+Rr` / `J+I+R+S+O` proof; those
exact/+1 tests remain implementation Definition of Done.

## Example validation

Python `jsonschema`を利用する場合:

```bash
python - <<'PY'
import json
from pathlib import Path
from jsonschema import Draft202012Validator

root = Path("schemas")
pairs = [
    ("reviewgraphen.input.schema.json", "reviewgraphen.input.example.json"),
    ("reviewgraphen.obligation.schema.json", "reviewgraphen.obligation.example.json"),
    ("reviewgraphen.report.schema.json", "reviewgraphen.report.example.json"),
    ("reviewgraphen.report.v2.schema.json", "reviewgraphen.report.v2.example.json"),
    ("reviewgraphen.migration.schema.json", "reviewgraphen.migration.example.json"),
]

for schema_name, example_name in pairs:
    schema = json.loads((root / schema_name).read_text())
    example = json.loads((root / example_name).read_text())
    Draft202012Validator(schema, format_checker=None).validate(example)
    print("ok", example_name)
PY
```

## Versioning

- Schema IDの末尾にmajor versionを含める。
- field semantics変更はmajor update。
- additive optional fieldはcompatibility note付きで同majorに追加可能。
- enum追加はconsumerのexhaustivenessを壊す可能性があるためminor扱いにしない場合がある。
- old fixtureとmigration testを保持する。
- canonical stateにschema-less JSONを保存しない。

Obligation output is currently v2. It adds public generator provenance and explicit
`universe.exclusions` records (reason, source trace, and positive JSON number
weight). The v1 schema and reviewed fixture remain available under the explicit
`.v1` names for compatibility tests. V1 did not preserve generator or exclusion
trace, so a v1 bundle must be re-synthesized from its ProgramSpace; it cannot be
losslessly migrated into v2 from JSON alone. Compatibility checks compare the
retained snapshot and each obligation's semantic tuple (target kind, ordered
path refs where applicable, property ID/version, normalized set-like context
IDs/capabilities/relation kinds, and scalar relation-depth/test/evidence
inclusion semantics), never the version-specific IDs. V2 then recovers generator, origin-rule when a
capability gap exists, and exclusion traces from the retained ProgramSpace.

Capability gaps are not exclusions. They remain denominator members with
`applicability.status: unknown` and a dedicated versioned capability-gap rule;
the generic current schema path continues to validate v2 output without changes
to the repository validation script.

The current v2 `universe.id` binds the explicit eligible obligation IDs and
explicit exclusions, plus the canonical extractor capability, adapter
completeness, and limitation inputs that qualify that denominator. Reordering
those set-like inputs does not change the ID; changing the explicit denominator
or a qualifying capability/completeness input does.

ProgramSpace input is currently `reviewgraphen.program_space.input.v2`. Each
`extraction.capabilities` value is a `CapabilityDeclaration` object
(`state` plus non-empty, unique `source_ids`) instead of a bare completeness
state string, and `extraction.limitations[].source_ids` is now required and
non-empty; `related_capabilities` is an explicitly allowed limitation
property. The v1 schema and fixture remain available under the explicit
`.v1` names for compatibility tests. See
[ADR 0011](../docs/adr/0011-program-space-v2-capability-trace.md) for the
full cross-field completeness contract, the resolvable `source_ids` ID set,
and the explicit v1→v2 migration path.

`reviewgraphen.program_space.migration.v1` is the `MigrationRecord` that
`migrate_program_space_v1_to_v2` returns alongside the migrated
`ProgramSpace`. `source_schema`/`target_schema` are pinned `const` values
(`reviewgraphen.program_space.input.v1`/`.v2`), not free-form strings. Its
`losses` array is a closed, internally-tagged union of exactly four
`MigrationLoss` kinds (`capability_source_backfill`,
`synthesized_limitation`, `carried_limitation_trace`,
`limitation_source_backfill`); each variant schema uses
`additionalProperties: false` and a `const` discriminator so an unknown
kind, a field from a different variant, or an empty `assigned_source_ids`
on a backfill loss all fail validation rather than being silently accepted.
A `synthesized_limitation` loss additionally constrains `state`/
`limitation_kind` to exactly the three valid pairs (`partial`/
`projection_loss`, `missing`/`capability_missing`, `unknown`/`unknown`) via
a nested `oneOf`, rejecting `complete` and any mismatched pairing. See
ADR 0011 §7 for what each loss kind records and when it is emitted.

`reviewgraphen.migration.example.json` is not a hand-authored shape
illustration: it is the literal, byte-exact canonical output of
`migrate_program_space_v1_to_v2` run on the checked-in
`reviewgraphen.input.v1.example.json`, including its real content-derived
`migration:`/`migration_loss:`/`limitation:` IDs. The
`migration_record_matches_the_checked_in_canonical_fixture_byte_for_byte`
test in `tests/m1.rs` re-runs that same migration and asserts the canonical
bytes — and the paired `reviewgraphen.migration.example.sha256` hash —
match exactly, so an ID-derivation, ordering, or field change in the
migration implementation fails this test instead of passing silently
against a schema-only or self-consistency check. `migration_record_schema_validates_actual_migration_output`
separately re-validates that same real output against the schema (shape,
not exact bytes). Because the source v1 fixture's only limitation has a
nonempty `source_ids`, this example does not exercise a
`limitation_source_backfill` loss; that shape (and every state/kind
pairing) is instead covered by the schema's dedicated positive/negative
tests, which mutate a copy of this fixture rather than relying on the
example to contain every case. Cross-field semantics this schema cannot
express — for example that every `assigned_source_ids` equals exactly
`[snapshot_id]`, that `limitation_id`/`capability` values resolve within
the migrated `ProgramSpace`, or that `losses` is actually ID-ordered — are
Rust contract guarantees enforced by `migrate_program_space_v1_to_v2`
itself (ADR 0011 §7), not by this JSON Schema.

`reviewgraphen.extraction_report.v1` is `reviewgraphen-ingest`'s own public
report type (`ExtractionReport`), separate from the `reviewgraphen.program_
space.input.v2` document it accompanies. Each `adapters[]` entry reports an
explicit discovered/accepted/excluded/failed denominator: `total` is every
discovered input, `parsed` is the accepted subset, `excluded` is the subset a
declared bound deliberately excluded (for example a symlink or a Git
submodule entry — every excluded input is retained as a typed `obstructions[]`
entry, never silently dropped), and `failed` is the subset the adapter
attempted but could not handle (for example a Rust parse failure).
`parsed + excluded + failed == total` whenever all four are present; a `null`
value means the concept is not meaningful for that adapter (Cargo package
metadata has no per-item exclude/fail concept). Every `obstructions[]` entry's
`source_ids` is required non-empty, mirroring the same non-empty source-trace
contract ADR 0011 requires of a `ProgramSpace` `Limitation`.

Unlike `reviewgraphen.migration.example.json` (derived from a fixed,
timestamp-free v1 JSON fixture and therefore byte-reproducible),
`reviewgraphen.extraction_report.v1.example.json` is derived from a real
local `git commit`, whose hash embeds a wall-clock timestamp — it cannot be
byte-reproduced across runs. The checked-in example is instead a frozen,
self-consistent instance: its own sha256 is checked against its own
canonical bytes (catching an accidental hand-edit), and a *separate* test
(`a_real_ingest_run_extraction_report_validates_against_the_public_schema` in
`crates/reviewgraphen-ingest/tests/m2.rs`) validates a fresh, live `ingest()`
run's report against this same schema, without a byte comparison to the
frozen example. `crates/reviewgraphen-ingest/tests/extraction_report_schema.rs`
covers the schema's own validity, the example's schema/hash self-consistency,
and negative cases (missing required field, wrong `schema` discriminator,
empty `source_ids`, unknown top-level field, invalid enum value).

## Trust boundary

- Input、report、import bundleはすべてuntrusted dataとしてdeserializeする。
- `additionalProperties: false`はtypoやsilent field driftを早期に見つけるための初期方針。
- extensionが必要になった場合、明示的な`extensions` objectとnamespace policyを追加する。
- raw source、model output、test logはreportへ大量埋め込みせずcontent-addressed artifact refにする。
