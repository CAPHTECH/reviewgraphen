# AGENTS.md — ReviewGraphen実装規約

この文書は、ReviewGraphenを実装・変更するAI coding agentと人間開発者の共通作業規約です。

## 最初に読む文書

1. `README.md`
2. `docs/00_vision_and_scope.md`
3. `docs/03_conceptual_model.md`
4. `docs/04_highergraphen_mapping.md`
5. `docs/05_system_architecture.md`
6. 対象変更に対応するADR

## 絶対に維持する境界

- Program fact、Review claim、Evidenceを同一レコードへ潰さない。
- AIが生成した構造をconfidenceだけでacceptedへ昇格しない。
- `reviewed`、`evidence_supported`、`verified`、`human_accepted`を同義にしない。
- カバレッジの分母を暗黙に作らない。snapshot、profile、rule-set version、extractor versionを記録する。
- Projectionはsource IDsとmeaningful information lossを宣言する。
- staleなevidenceを現在snapshotのsign-offに使用しない。
- 局所レビューの成功から、gluing checkなしに大域的安全性を結論しない。
- LLMをparser、symbol resolver、deterministic fact extractorの代替にしない。
- 任意shell実行をreviewerへ許可しない。tool実行はallow-listとworkspace-scoped cwdを使う。
- raw model proseをcanonical stateにしない。構造化出力を検証してイベントとして記録する。

## 実装原則

### 決定論的部分

同じ入力snapshot、profile、rule set、extractor versionに対して、次は同じ結果を返すこと。

- ProgramSpaceのaccepted fact IDs。
- ReviewObligation IDs。
- obligation universe。
- context projectionのsource set。
- staleness判定。
- coverage集計。
- schema validation。

### 確率的部分

LLM出力は再現性を保証しない。そのため必ず次を記録する。

- provider、model、model revisionが得られる場合はrevision。
- system/prompt template version。
- temperature等の推論設定。
- context projection IDとhash。
- tool call記録。
- raw response artifact hash。
- parsed claim IDs。
- abstentionとparse failure。

## 変更手順

1. 変更対象の概念と不変条件を特定する。
2. 対応するADRがなければ先にADRを書く。
3. schemaまたはpublic typeを変更する場合、versioningとmigrationを定義する。
4. accepted factとinferenceの境界テストを先に追加する。
5. happy pathだけでなく、stale、unknown、unverifiable、gluing failureをテストする。
6. projectionのsource traceとloss declarationを検証する。
7. 実装後にreference scenarioとcontract fixturesを更新する。

## Definition of Done

変更は次を満たしたときに完了する。

- formatter、lint、unit test、schema validationが通る。
- 同一入力のdeterminism testが通る。
- public reportのfixtureが更新されている。
- 新しいclaimがevidenceなしにacceptedにならない。
- 新しいprojectionがsource IDsとinformation lossを持つ。
- error stringだけでなく、必要な失敗がObstructionまたはtyped errorとして表現される。
- docsとADRが実装に一致する。
- 既存schemaの互換性を破る場合、明示的なmajor versionまたはmigrationがある。

## 禁止する短絡

- 「モデルが十分賢いので」obligation trackingを省略する。
- ファイルを読んだだけでnode/relation/path/invariant coverageを100%とする。
- review commentの件数をcoverageとして扱う。
- confidence 0.9以上をverifiedとして扱う。
- テストが存在するだけで対象propertyが検証されたとみなす。
- graph centralityの高さをseverityとみなす。
- LLMに全repoを渡すことをcontext constructionと呼ぶ。
- 複数agentを増やすだけで独立性や多角性が得られたとみなす。
- personaを増やしてreview lensの代替にする。
- generated code、vendored code、docsを無条件に除外する。除外はprofileで宣言し、coverage denominatorへの影響を残す。

## コードレビュー時の確認順

1. 境界違反。
2. ID安定性とversioning。
3. accepted/inferred/verifiedの状態遷移。
4. source trace。
5. staleness propagation。
6. coverage denominator。
7. security boundary。
8. performanceとincrementality。
9. projection表現。

## コミット単位

一つのコミットは、原則として一つの設計判断または一つのcontract変更に対応させる。schema、fixture、test、docsは同じ変更単位に含める。
