# 18. MVP Roadmap

> Status: Draft v0.1  
> Goal: ReviewGraphenの中心仮説を、狭いが一貫したend-to-end systemで検証する

## 1. MVPの定義

ReviewGraphenのMVPは「高性能なAIコードレビュー製品」ではありません。次の制御ループが実際に成立することを示す研究・実装用vertical sliceです。

```text
bounded snapshot
  -> accepted ProgramSpace facts
  -> versioned obligation universe
  -> risk-aware review plan
  -> minimal context projection
  -> reviewer claim / abstention
  -> evidence verification
  -> coverage / gluing / obstruction
  -> auditable report
```

MVPが証明すべきなのは、LLMを賢く見せることではなく、**同じ対象に対するレビューの分母、処理状態、証拠、未検証領域を外部状態として管理できること**です。

## 2. 初期スコープ

### 2.1 対象

- Local Git repository。
- Rust project。
- Base revisionとhead revisionの差分。
- Code Review profileのみ。
- `Node / Relation / Path / Invariant` obligation。
- CLIとJSON report。
- Local-first store。
- 一つのLLM reviewer adapterとdeterministic fake reviewer。
- allow-listされたtest/static verifier。
- `double-submit-payment`参照シナリオ。

### 2.2 MVPで扱うProgramSpace fact

- repository、snapshot、file、module、function、type、test。
- containment。
- direct call。
- import/module dependency。
- changed-by relation。
- test-to-target relation。
- selected state/write relation。
- source location、hash、provenance。

Call graphは完全である必要はありません。ただし、何を解決できなかったかを`ExtractionCompleteness`とobstructionとして出力します。

### 2.3 MVP rule pack

最低限、次を実装します。

| Rule ID | Target | Property |
| --- | --- | --- |
| `node.changed_public_symbol@1` | Node | local correctness / contract change |
| `relation.changed_call_contract@1` | Relation | error・async・null・ownership contract |
| `relation.concurrent_reentry@1` | Relation/Subgraph | duplicate event / re-entry |
| `path.external_side_effect@1` | Path | validation、authorization、idempotency |
| `invariant.payment_at_most_once@1` | Invariant/Path | at-most-once external charge |
| `test.changed_behavior_evidence@1` | Node/Relation | required behavioral evidence |
| `projection.loss_declared@1` | Projection | source traceとloss declaration |

Rule packは一般的な全バグtaxonomyを装いません。参照シナリオと評価対象に必要な狭い集合から開始します。

## 3. 明示的な非スコープ

- Web UI、IDE extension、dashboard。
- Hosted multi-tenant service。
- GitHub Appによる自動comment投稿。
- 自動修正、自動merge、自動approval。
- 多言語対応。
- 完全なinterprocedural data flow。
- 任意repository commandの自律実行。
- persona agentの大量並列化。
- Architecture / Specification / Test Review profile。
- 組織横断analytics。
- compliance certification。

これらを先に追加すると、ReviewGraphen固有の仮説ではなくintegration規模だけが増えます。

## 4. 段階

## M0. Contract baseline

### Deliverables

- Conceptual model。
- ADR一式。
- Input / obligation / report schema。
- Reference fixture。
- deterministic ID specification。
- state transition testsの設計。

### Exit criteria

- すべてのJSON fixtureがschema validationを通る。
- accepted fact、claim、evidence、decisionの境界がschemaと文書で一致する。
- `reviewed != verified != accepted`を破る例がnegative fixtureで拒否される。
- obligation universeのversion tupleが定義されている。

## M1. Deterministic review core

### Deliverables

- `reviewgraphen-core`。
- JSONからProgramSpaceを取り込むmanual adapter。
- obligation synthesis。
- stable IDs。
- lifecycle event log。
- raw/weighted coverage。
- human / AI / audit projection。

### Exit criteria

- 同一input、profile、rule versionからbyte-equivalent canonical obligation bundleを生成する。
- obligationを一件も実行していない状態をcoverage 0%として正しく報告する。
- excluded targetを分母から暗黙に消さず、exclusion recordを残す。
- partial runを再開できる。

## M2. Rust and Git ingestion

### Deliverables

- Git snapshot adapter。
- Rust `syn` based AST adapter。
- Cargo metadata adapter。
- direct call / containment / test mappingの限定抽出。
- extraction completeness report。
- changed structure mapping。

### Exit criteria

- HigherGraphen repositoryの指定commitをnetworkなしでingestできる。
- parse failure、macro expansion未解決、dynamic dispatch未解決をunknownとして出力する。
- parser未対応領域を「問題なし」へ変換しない。
- source locationとcontent hashからfactを追跡できる。

## M3. Context projection and reviewer protocol

### Deliverables

- ReviewContextEnvelope builder。
- source selection policy。
- projection loss declaration。
- fake reviewer。
- provider-neutral LLM reviewer adapter。
- structured claim parser。
- abstention / malformed output handling。

### Exit criteria

