# 21. Migration from HigherGraphen

> Status: Draft v0.1  
> Baseline: HigherGraphen 0.7.1 / `CAPHTECH/higher-graphen@0f1e1cfe`  
> Scope: 現行 `pr-review` / `test-gap` 資産を失わずReviewGraphenへ一般化する

## 1. 移行の目的

HigherGraphenには既に、PR snapshotをSpace/Cell/Incidence/Contextへliftし、review target、obstruction、completion candidate、human/AI/audit projectionを出す`pr-review`系contractがあります。また、変更コードに対応するtestの不足を候補として出す`test-gap` workflowがあります。

ReviewGraphenはこれらを否定して作り直すのではありません。次の不足を補う上位のレビュー制御モデルへ移します。

- review target recommendationとreview obligationを区別する。
- coverage denominatorをversioned universeとして固定する。
- execution、claim、evidence、verification、decisionを永続化する。
- Node以外にRelation、Path、Invariantを対象化する。
- Context projectionとgluingをreview workflowへ統合する。
- snapshot変更によるstalenessを追跡する。

## 2. Repository boundary

### HigherGraphenに残すもの

- Space、Cell、Complex、Context、Morphism。
- Invariant、Obstruction、CompletionCandidate。
- Evidence、Projection、InterpretationPackage。
- generic reasoning / coverage / gluing / morphism primitives。
- reviewに限定されないcore engine。

### ReviewGraphenへ置くもの

- ReviewProfile / ReviewRule。
- ReviewObligation Universe。
- ReviewPlan / ReviewContextEnvelope。
- ReviewExecution / ReviewClaim / ReviewDecision。
- review-specific coverage stages。
- review-specific staleness policy。
- reviewer / verifier adapters。
- Code Review profile。
- `reviewgraphen` CLI。

HigherGraphen coreへreview-specific enumやprovider integrationを逆流させません。

## 3. Existing `pr-review` mapping

| HigherGraphen `pr-review` | ReviewGraphen | Migration semantics |
| --- | --- | --- |
| repository / PR identity | Snapshot / RepositoryRef | accepted input factとしてlift。 |
| changed file | ProgramSpace `file` cell | source hashとsnapshot scopeを追加。 |
| symbol | ProgramSpace `symbol` cell | extractor provenanceを保持。 |
| owner | Artifact/Policy cell | accepted observation。 |
| context | Context | typeとsourceを保存。 |
| test | ProgramSpace `test` cell | target relationが不明ならunknown。 |
| dependency edge | ProgramSpace relation | relation targetとしてobligation生成可能。 |
| supplied evidence | EvidenceSpace evidence | validity contextを追加。 |
| risk signal | Evidence/Valuation observation | findingやverified factへ昇格しない。 |
| review target | candidate ReviewObligation | legacy recommender由来、`proposed`。 |
| obstruction | ReviewObstruction candidate | blocker scopeを明示。 |
| completion candidate | CompletionCandidate | review stateを保持。 |
| human/AI/audit view | ProjectionViewSet | source traceとlossを維持。 |

### 3.1 Important semantic change

既存`review_target`は「見る価値がある対象の推薦」です。ReviewGraphenの`ReviewObligation`は、targetだけでなくproperty、required context、evidence requirement、applicability、risk、versionを持ちます。

したがって、migration adapterはreview targetを自動的に完全なaccepted obligationへ昇格しません。

```text
legacy review target
  -> proposed obligation candidate
  -> rule/profile mapping review
  -> admitted obligation or retained candidate
```

legacy targetを読んだことをcoverageへ算入しても、obligation universe全体をreviewしたことにはなりません。

## 4. Existing `test-gap` mapping

`test-gap`の結果は二つの意味へ分解します。

### 4.1 Review obligation

```text
target: changed behavior / symbol / relation
property: test.behavioral_coverage
required evidence: relevant test or accepted verification method
```

これは「テストによる証拠が必要」という確認責務です。

### 4.2 Completion candidate

```text
candidate: missing unit/integration/e2e test
rationale: no accepted mapping/evidence
```

これは不足を補う提案です。

「テストmappingが見つからない」だけで「テストが存在しない」というaccepted factやcode defectにはしません。extractor capability、naming convention、external test suiteの可能性をlimitationとして残します。

## 5. ID migration

