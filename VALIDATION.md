# Bundle Validation

> Validation date: 2026-08-07  
> Validator: `scripts/validate_bundle.py` via `scripts/ci.sh fast`
> Environment: Rust 1.95.0 development harness

## Result

```text
ReviewGraphen bundle validation: PASS
- parsed JSON: 9 files
- parsed TOML: 7 files
- validated JSON Schema examples: 3
- checked Markdown: 42 files
- checked relative links: 87
- semantic fixture IDs: ProgramSpace=26, obligations=5
- semantic report records: executions=5, claims=5, verifications=5
- Rust fixture: cargo test passed
```

## Interpretation

- JSON、TOML、JSON Schema、Markdown relative link、reference ID、coverage denominator、claim/evidence/verification/finding traceをofflineで検査しました。
- `reviewgraphen.input.example.json`、`reviewgraphen.obligation.example.json`、`reviewgraphen.report.example.json`はDraft 2020-12 schema validationを通過しています。
- schema exampleと`examples/double-submit-payment/`のProgramSpace/reportが同一であることを検査しています。
- Rust fixtureのsource pathとcounterexample test実行を検査済みです。

## Harness validation

fast gateではbundle validation、Clippy、nextest、doc testが通過しています。依存ポリシーは`cargo-deny`で検証済みです。

```bash
scripts/ci.sh fast
scripts/ci.sh deny
```

製品Rust sourceはまだ存在しないため、PBTとsnapshotのframeworkはworkspace契約として固定されていますが、製品propertyは未実行です。同じ理由でcoverage modeは明示的にskipします。fixture testのpassは二重課金counterexampleの再現を意味し、安全性のsign-offではありません。