- Envelopeがobligation、source IDs、included/excluded、unknowns、lossを持つ。
- raw model proseをcanonical stateへ直接保存しない。
- malformed responseはclaimを捏造せずtyped failureになる。
- 同じEnvelopeを再現・監査できる。
- repository内のprompt injectionをinstructionとして実行しない。

## M4. Evidence-bound verification

### Deliverables

- EvidenceSpace store。
- claim-evidence binding。
- allow-list test verifier。
- static fact verifier。
- verification policy。
- finding projection。

### Exit criteria

- `issue_present` claimをtest witnessで支持できる。
- verification不能なclaimを`unsupported`または`inconclusive`として保つ。
- second LLM opinionをmachine verificationへ偽装しない。
- accepted findingがclaim、evidence、verification、decisionへ遡れる。

## M5. Context cover and gluing

### Deliverables

- Context、Cover、Section、RestrictionのReviewGraphen interpretation。
- overlap extraction。
- local claim compatibility check。
- gluing obstruction。
- double-submit scenarioのUI/payment overlap。

### Exit criteria

- 各context単独では通るがoverlapで矛盾するfixtureを検出する。
- gluing failureを単なるduplicate findingへ潰さない。
- global claimを、必要なlocal sectionsが欠けた状態で成立させない。
- overlap source IDsとrequired resolutionをreportする。

## M6. Incremental review and staleness

### Deliverables

- snapshot change morphism。
- obligation correspondence。
- direct / indirect staleness。
- property-sensitive invalidation。
- fresh verified coverage。
- partial rerun plan。

### Exit criteria

- targetは不変でもcallee変更によりrelation/path obligationがstaleになる。
- stale verificationがcurrent gateへ利用されない。
- rename/moveを可能な範囲でsemantic mappingする。
- mapping不能を保守的にstaleとして説明する。

## M7. Evaluation harness

### Deliverables

- B0/B1/B2/B3/G1–G4 condition runner。
- benchmark adapters。
- mutation corpus。
- token/cost/runtime log。
- paired metric calculator。
- reproducibility bundle。

### Exit criteria

- 同じmodel/budgetでbaselineとReviewGraphen条件を比較できる。
- finding matchingを自動judgeだけに依存せずexpert adjudicationへ接続できる。
- obligation generation missを独立metricとして測れる。
- negative resultを含むrun artifactを再現できる。

## 5. MVP completion gate

MVP完了には、機能の存在ではなく次のend-to-end contractを要求します。

1. Reference repository snapshotをingestできる。
2. obligation universeを決定論的に生成できる。
3. Relation/Path/Invariant obligationが二重課金リスクを対象化する。
4. reviewerがclaimまたはabstentionを構造化出力する。
5. duplicate-submit testが反例evidenceを生成する。
6. verificationとhuman decisionからaccepted findingを構成できる。
7. UI/payment assumptionsのgluing conflictを表現できる。
8. raw、evidence-supported、verified、fresh coverageを分離できる。
9. snapshot変更で関連recordをstaleへ伝播できる。
10. reportをJSON Schemaで検証できる。
11. audit traceがsource、tool、model、projection lossへ遡れる。
12. 同一決定論stageが再実行で同じcanonical outputを返す。

## 6. Research gate

MVP implementation completeとresearch hypothesis supportedを分けます。

### Implementation complete

上記のend-to-end contractが成立する。

### Research promising

最低限、pilotで次を観察する。

- free-form baselineよりRelation/Path issueのrecallが改善する。
- false positiveが検証stageで減る。
- uncovered obligationsが再現可能に可視化される。
- run間のfinding set分散が低下する。

### Research unsupported

次の場合、製品範囲または仮説を再設計します。

- obligation生成漏れが自由探索漏れより大きい。
- context projectionが必要情報を頻繁に失う。
- verifierがtrue positiveを過剰に棄却する。
- gluingが実質的な新規情報をほとんど生まない。
- tracking overheadが予算内のレビュー量を著しく減らす。

## 7. Risk order

実装順は「作りやすさ」ではなく仮説への危険度で決めます。

| Risk | 早期に確認する理由 |
| --- | --- |
| obligation universeが妥当でない | coverageの分母そのものが無意味になる。 |
| minimal projectionが情報を落とす | LLMレビュー性能が構造的に低下する。 |
| relation/path抽出が不十分 | node reviewとの差が成立しない。 |
| evidence bindingが弱い | findingのprecisionと監査性が成立しない。 |
| stalenessが過剰/不足 | 再利用利益または安全性が失われる。 |
| gluingの実益がない | HigherGraphenを使う強い理由が一つ減る。 |

UI、hosted orchestration、provider数はこれらの後です。

## 8. Release interpretation

```text
v0.0.x  contract and research prototype
v0.1.x  deterministic vertical slice
v0.2.x  Rust/Git profile and reviewer execution
v0.3.x  evidence, gluing, incremental review
v0.4.x  public evaluation preview
v1.0    schema/CLI compatibility commitment after evidence
```

version番号は実装進捗の約束ではなく、互換性と成熟度を表します。研究結果が否定的なら、v1.0へ進めることを目的化しません。
