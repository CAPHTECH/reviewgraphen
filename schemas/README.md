# ReviewGraphen Schemas

> Status: Draft v0.1  
> JSON Schema dialect: Draft 2020-12

## Files

| File | Schema ID / purpose |
| --- | --- |
| `reviewgraphen.input.schema.json` | `reviewgraphen.program_space.input.v2` — current language-neutral ProgramSpace ingestion input; this stable generic path is used by the validation script。 |
| `reviewgraphen.input.v1.schema.json` | Preserved `reviewgraphen.program_space.input.v1` legacy contract。 |
| `reviewgraphen.input.v3.schema.json` | Additive M6 ProgramSpace carrier requiring a resolved Git commit/tree closure, complete version-pinned Rust symbol anchors, and ordered relation endpoints; v1/v2 remain frozen。 |
| `reviewgraphen.input.v3.example.json` | Strict v3 double-submit ProgramSpace fixture validated by both JSON Schema and the Core runtime boundary。 |
| `reviewgraphen.obligation.schema.json` | Current obligation alias. It will advance only through an explicit major schema/version pair; the former v2 bytes are retained below. |
| `reviewgraphen.obligation.v2.schema.json` | Preserved exact `reviewgraphen.review_obligations.v2` schema formerly served by the current alias. |
| `reviewgraphen.obligation.v1.schema.json` | Preserved `reviewgraphen.review_obligations.v1` legacy contract。 |
| `reviewgraphen.report.schema.json` | `reviewgraphen.review.report.v1` — end-to-end review report。 |
| `reviewgraphen.report.v2.schema.json` | Accepted authority-free `reviewgraphen.review.report.v2` D2 design contract (ADR 0018); it requires declared tail metadata, fixed proposed/unreviewed claims, and zero verified/accepted coverage, while source confirmation remains outside JSON Schema and v1 stays frozen。 |
| `reviewgraphen.report.v2.example.json` | Local-only, claim-free fake-reviewer abstention example; it demonstrates explicit null model fields, no-tools trace, honest partial coverage, recoverable and nonrecoverable per-view losses, and no fabricated review claim。 |
| `reviewgraphen.report.v3.schema.json` | Frozen source-bound M4 `reviewgraphen.review.report.v3` contract。 |
| `reviewgraphen.report.v3.example.json` | Public-pipeline M4 source-bound report fixture。 |
| `reviewgraphen.report.v4.schema.json` | Closed additive M5 `reviewgraphen.review.report.v4` contract with nested input registrations/descriptors and index-v5 gluing records。 |
| `reviewgraphen.report.v4.example.json` | Canonical conflict-path Report V4 emitted by the public journal/CAS/index/runtime/report pipeline。 |
| `reviewgraphen.report.v4.example.sha256` | SHA-256 of the Report V4 example's canonical bytes, checked against a fresh public-pipeline generation。 |
| `reviewgraphen.report.v5.schema.json` | Closed M6 incremental report contract. It adds the dual-run source/target authority coordinates, explicit M6 coverage axes, separate preservation registrations, and the report-only `reviewgraphen.incremental_gate.v5`. A report remains a projection: it cannot reconstruct or replace the source-bound V5 journal/index/CAS authority. |
| `reviewgraphen.migration.schema.json` | `reviewgraphen.program_space.migration.v1` — explicit v1→v2 `MigrationRecord` output of `migrate_program_space_v1_to_v2`。 |
| `reviewgraphen.input.example.json` | Current v2 double-submit ProgramSpace fixture, with source-traced `CapabilityDeclaration` capabilities。 |
| `reviewgraphen.input.v1.example.json` | Preserved `reviewgraphen.program_space.input.v1` double-submit ProgramSpace fixture。 |
| `reviewgraphen.obligation.example.json` | Current obligation-alias example; its version is fixed by the matching current schema. |
| `reviewgraphen.obligation.v2.example.json` | Preserved exact former current-alias v2 example: five concrete Node/Relation/Path/Invariant obligations plus four `capability_gap.origin_rule@1` obligations. |
| `reviewgraphen.obligation.v1.example.json` | Preserved reviewed v1 obligation fixture。 |
| `reviewgraphen.report.example.json` | Claim、evidence binding、verification、decision、finding、gluing、coverageの参照report。 |
| `reviewgraphen.migration.example.json` | The exact canonical `migrate_program_space_v1_to_v2` output for `reviewgraphen.input.v1.example.json`: 5 `capability_source_backfill` losses, 2 `synthesized_limitation` losses (the fixture's two `partial` capabilities), and 1 `carried_limitation_trace` loss for its one nonempty-source v1 limitation。 |
| `reviewgraphen.migration.example.sha256` | SHA-256 of `reviewgraphen.migration.example.json`'s canonical bytes, checked byte-for-byte against a real `migrate_program_space_v1_to_v2` run in `tests/m1.rs`。 |
| `reviewgraphen.extraction_report.v1.schema.json` | `reviewgraphen.extraction_report.v1` — `reviewgraphen-ingest`'s public M2 adapter completeness/obstruction report shape (`ExtractionReport`)。 |
| `reviewgraphen.extraction_report.v1.example.json` | A frozen, self-consistent `ExtractionReport` instance from a real M2 fixture ingest。 |
| `reviewgraphen.extraction_report.v1.example.sha256` | SHA-256 of the example's own canonical bytes (self-consistency only — see below, this is not byte-compared against a fresh ingest run)。 |
| `reviewgraphen.config.example.toml` | CLI/local runtime configuration example。 |
| `reviewgraphen.benchmark.trial_manifest.v1.schema.json` | M7 non-authority trial protocol, exact packet set, source inventory, ontology and protocol versions. |
| `reviewgraphen.benchmark.candidate_output.v1.schema.json` | Shared B1/G3-proxy candidate output using the closed arm-neutral mechanism ontology. |
| `reviewgraphen.benchmark.oracle.v1.schema.json` | Private deterministic root anchors; never reviewer input. |
| `reviewgraphen.benchmark.score.v1.schema.json` | Non-authority score bound to manifest, candidate, oracle, input tree, and paired execution configuration. |
| `reviewgraphen.benchmark.collection.v1.schema.json` | Collector-owned outcome; only this record may mark a trial `protocol_invalid`. |
| `reviewgraphen.benchmark.trial_inventory.v1.schema.json` | Complete expected trial denominator with exact manifest and paired-configuration hashes. |
| `reviewgraphen.benchmark.run_summary.v1.schema.json` | Inventory-driven prepared/valid/protocol-invalid/binding-invalid/missing counts, eligible pairs, and exclusion reasons. |
| `reviewgraphen.benchmark.real_unit.v1.schema.json` | Private real-fix pair binding with machine-recorded parent-fails/fix-passes regression evidence; never reviewer input. |
| `reviewgraphen.benchmark.real_oracle.v1.schema.json` | Private target-only positive/control oracle with code-fix-hunk anchors and explicit control semantics. |
| `reviewgraphen.benchmark.real_score.v1.schema.json` | Target-root score that keeps every matched-fix-control finding unlabeled pending adjudication. |
| `reviewgraphen.benchmark.real_trial_inventory.v1.schema.json` | Private four-cell inventory for positive/control by B1/G3, without leaking roles to reviewers. |
| `reviewgraphen.benchmark.real_run_summary.v1.schema.json` | Real-fix paired recall and separately named unlabeled-control/target-anchor allegation counts; no false-positive claim. |
| `reviewgraphen.benchmark.real_full_run_summary.v1.schema.json` | Additive full-ReviewGraphen-only real-fix summary; the frozen B1/G3 paired summary remains unchanged. |
| `benchmarks/structural-sloppiness-v1/schema/structural-sloppiness-report-v1.schema.json` | Closed experimental v1 report for the fixed `changed_input_consumer_bridge_mismatch@1` benchmark classifier; it is not a product or authority schema. |
| `benchmarks/structural-sloppiness-v1/example-missing-bridge-report.json` | Generated synthetic missing-bridge report, validated against the adjacent experimental schema. |
| `reviewgraphen.process_reviewer_record.v1.schema.json` | Hash-bound raw Codex/Claude process observation for deterministic replay; explicitly non-authority. |
| `reviewgraphen.generic_review_request.v1.schema.json` | Ordinary Git generic orchestration request with explicit ingest/plan bounds and a swappable local-process or replay backend. |
| `reviewgraphen.generic_review_request.v2.schema.json` | ADR 0038 closed generic request: admitted local Git roots, `rust.production.v1` ingest bounds, the D relation review profile, bounded plan, provider-free/replay observer selection, and the deferred workspace verifier selection. |
| `reviewgraphen.reviewer_output.v1.schema.json` | Preserved strict non-authority reviewer output contract; root-level exclusivity makes it unsuitable as the current provider Structured Outputs schema. |
| `reviewgraphen.reviewer_output.v2.schema.json` | Provider-facing non-authority output with a root object and a nested closed `anyOf` between structured claims and abstention; new generic packets emit v2 and deterministically lower it through the strict v1 parser. |
| `reviewgraphen.generic_review_run.v1.schema.json` | Denominator-preserving generic run projection with raw process records, parsed proposal outcomes, and a fixed non-authority/incomplete ceiling. |
| `reviewgraphen.generic_review_run.v2.schema.json` | ADR 0038 canonical non-authority audit projection. It records the D two-layer obligation denominator, `context.subject_windows@2`, observation/coverage partitions, partial direct-call enumeration trace, a closed deferred verifier result, and a fixed incomplete authority ceiling. |
| `reviewgraphen.generic_review_request.v4.schema.json` | Closed mixed production-v4 request with the exact D→`@3` and Node→`@4` policy registry. |
| `reviewgraphen.generic_review_run.v4.schema.json` | Closed non-authority mixed audit with ordered D two-layer and Node single-layer coverage; Node has no candidate-gap or `direct_calls` fields. |
| `reviewgraphen.generic_review_human_report.v1.schema.json` | Closed non-authority manifest derived only from a validated run-v2 audit; it binds audit and Markdown hashes, source/window trace, meaningful loss, proposal/abstention state, and verifier status. |
| `reviewgraphen.generic_review_human_report.v3.schema.json` | Closed non-authority projection of a basis-bound run-v4, retaining separate D limitation and Node public-function scope. |
| `reviewgraphen.generic_review_diagnostics.v1.schema.json` | Closed non-authority operational diagnostic with nullable request/run bindings and exactly six ordered stage observations; the adjacent `.example.json` uses real v3 run bindings, while elapsed microseconds remain non-canonical operational observations. |
| `reviewgraphen.responsibility_family_state.v1.schema.json` | Accepted responsibility-family product state, kept separate from ProgramSpace and requiring a human decision, Evidence, Verification, distinct implementation/endpoint denominators, purpose constraints, unknowns, and a fixed non-sign-off authority ceiling. |
| `reviewgraphen.responsibility_family_state.v1.example.json` | Canonical example of an externally accepted shared-conformance-test family; it is not evidence that the illustrated FSL family was actually accepted. |
| `provider-free.source-grounded-packet.v1.schema.json` | Closed provider-free reviewer packet containing only admitted source payloads and a fixed abstention response schema; it is not an authority artifact. |
| `provider-free.source-grounded-abstention.v1.schema.json` | The sole fixed output for `deterministic.abstain@1`: an explicit abstention, never a semantic finding. |
| `provider-free.source-inventory.v1.schema.json` | Canonical admitted-source inventory bound into the provider-free packet and abstention. |
| `reviewgraphen.review_profile.v1.schema.json` | Closed `rust.production.v1` profile used by the ADR 0038 changed-public-callee path; declared exclusions remain visible in the obligation universe. |

## Validation layers

JSON Schemaはshape、required field、enum、basic rangeを確認します。次はruntime cross-record validatorが担当します。

- ID uniqueness。
- target/source/reference existence。
- coverage numerator ≤ denominator。
- universe `obligation_ids`と実体の一致。
- illegal lifecycle/disposition/verification transitions。
- Benchmark manifest `paired_configuration_hash` recomputation and inventory pair completeness.
- Candidate finding IDs referenced by obligation results and G3 finding-to-obligation coverage.
- Collection/manifest/candidate/score hash equality and exclusion of invalid or missing trials.
- Oracle input-tree/source-bundle equality plus root path/file-hash/range equality against manifest inventory.
- Score arithmetic equality and paired oracle/configuration compatibility.
- Real-fix presence evidence equality, parent-fails/fix-passes exit semantics, and commit/tree binding.
- Real positive/control four-cell completeness; positive-only target roots; all control findings remain unlabeled.

For benchmark artifacts, JSON Schema checks closed shapes, enums, caps, and local conditionals. The Rust validator is normative for these cross-record relations; a schema-valid record alone is not score-eligible.
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

The report-only v2 gate cannot confirm its own tail or source closure. Its
local example deliberately makes no source-proof, confirmed-tail,
CAS-presence, denominator, or lifecycle claim. The later v3 and v4 examples
are checked separately against actual public source-bound pipelines and are
not authority for reconstructing the journal, CAS, or index from report prose.

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
    ("reviewgraphen.input.v3.schema.json", "reviewgraphen.input.v3.example.json"),
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

The stable generic manual input remains `reviewgraphen.program_space.input.v2`.
M6-capable deterministic Git/Rust ingestion emits the additive
`reviewgraphen.program_space.input.v3` contract instead. V3 requires an exact
lowercase resolved base/target Git commit and tree closure, the fixed
`reviewgraphen.ingest.rust_syn.anchor.v1` / syn `2.0.119` producer binding, a
complete anchor for every accepted Rust function/method/type, and a unique
ordered endpoint sequence for every relation. Core verifies those values
against the accepted source/snapshot/provenance records and relation set; JSON
Schema shape success alone cannot establish the cross-record equalities. The
v1/v2 schemas and canonical bytes are unchanged.

In v2, each
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
