# 01. Product Positioning

> Status: Draft v0.1  
> Decision: ReviewGraphenはHigherGraphen上のIntermediate Tool

## 1. 名称の三層

| 名称 | 意味 |
| --- | --- |
| **ReviewGraphen** | プロダクト、CLI、再利用可能なIntermediate Tool。 |
| **Graph-Driven Review** | 方法論。 |
| **Review Graph** | ReviewSpaceに構築されるレビュー状態の構造。 |

例:

> ReviewGraphen constructs a Review Graph and executes a Graph-Driven Review workflow.

## 2. HigherGraphenの層における位置

HigherGraphenは、core packages、intermediate tools、domain productsを分けています。`*graphen` は再利用可能な抽象ツール名であり、事業領域固有製品に直接使うべきではありません。

ReviewGraphenの中心対象は「レビュー義務、レビュー実行、主張、証拠、判断、カバレッジ」という領域横断の抽象物です。したがって、Code Reviewという一つのDomain ProductではなくIntermediate Toolに置きます。

```text
Level 0: HigherGraphen Core
  Space / Cell / Context / Morphism / Invariant / Obstruction
  Evidence / Projection / Reasoning / Runtime

Level 1: Intermediate Tools
  CaseGraphen
  ContextGraphen
  EvidenceGraphen
  ...
  ReviewGraphen  ← review processを中心に再構成

Level 2: Review Profiles / Domain Products
  Code Review
  Architecture Review
  Specification Review
  Test Review
  Contract Review
```

## 3. ReviewGraphenのcentral object

ReviewGraphenのcentral objectは単一のコメントやFindingではありません。

```text
ReviewObligation
```

です。

ReviewObligationは次を結びます。

- どのtargetを。
- どのpropertyについて。
- どのcontextとevidence requirementで。
- どのriskとpriorityで。
- 誰または何が確認し。
- どの状態まで到達したか。

これにより、レビューを「発見コメントの集合」ではなく「確認責務の構造」として扱えます。

## 4. 初期profileをCode Reviewにする理由

ReviewGraphen自体は汎用ですが、最初の強い業務シナリオはCode Reviewです。

- AI生成コードの増加に対し、生成速度より検証能力が不足している。
- AST、symbol、call、test、diffなど決定論的な構造抽出手段がある。
- human review comment、既知bug、test failureなど評価材料が比較的得やすい。
- HigherGraphenに既存の `pr-review` と `test-gap` 契約がある。
- HigherGraphen自身をdogfooding対象にできる。
- Node / Relation / Path / Invariantという階層差を実証しやすい。

## 5. 既存カテゴリとの差

### 5.1 LLM Code Reviewer

典型的にはdiffまたはrepoを読み、コメントを生成します。

ReviewGraphenとの差:

- コメント生成の前にobligation universeを作る。
- 未処理、abstention、staleを保持する。
- claimとevidenceを分離する。
- coverageとgluingを工程として持つ。
- LLM providerを交換可能な一adapterにする。

### 5.2 Static Analysis / SAST

決定論的ruleで欠陥候補を検出します。

ReviewGraphenとの差:

- static analyzerの結果をEvidenceまたはReviewObligation sourceとして統合する。
- ruleで直接判定しにくいsemantic propertyをLLMやhumanへ配分する。
- 複数解析結果と局所レビューを一つのReview Graphへまとめる。
- analyzerの「未対応領域」をcoverageから隠さない。

### 5.3 Code Graph / Graph-RAG

repository graphから関連コードを検索し、LLMへ渡します。

ReviewGraphenとの差:

- graphはcontext selectorだけでなくreview controllerである。
- query結果ではなく、義務、状態、証拠、失効を永続化する。
- 「検索されなかった対象」を未レビューとして残す。
- local resultのoverlapとgluingを扱う。

### 5.4 Policy Gate / CI

testやlintの結果からpass/failを決めます。

ReviewGraphenとの差:

- `pass`だけでなく`blocked`、`incomplete`、`unknown`を区別する。
- gateの根拠となるobligation、evidence、stalenessを説明できる。
- policyをReview Graph上の構造として扱う。

### 5.5 Human Review Management

reviewer assignmentやapprovalを管理します。

ReviewGraphenとの差:

- 人間の作業管理だけでなく、AI、静的解析、テスト、人間の異種reviewerを同じ義務へ結びつける。
- reviewer assignmentを最終目的にせず、構造的な確認状態を保持する。

## 6. Actors

これはマーケティング上のpersonaではなく、責任とcapabilityを持つactorです。

| Actor | Capability | 責任境界 |
| --- | --- | --- |
| Repository adapter | source factを抽出する | semantic判断をacceptedにしない |
| Static analyzer | checkable propertyを検査する | 未対応propertyを安全とみなさない |
| LLM reviewer | bounded contextからclaimを提案する | claimを自己承認しない |
| Verifier | claimを再現・反証する | 対応可能な証拠種別を明示する |
| Human reviewer | 判断、例外、受理を行う | 未確認領域を理解してsign-offする |
| Scheduler | 次のobligationを選ぶ | risk scoreを真理値とみなさない |
| Policy gate | 組織ルールを適用する | incompleteとpassを混同しない |

## 7. 製品境界

### ReviewGraphenが所有する

- ReviewGraphen固有modelとschema。
- obligation synthesis。
- context projection。
- review orchestration。
- claim / evidence binding。
- coverage、scheduling、staleness。
- report、gate、audit。
- profile contract。
- reviewer / verifier adapter contract。

### 外部へ委譲する

- language parser、type checker、LSP。
- precise call graph / data flow engine。
- Git provider API。
- LLM inference runtime。
- test runner、symbolic executor、model checker。
- organization identity / approval authority。

## 8. リポジトリ境界

推奨は独立repositoryです。

```text
CAPHTECH/reviewgraphen
  depends on published higher-graphen crates
```

理由:

- parser、analyzer、LLM adapter、evaluation harnessを含み、HigherGraphen coreより依存が重い。
- Code Review profileを高速に反復できる。
- private policy packやenterprise connectorとの商用境界を保ちやすい。
- HigherGraphen側へはdomain-neutralな不足だけをupstreamできる。
- `highergraphen pr-review` を互換wrapperとして残せる。

詳細は [`adr/0006-standalone-repository-over-highergraphen.md`](adr/0006-standalone-repository-over-highergraphen.md) と [`21_migration_from_highergraphen.md`](21_migration_from_highergraphen.md) を参照してください。

## 9. プロダクトの主張

ReviewGraphenが市場や研究に対して主張すべきなのは、単に「グラフを使うので賢い」ではありません。

> ReviewGraphen makes AI review coverage-bearing, evidence-bound, and incrementally maintainable.

日本語では次のように表せます。

> ReviewGraphenは、AIレビューを、対象範囲・証拠・未確認領域・失効を持つ保守可能な工程へ変える。

## 10. 主張してはいけないこと

- 「すべてのバグを検出する」。
- 「人間レビューを不要にする」。
- 「coverage 100%なら安全である」。
- 「Higher-order graphを使うから精度が上がる」。
- 「multi-agentだから多角的である」。
- 「検証済み」という語を、単なる二度目のLLM判定に使う。

これらは実験または明確な証拠なしに成立しません。
