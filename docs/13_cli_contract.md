# 13. CLI Contract

> Status: Draft v0.1  
> Binary: `reviewgraphen`  
> Design: CLI-first, JSON-contract-first, local-first

Implementation note: ADR 0029 withdraws ADR 0024's fixed offline
`review --fixture double-submit` command because it was not a generic review
path. The implemented surface contains `schema list|print|validate` and the
ADR 0030 generic command below; all `review --fixture ...` forms are rejected
before input access. ADR 0030
implements the generic `review --request <request.json> --artifacts
<fresh-absolute-dir>` path over ordinary Git ingestion, deterministic
obligations/planning/context, isolated Codex or Claude CLI execution, and exact
process-record replay. Its output is explicitly non-authority and incomplete:
it creates proposed/unreviewed claims but performs no verifier or human
acceptance operation. ADR 0028 likewise withdraws the detached-report-only
`gate <report.json>` command. The Store-bound `gate` described later in this
draft remains a future contract, not currently accepted command syntax.

**実装状態追記（2026-08-24）**: 上のv1説明は歴史的な契約説明として残す。
現行ツリーには、ADR 0038用に独立した
`reviewgraphen.generic_review_request.v2` / `reviewgraphen.generic_review_run.v2`
に加え、`reviewgraphen.generic_review_request.v3` /
`reviewgraphen.generic_review_run.v3` のexact dispatchがある。v2は
`context.subject_windows@2`、v3は`context.subject_windows@3`を使い、双方とも
`relation.changed_public_callee@1`、`deterministic.abstain@1` を使う
provider-free経路がある（`crates/reviewgraphen-cli/src/lib.rs:58-64`,
`crates/reviewgraphen-runtime/src/generic.rs:1353-1365,1778-1802`）。v2は
`codex_cli`、`claude_cli`、`codex_app_server`を選べるschema形状を持つが、
現在の実行器はそれらをtyped `unsupported`として拒否し、実際に起動するのは
deterministic abstentionまたは検証済みreplayだけである
（`generic.rs:2054-2062`）。

v3の製品経路とartifact layoutは実装済みだが、exact command sequence、exit code、
expected hashを伴うclone quickstartは`expected-hashes.json`と独立した2 cloneでの
検証が完了してから確定する。それまでは、
ここにあるv1のコマンド例や将来形のcommand familyを、v2 quickstartの確定した
操作手順として読んではならない。現時点の再現済み範囲と未確認範囲は
[`23_current_capability_status.md`](23_current_capability_status.md)を正典とする。

ADR 0038 §11.1 は固定秒数、stage別、入力比例のいずれの製品deadlineも定義せず、
exit 21を割り当てない。10秒watchdogはprebuilt fail-fast fixtureを監視する
テストハーネスだけの失敗条件であり、製品CLI契約ではない。任意の
`--diagnostics <fresh-file>` はartifact rootの外部にだけ、非canonicalな
`reviewgraphen.generic_review_diagnostics.v1`をcreate-newで書く。既定では書かず、
artifact rootと同一・配下・祖先、既存file、symlinkはexit 2で拒否する。
診断の有無はcanonical audit / manifest bytesを変更しない。
v3 human reportはlive実行が返すbasis-bound
`ValidatedGenericReviewRunV3`から直接生成する。bytes-only decodeが返す
`UnvalidatedGenericReviewRunV3`からhuman report v2を生成する経路は持たない。

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

現在実装されているgeneric orchestration:

```bash
reviewgraphen review \
  --request /absolute/path/review-request.json \
  --artifacts /absolute/fresh/path/review-artifacts
```

requestは`reviewgraphen.generic_review_request.v1`、stdoutはcanonicalな
`reviewgraphen.generic_review_run.v1`です。artifacts directoryにはmodelへ
渡したexact packet、process output、replay用process recordを残します。
Codex CLI、Claude CLI、またはrecord replayをrequestで選択します。Codex
App Serverは差し替え境界だけが存在し、未実装としてfail-closedします。

これはingest、obligation synthesis、plan、context construction、reviewer
実行、proposal parsingを順に呼ぶorchestrationです。Evidence、Verification、
Decision、Finding、V5 report、gateは生成しません。

現在のrequest v1は`MvpRulePack`固定です。profile fieldはingestと分母の
identityを束縛しますが、任意のcorrectness rule engineを選択しません。
accepted domain invariantがない入力では、実測上すべてのobligationが
`reviewgraphen.capability_gap`になり得ます。この場合はanalyzer capabilityを
reviewしているのであり、generic bug detectionを実行したとは扱いません。
M7は別途freezeしたontology/obligation profileをprocess adapterのconsumer
として使い、同じraw-record/replay境界の下で検出性能を測ります。

以下のoption-oriented convenience syntaxは将来案であり、現在のaccepted
command syntaxではありません。

```bash
reviewgraphen review --base main --head HEAD --profile code-review
```

内部stageが失敗した場合はnonzeroで停止します。作成済みpacket/process
artifactは診断用に残りますが、durable Store eventやauthorityとしては扱いません。

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
