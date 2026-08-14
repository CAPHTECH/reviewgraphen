# 13. CLI Contract

> Status: Draft v0.1  
> Binary: `reviewgraphen`  
> Design: CLI-first, JSON-contract-first, local-first

Implementation note: ADR 0024 currently implements only the fixed offline
reference command `review --fixture double-submit` and
`schema list|print|validate`. ADR 0028 withdraws the detached-report-only
`gate <report.json>` command. The Store-bound `gate` described later in this
draft remains a future contract, not currently accepted command syntax.

## 1. CLIの役割

CLIは人間向けUIの代替ではなく、次を安定して提供するagent-facing execution surfaceです。

- snapshot作成。
- ProgramSpace ingestion。
- ReviewObligation生成。
- plan作成。
- reviewer/verifier実行。
- coverageとstaleness集計。
- report projection。
- policy gate。
- schema validation。
- audit可能なlocal run。

将来のMCPやhosted APIも同じruntimeとschemaを利用します。

## 2. Command family

```text
reviewgraphen
  init
  snapshot
  ingest
  obligations
  plan
  run
  verify
  glue
  coverage
  stale
  report
  gate
  inspect
  schema
```

## 3. Convenience command

一般利用者向け:

```bash
reviewgraphen review \
  --base main \
  --head HEAD \
  --profile code-review \
  --format json \
  --output .reviewgraphen/reports/latest.json
```

これはstage commandを順に呼ぶtransactional orchestrationです。

内部stageが失敗した場合、完了済みeventを残し、どこまで進んだかをreportします。

## 4. Initialization

```bash
reviewgraphen init
```

生成:

```text
.reviewgraphen/
  config.toml
  policies/
  profiles/
  store/
  artifacts/
  reports/
```

既存directoryがある場合、上書きしません。`--force`でもevent storeを削除せず、config templateだけ明示的に更新します。

## 5. Snapshot

```bash
reviewgraphen snapshot from-git \
  --base main \
  --head HEAD \
  --output /tmp/snapshot.json
```

出力:

- repository identity。
- base/head。
- changed files。
- source hashes。
- dirty state。
- toolchain。
- exclusion policy。
- snapshot ID。

network accessは不要です。GitHub PR metadata等はprovider adapter commandで別途追加します。

## 6. Ingest

```bash
reviewgraphen ingest \
  --snapshot /tmp/snapshot.json \
  --adapter rust-syn \
  --adapter cargo-metadata \
  --adapter test-map \
  --output /tmp/program-space.json
```

またはrepositoryから直接:

```bash
reviewgraphen ingest from-git \
  --base main \
  --head HEAD \
  --profile code-review \
  --format json
```

出力にはfactsだけでなくcompletenessとobstructionsを含めます。

## 7. Obligations

```bash
reviewgraphen obligations synthesize \
  --program-space /tmp/program-space.json \
  --profile profiles/code-review/profile.toml \
  --output /tmp/obligations.json
```

補助command:

```bash
reviewgraphen obligations list --state generated
reviewgraphen obligations explain <obligation-id>
reviewgraphen obligations diff --old run:A --new run:B
```

同一入力なら同じobligation IDsを返します。

## 8. Plan

```bash
reviewgraphen plan \
  --obligations /tmp/obligations.json \
  --budget-tokens 200000 \
  --budget-cost 50 \
  --mode risk-first \
  --output /tmp/review-plan.json
```

planはcandidateであり、必要に応じてhumanが編集またはacceptします。

```bash
reviewgraphen plan accept /tmp/review-plan.json
```

MVPでは明示acceptを省略できても、plan hashをrunへ記録します。

## 9. Run

```bash
reviewgraphen run \
  --plan /tmp/review-plan.json \
  --reviewer llm:provider-model \
  --max-concurrency 4 \
  --output /tmp/executions.jsonl
```

特定obligation:

```bash
reviewgraphen run obligation <obligation-id> \
  --reviewer llm:provider-model
```

dry run:

```bash
reviewgraphen run --plan ... --dry-run
```

表示:

- 対象obligation。
- context size。
- estimated cost。
- allowed tools。
- required evidence。
- source loss。

## 10. Verify

```bash
reviewgraphen verify \
  --run <run-id> \
  --verifier test \
  --verifier static \
  --output /tmp/verification.json
```

特定claim:

```bash
reviewgraphen verify claim <claim-id> \
  --verifier duplicate-submit-test
```

verifier未対応の場合、exit errorではなくdomain result `unsupported`を返します。ただしconfiguration不正はtool errorです。

## 11. Glue

```bash
reviewgraphen glue \
  --run <run-id> \
  --contexts payment,ui-event,persistence \
  --output /tmp/gluing.json
```

実行内容:

- overlap抽出。
- Section restriction。
- contract/assumption/evidence compatibility。
- gluing result。
- obstruction作成。

