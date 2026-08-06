# ReviewGraphen Schemas

> Status: Draft v0.1  
> JSON Schema dialect: Draft 2020-12

## Files

| File | Schema ID / purpose |
| --- | --- |
| `reviewgraphen.input.schema.json` | `reviewgraphen.program_space.input.v1` — language-neutral ProgramSpace ingestion input。 |
| `reviewgraphen.obligation.schema.json` | `reviewgraphen.review_obligations.v1` — versioned obligation universe and obligations。 |
| `reviewgraphen.report.schema.json` | `reviewgraphen.review.report.v1` — end-to-end review report。 |
| `reviewgraphen.input.example.json` | Double-submit ProgramSpace fixture。 |
| `reviewgraphen.obligation.example.json` | Five Node/Relation/Path/Invariant obligations。 |
| `reviewgraphen.report.example.json` | Claim、evidence binding、verification、decision、finding、gluing、coverageの参照report。 |
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

## Trust boundary

- Input、report、import bundleはすべてuntrusted dataとしてdeserializeする。
- `additionalProperties: false`はtypoやsilent field driftを早期に見つけるための初期方針。
- extensionが必要になった場合、明示的な`extensions` objectとnamespace policyを追加する。
- raw source、model output、test logはreportへ大量埋め込みせずcontent-addressed artifact refにする。