既存IDを削除・再利用しません。

```json
{
  "legacy_ref": {
    "schema": "highergraphen.pr_review_target.report.v1",
    "id": "target:payment"
  },
  "reviewgraphen_id": "obligation-candidate:...",
  "mapping_kind": "lifted_from_legacy",
  "mapping_version": "highergraphen-pr-review@1-to-reviewgraphen@1"
}
```

stable semantic keyが作れる場合でも、legacy IDとnew IDの対応表をaudit traceへ残します。

## 6. Schema adapters

### Import

```bash
reviewgraphen import highergraphen-pr-review \
  --input pr-review-report.json \
  --output imported-events.jsonl
```

```bash
reviewgraphen import highergraphen-test-gap \
  --input test-gap-report.json \
  --output imported-events.jsonl
```

### Compatibility export

```bash
reviewgraphen report \
  --run <run-id> \
  --compat highergraphen-pr-review-v1
```

compatibility exportはlegacy schemaの意味を超えて情報を表せません。落としたobligation、verification、coverage、stalenessはinformation lossへ記録します。

## 7. CLI migration stages

## Stage A — Preserve

- HigherGraphenの既存commandとschemaを変更しない。
- ReviewGraphen fixtureでlegacy reportをimportできるようにする。
- baseline behaviorをgolden test化する。

## Stage B — Dual run

- 同じGit snapshotへHigherGraphen `pr-review`とReviewGraphen baseline profileを実行する。
- target correspondence、ranking差、missing obligationsを比較する。
- ReviewGraphenの結果を既存commandへ混ぜない。

## Stage C — Thin wrapper preview

```text
highergraphen pr-review
  -> stable legacy wrapper
  -> ReviewGraphen runtime or adapter
  -> legacy report projection
```

wrapper化はoutput compatibility testが通る場合だけ行います。

## Stage D — Deprecation notice

- deprecation versionを明記。
- replacement commandを表示。
- schema migration guideを提供。
- minimum support windowをrelease policyで定める。

## Stage E — Ownership transfer

- review-specific implementationをReviewGraphen repositoryへ集約。
- HigherGraphenにはgeneric primitiveとcompatibility pointerを残す。
- historical schema/docsを削除せずarchiveする。

## 8. Dual-run comparison

比較項目:

- changed file/symbol lift一致率。
- legacy targetからadmitted obligationへのmapping率。
- legacyで未表現のRelation/Path/Invariant obligation。
- ranking差。
- obstruction差。
- completion candidate差。
- source trace差。
- token/cost。
- false positive / missed issue。

差を「新実装が正しい」と前提にしません。legacy behaviorが有益でnew ruleが落とす場合、profileへ反映します。

## 9. Compatibility invariants

1. legacy accepted observationをAI inferenceへ格下げ・改変しない。
2. legacy unreviewed targetをaccepted obligationへ自動昇格しない。
3. legacy projectionから復元不能な情報を捏造しない。
4. compatibility exportのlossを宣言する。
5. old reportをnew gate passの根拠として無条件利用しない。
6. schema IDとmigration versionを残す。
7. deprecation前にfixtureとwrapper testを用意する。

## 10. HigherGraphen dependency versioning

ReviewGraphen run manifestへ次を記録します。

- HigherGraphen crate versions。
- interpretation package version。
- schema version。
- selected generic engine feature。
- compatibility status。

HigherGraphen minor updateで意味が変わる可能性がある場合、Cargo versionだけでなくnormalized interpretation hashを保存します。

## 11. What not to migrate

次を機械的に引き継ぎません。

- human summary proseをcanonical factとしてimportすること。
- legacy confidenceをverification outcomeへ変換すること。
- review target countをcoverage denominatorへ使うこと。
- missing test candidateをaccepted defectへ変換すること。
- old projectionをcurrent snapshotのfresh evidenceとして使うこと。

## 12. Migration completion criteria

- legacy fixturesをloss-awareにimport/exportできる。
- HigherGraphenとReviewGraphenの責任境界がrepository docsに反映される。
- wrapperがある場合、legacy schema golden testが通る。
- all new review semanticsはReviewGraphen側にある。
- HigherGraphen coreにreview provider依存がない。
- users/agentsがreplacement pathをskillから辿れる。
- historical reportsのaudit traceが維持される。
