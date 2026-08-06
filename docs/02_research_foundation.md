# 02. Research Foundation

> Status: Draft v0.1  
> Purpose: ReviewGraphenの新規性を過大評価せず、検証可能な研究課題へ落とす

## 1. 研究上の出発点

ReviewGraphenの発想には、既に成熟または進展している複数の研究系譜があります。

1. Program Dependence Graph、System Dependence Graph、Code Property Graph。
2. repository graphを使ったLLMのcode navigationとcontext retrieval。
3. CPG sliceとLLMを組み合わせた脆弱性検出。
4. LLM agentによるrepository-level auditingとdeterministic validator。
5. AI code review benchmark。
6. obligationとexecutable evidenceで制約したagentic review。

したがって、次は新規性として弱い主張です。

- ASTをLLMへ渡す。
- code graphから関連ファイルを検索する。
- repositoryをgraph databaseへ入れる。
- function単位でLLMレビューする。
- 複数agentを並列に動かす。

ReviewGraphenの研究焦点は、**graph-guided context retrievalではなく、graph-controlled review process**です。

## 2. 先行研究の整理

### 2.1 Program graph

Program Dependence Graphはcontrol dependenceとdata dependenceを統合し、テキスト順では捉えにくい意味的関係を扱います。Code Property GraphはAST、CFG、PDG等をproperty graphへ統合し、脆弱性パターンをquery可能にしました。

これらはReviewGraphenのProgramSpace抽出における基盤ですが、レビュー工程のcoverageやevidence stateそのものは中心対象ではありません。

### 2.2 Graph as context

**CodexGraph** はcode repositoryをgraph databaseへ変換し、LLM agentがgraph queryを生成してrepositoryを探索します。  
<https://arxiv.org/abs/2408.03910>

**RepoGraph** はrepository-level code graphを既存software engineering agentへ接続し、navigationとretrievalを改善します。  
<https://github.com/ozyyshr/RepoGraph>

これらの中心問題は「必要なコードへどう到達するか」です。ReviewGraphenは、その知見を利用しつつ、「何をレビューすべきか」「未処理は何か」という外部状態を追加します。

### 2.3 CPG-guided LLM analysis

**LLMxCPG** はCode Property Graphから脆弱性関連sliceを構成し、LLMへ与えるコード量を削減しながらfunction-levelおよびmulti-function vulnerability detectionを改善したと報告しています。  
USENIX Security 2025: <https://www.usenix.org/conference/usenixsecurity25/presentation/lekssays>

この結果は、full repository contextではなくpropertyに対応するbounded sliceを作る設計を支持します。ただし対象は主に脆弱性検出であり、一般review obligation universeやstaleness管理とは異なります。

### 2.4 Agentic repository audit

**RepoAudit** はinter-procedural data-flow factsとpath conditionを追跡し、validatorでLLM推論を確認します。ICML 2025論文では、15プロジェクトから40件のtrue bug、precision 78.43%を報告しています。  
<https://proceedings.mlr.press/v267/guo25n.html>

RepoAuditは、LLM findingをdeterministic validatorへ接続する設計の重要な先行例です。ReviewGraphenはこれを一般化し、findingだけでなくobligation、coverage、projection loss、local-to-global consistencyを扱います。

### 2.5 AI code review benchmark

**SWE-PRBench** は350件のPRとhuman-annotated ground truthを用いた2026年のpreprintです。8つのfrontier modelがhuman-flagged issueの15–31%を検出し、diff onlyからfull contextへ情報を増やすと全モデルが単調に悪化したと報告しています。  
<https://arxiv.org/abs/2603.26130>

この結果はpreprintであり、他dataset・modelでの再現が必要です。しかし、ReviewGraphenの次の設計仮説と整合します。

- full graphをそのままLLMへ与えない。
- graphを外部controllerとして使う。
- target propertyごとに小さいcontext projectionを作る。
- context量ではなくsource selectionとloss declarationを制御する。

**Code Review Bench** は50件の実PR、5言語、human-verified golden commentsを持つ公開benchmarkと、fresh PRを扱うonline benchmarkを提供します。  
<https://github.com/withmartian/code-review-benchmark>

ReviewGraphenの比較実験に利用可能ですが、LLM-as-judge、training contamination、golden commentの不完全性を評価上の限界として明示する必要があります。

### 2.6 Obligation-guided review

**Archer** はLLVM optimization reviewを対象とした2026年のpreprintで、obligationによってanalysisを誘導し、executable evidenceを持つfindingだけをvalidation guardが受理します。  
<https://arxiv.org/abs/2607.01808>

これはReviewGraphenに非常に近い思想を持ちます。差分候補は次です。

- LLVM optimizationに限定せず、profile化された一般review processを扱う。
- obligation universeとcoverageをfirst-classにする。
- Artifact / Review / Evidence Spaceを分ける。
- Context / Cover / Section / Gluingで局所レビューの大域接続を扱う。
- change morphismとstalenessを永続管理する。

この差は実装しただけでは研究的新規性になりません。比較実験で有効性を示す必要があります。

## 3. 研究上の空白

既存の多くのcode graph研究は、次を改善します。

