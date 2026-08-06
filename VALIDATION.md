# Bundle Validation

> Validation date: 2026-08-07  
> Validator: `scripts/validate_bundle.py`  
> Environment: bundle construction container

## Result

```text
ReviewGraphen bundle validation: PASS
- parsed JSON: 9 files
- parsed TOML: 2 files
- validated JSON Schema examples: 3
- checked Markdown: 40 files
- checked relative links: 83
- semantic fixture IDs: ProgramSpace=26, obligations=5
- semantic report records: executions=5, claims=5, verifications=5
- Rust fixture: skipped (cargo not installed)
```

## Interpretation

- JSON、TOML、JSON Schema、Markdown relative link、reference ID、coverage denominator、claim/evidence/verification/finding traceをofflineで検査しました。
- `reviewgraphen.input.example.json`、`reviewgraphen.obligation.example.json`、`reviewgraphen.report.example.json`はDraft 2020-12 schema validationを通過しています。
- schema exampleと`examples/double-submit-payment/`のProgramSpace/reportが同一であることを検査しています。
- Rust fixtureのsource pathは検査済みです。

## Known validation limitation

この環境には`cargo`が存在しなかったため、Rust fixtureの`cargo test`だけは実行していません。validatorは`cargo`が存在する環境では自動的にtestを実行します。

```bash
cd examples/double-submit-payment/fixture
cargo test
```

この未実行項目はJSON/schema/document contractのvalidation結果を無効にしませんが、Rust fixtureが実際にcompile/runすることの証拠にはなりません。