## 12. Coverage

```bash
reviewgraphen coverage \
  --run <run-id> \
  --fresh-only \
  --format table
```

例:

```text
Universe: code-review@1 / rules 7a9... / snapshot 42d...
Extraction: symbol 96%, calls 71%, paths partial

Stage                 Raw          Weighted
visited               420/500      91.0%
completed             398/500      88.0%
evidence-supported    250/500      79.0%
verified              180/500      72.0%
fresh-verified        174/500      69.0%

Critical unresolved: 2
Gluing obstructions: 1
Gate: incomplete
```

## 13. Stale

```bash
reviewgraphen stale compute \
  --previous-run run:old \
  --base old-head \
  --head HEAD
```

```bash
reviewgraphen stale list --reason dependency_changed
reviewgraphen stale explain <record-id>
```

## 14. Report

```bash
reviewgraphen report \
  --run <run-id> \
  --view human-review \
  --format markdown \
  --output review.md
```

views:

```text
human-review
ai-view
audit-trace
ci-gate
research-export
context-envelope
```

## 15. Gate

```bash
reviewgraphen gate \
  --run <run-id> \
  --policy .reviewgraphen/policies/default.toml
```

output:

```json
{
  "status": "incomplete",
  "blocking_obstruction_ids": [],
  "incomplete_reason_ids": [
    "obstruction:critical-obligation-unverified"
  ]
}
```

`gate`だけがdomain statusをCI exit codeへ変換します。他commandはvalid reportを生成できたかをexit codeで表します。

この節は将来契約です。実装済みだった `gate <report.json>` は、信頼された
Store revision または署名へ束縛されない直列化 report から pass を返せたため、
ADR 0028 で撤去されました。現在の CLI に `gate` command はありません。

## 16. Inspect

```bash
reviewgraphen inspect obligation <id>
reviewgraphen inspect claim <id>
reviewgraphen inspect evidence <id>
reviewgraphen inspect path <id>
reviewgraphen inspect context <id>
reviewgraphen inspect source <id>
```

`--json`でstable machine outputを返します。

## 17. Schema

```bash
reviewgraphen schema list
reviewgraphen schema print reviewgraphen.report.v1
reviewgraphen schema validate report.json
```

schema validationはnetworkを必要としません。

## 18. Configuration priority

高い順:

1. CLI arguments。
2. environment variables。
3. `.reviewgraphen/config.local.toml`。
4. `.reviewgraphen/config.toml`。
5. built-in defaults。

secretはconfig fileへ平文保存せず、environmentまたはOS secret mechanismを使います。

## 19. Output policy

- machine commandのdefaultはJSON。
- progressはstderr。
- report payloadはstdout。
- `--quiet`はprogressを抑制。
- IDsとenumはlocale非依存。
- human formatは日本語/英語localization可能。
- JSON schemaは英語field名で固定。
- raw model outputはstdoutへ混ぜない。

## 20. Exit codes

| Code | Meaning |
| --- | --- |
| 0 | command succeeded; 将来のStore-bound `gate`の場合はpass |
| 2 | invalid CLI arguments |
| 3 | schema or configuration invalid |
| 4 | I/O or store error |
| 5 | extractor/reviewer/verifier tool failure |
| 10 | future Store-bound `gate`: blocked |
| 11 | future Store-bound `gate`: incomplete |
| 12 | future Store-bound `gate`: policy evaluation error |
| 20 | internal invariant violation |

Findingがあるだけでstage commandをnon-zeroにしません。domain resultとtool failureを分離します。

## 21. Security options

```bash
--network none|provider-only|allow-list
--workspace-root <path>
--allow-tool <id>
--deny-tool <id>
--redact-secrets
--no-source-upload
--max-source-bytes
```

default:

- repository write禁止。
- arbitrary shell禁止。
- networkなし。LLM provider利用時だけprovider endpoint。
- workspace外read禁止。
- secrets redaction有効。

## 22. Compatibility wrapper

HigherGraphenの既存commandは将来次のthin wrapperにできます。

```bash
highergraphen pr-review ...
  -> reviewgraphen review --profile code-review-baseline ...

highergraphen test-gap ...
  -> reviewgraphen obligations synthesize --rule-pack test-gap ...
```

移行期間はschema adapterを持ち、silent semantic changeを避けます。

## 23. CLI invariants

1. progressとJSONを同じstreamへ混ぜない。
2. valid domain obstructionをtool crash扱いしない。
3. gate以外でfinding件数をexit codeへ直結しない。
4. same input stageはdeterministic outputを返す。
5. LLM stageはmodel/prompt/configを記録する。
6. security defaultはno arbitrary shell / no repository write。
7. schema versionを省略しない。
8. partial runを破棄せず、incompleteとして再開可能にする。