```text
Program Graph
  -> relevant context retrieval
  -> LLM
  -> finding
```

ReviewGraphenが対象にする空白は次です。

```text
Program Graph
  -> versioned obligation universe
  -> explicit frontier and scheduler
  -> bounded context with declared loss
  -> heterogeneous reviewers
  -> claims
  -> evidence and verification
  -> local-to-global gluing
  -> multi-level coverage
  -> staleness under change
```

端的には、**code graphを知識表現からレビュー工程の制御面へ拡張する**ことです。

## 4. 研究仮説

### H1. Graph-controlled decomposition

同一モデル・同一予算なら、repository全体を自由探索させるより、semantic obligationへ分解して処理した方がseverity-weighted recallが高くなる。

### H2. Relation and path obligations

node-only obligationへrelationとpath obligationを加えると、cross-file/contextual issueのrecallが改善する。

### H3. Minimal projected context

full contextより、target、property、必要近傍、invariant、evidence requirementだけを含むprojectionの方がprecision-recall trade-offとrun-to-run stabilityを改善する。

### H4. Explicit frontier

自由探索agentより、未処理obligation frontierを持つschedulerの方が見落としの分散を減らす。

### H5. Evidence-bound admission

LLM findingをそのまま報告するより、static analysis、test、runtime witness、human decision等のverification stageを入れた方がprecisionを改善する。

### H6. Risk-weighted scheduling

同一token budgetでは、uniform traversalよりrisk-weighted schedulingの方がcritical/high issue recallを改善する。

### H7. Gluing detects cross-context inconsistency

各contextのlocal review結果を独立に集約するより、overlap restrictionとgluing checkを行う方がassumption mismatchとglobal inconsistencyを多く検出する。

### H8. Incremental preservation reduces cost safely

change morphismによる選択的stalenessは、毎回全レビューを再実行する方式より低コストで、critical issue recallを実質的に損なわない。

## 5. Research Questions

| ID | 問い |
| --- | --- |
| RQ1 | obligation-driven reviewはfree-form reviewより見落としを減らすか。 |
| RQ2 | Node / Relation / Path / Invariantのどの層がどのbug classへ寄与するか。 |
| RQ3 | context sizeではなくprojection strategyが性能へどう影響するか。 |
| RQ4 | deterministic verificationはfalse positiveをどの程度減らすか。 |
| RQ5 | risk schedulerはbudget当たりの重大欠陥発見を改善するか。 |
| RQ6 | explicit frontierはrun間分散を減らすか。 |
| RQ7 | gluingは単純なfinding aggregationでは見つからない矛盾を検出するか。 |
| RQ8 | staleness propagationは再利用率と安全性を両立するか。 |

## 6. 新規性を守るための設計境界

論文または研究成果としては、次を一つのsystem contributionとして結ぶ必要があります。

1. Versioned Review Obligation Universe。
2. Review Graphとしてのlifecycle・claim・evidence状態。
3. Minimal context projection with declared loss。
4. Multi-level coverage。
5. Evidence-bound verification。
6. Local-to-global gluing。
7. Change-aware staleness。

一つだけでは既存研究と重なりやすい一方、この統合が複雑すぎるとablation不能になります。実験では各機構を段階的に追加して寄与を分離します。

## 7. 反証条件

ReviewGraphenの仮説は、次の結果が出た場合に修正または棄却すべきです。

- free-form agentが同予算で一貫して高いrecallを示す。
- obligation生成漏れがLLM探索漏れを上回る。
- relation/path追加がcontext noiseを増やすだけで性能を改善しない。
- evidence verificationがtrue positiveを過剰に棄却する。
- risk weightingが既知bug taxonomyへ過学習する。
- gluing costに対して新規findingがほとんどない。
- staleness判定漏れが再利用利益を上回る。

ReviewGraphenは思想の正しさを証明するための研究ではありません。どの条件で有効で、どの条件で成立しないかを確定するための研究です。

## 8. 参考文献

- Ferrante, Ottenstein, Warren. “The Program Dependence Graph and Its Use in Optimization.” ACM TOPLAS, 1987.
- Yamaguchi et al. “Modeling and Discovering Vulnerabilities with Code Property Graphs.” IEEE S&P, 2014.
- Liu et al. “CodexGraph: Bridging Large Language Models and Code Repositories via Code Graph Databases.” 2024.
- Ouyang et al. “RepoGraph: Enhancing AI Software Engineering with Repository-Level Code Graph.” ICLR 2025.
- Guo et al. “RepoAudit: An Autonomous LLM-Agent for Repository-Level Code Auditing.” ICML 2025.
- Lekssays et al. “LLMxCPG: Context-Aware Vulnerability Detection Through Code Property Graph-Guided Large Language Models.” USENIX Security 2025.
- Kumar. “SWE-PRBench: Benchmarking AI Code Review Quality Against Pull Request Feedback.” arXiv preprint, 2026.
- Ni and Li. “Archer: Towards Agentic Review for Compiler Optimizations.” arXiv preprint, 2026.
- Martian. “Code Review Bench.” Public benchmark repository, 2026.
