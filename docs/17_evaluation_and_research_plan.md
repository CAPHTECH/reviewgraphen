# 17. Evaluation and Research Plan

> Status: Draft v0.1  
> Goal: ReviewGraphenの有効条件と失敗条件を実験で確定する

## 1. 評価原則

ReviewGraphenを評価するとき、モデル性能とreview harness性能を混同しません。

- 同じmodel。
- 同じprovider設定。
- 同じtoken/cost budget。
- 同じPR/ground truth。
- context strategyとworkflowだけを変える。

主張は「このモデルが優秀」ではなく、**同じモデルでもreview processの構造によって見落とし、誤報、分散がどう変わるか**です。

## 2. Research Questions

| ID | Research Question |
| --- | --- |
| RQ1 | obligation-driven reviewはfree-form agent reviewよりrecallを改善するか。 |
| RQ2 | Relation/Path/Invariant obligationはNode-onlyよりcross-file issueを検出するか。 |
| RQ3 | minimal context projectionはfull contextよりprecision-recallと安定性を改善するか。 |
| RQ4 | evidence verificationはfalse positiveを減らすか。 |
| RQ5 | risk schedulingはfixed budgetで重大issue recallを改善するか。 |
| RQ6 | explicit frontierはrun-to-run varianceを減らすか。 |
| RQ7 | gluingはlocal finding aggregationでは得られないconflictを検出するか。 |
| RQ8 | incremental stalenessはfull rerunより低コストで同等のcritical recallを保つか。 |

## 3. Experimental conditions

### B0: Diff-only prompt

PR diffとsummaryだけを一回レビュー。

### B1: Free-form repository agent

agentがrepositoryを自由探索し、review commentsを生成。

### B2: AST node traversal

changed/relevant AST nodesごとにreview。

### B3: Graph-guided retrieval

targetからk-hopまたはqueryでcontextを取得してreview。obligation stateは持たない。

### G1: Node obligations

versioned obligation universe、explicit frontier、minimal context。

### G2: Node + Relation

relation obligation追加。

### G3: Node + Relation + Path + Invariant

full ReviewGraphen target hierarchy。

### G4: G3 + Evidence verification

deterministic verifier。

### G5: G4 + Gluing

context overlapとglobal obstruction。

### G6: G5 + Risk scheduling

fixed budget optimization。

この段階比較で寄与をablationします。

## 4. Datasets

### 4.1 Code Review Bench

- 50 PR。
- Python、Go、TypeScript、Ruby、Java。
- human-verified golden comments。
- severity labels。
- offline reproducibility。
- online fresh PR benchmarkも存在。

利用上の注意:

- static benchmark contamination。
- golden commentがすべてのissueを含むとは限らない。
- LLM judge variance。
- tool setup differences。

### 4.2 SWE-PRBench

- 350 PR。
- human-annotated ground truth。
- context configuration比較。
- 2026年preprint。

利用上の注意:

- preprint結果の再現確認。
- dataset licenseとartifact availability。
- model versionsが変わる。
- context constructionをReviewGraphen用に再実装する場合、原条件との差を記録。

### 4.3 ReviewGraphen Mutation Corpus

controlled fault injectionにより、obligation層別の検出力を測ります。

mutation classes:

- node-local null/validation。
- call contract mismatch。
- missing error propagation。
- duplicate event / idempotency。
- transaction boundary。
- auth path。
- cache invalidation。
- state transition。
- API compatibility。
- missing test relation。
- architecture boundary。
- gluing assumption conflict。

synthetic mutationだけに最適化しないよう、real bugと併用します。

### 4.4 HigherGraphen dogfooding

HigherGraphenの実PR/commitを対象にします。

利点:

- current `pr-review` / `test-gap` baselineがある。
- Rust adapterを検証できる。
- project domain knowledgeが得られる。
- ReviewGraphen自身の開発へfeedbackできる。

欠点:

- author bias。
- project diversity不足。
- ground truthが不完全。

## 5. Ground truth

ground truth source:

- merged human review comment。
- post-review fix。
- accepted issue。
- failing/passing test。
- known bug patch。
- expert annotation。
- injected mutation。

一つのsourceだけを絶対視しません。

human reviewにない真のissueをtoolが見つける可能性があるため、unmatched candidateをexpert adjudicationへ回します。

## 6. Metrics

### 6.1 Detection

- precision。
- recall。
- F1。
- severity-weighted recall。
- critical/high recall。
- false positive per KLOC / PR。
- actionable finding rate。

### 6.2 Structural

- node issue recall。
- relation issue recall。
- path issue recall。
- invariant issue recall。
- cross-file recall。
- contextual/latent recall。

### 6.3 Process

- obligation coverage。
- evidence-backed coverage。
- fresh verified coverage。
- abstention rate。
- unverifiable rate。
- unresolved gluing obstruction。
- extraction completeness。

### 6.4 Efficiency

- tokens / true positive。
- cost / true positive。
- cost / critical finding。
- runtime。
- tool invocations。
- context bytes。
- verifier cost。

### 6.5 Stability

- run-to-run recall variance。
- finding set Jaccard。
- severity agreement。
- source citation consistency。
- abstention variance。

### 6.6 Calibration

- confidence bin accuracy。
- risk score vs actual issue。
- evidence-supported claim precision。
- gate false pass / false block / incomplete rate。

## 7. Statistical design

- 各conditionを複数seed/runで評価。
- paired comparisonを使用。
- bootstrap confidence interval。
- PR単位のclusterを考慮。
- project/language stratification。
- effect sizeを報告。
- multiple comparison補正。
- benchmark内でhyperparameterを調整しすぎない。
- profileを事前固定する。

## 8. Token and cost normalization

modelごとにcontext windowとpricingが異なるため、少なくとも二種類で比較します。

1. same model, same token budget。
2. same model, same monetary budget。

wall-clockはprovider latencyの影響が大きいため補助metricとします。

## 9. Context ablation

変数:

- diff anchoring。
- caller/callee depth。
- path inclusion。
- test inclusion。
- invariant inclusion。
- prior evidence inclusion。
- natural language summary。
- full source vs structural facts。
- information loss disclosure。

context sizeだけでなく構成要素ごとの寄与を測ります。

## 10. Obligation ablation

- changed node only。
- all public node。
- relation。
- path。
- invariant。
- morphology/change。
- test gap。
- AI-proposed supplemental obligations。

obligation数の増加によるrecall向上とcostを分けます。

## 11. Verification ablation

- no verification。
- second LLM。
- static analyzer。
- executable test。
- path validator。
- human adjudication。
- mixed policy。

second LLMをdeterministic verificationと同列にしません。

## 12. Gluing evaluation

専用fixtureを用意します。

- individual contextsでは問題なし。
- overlap assumptionsが矛盾。
- global invariantだけが失敗。
- evidence environmentが不一致。
- accepted exception scopeが衝突。

評価:

- conflict detection recall。
- false gluing obstruction。
- added cost。
- human resolution time。
- global false-pass reduction。

## 13. Incremental evaluation

sequence of commitsを用意します。

条件:

- full rerun。
- changed files only。
- dependency cone。
- ReviewGraphen property-sensitive staleness。

metrics:

- reused verified weight。
- missed invalidation。
- unnecessary rerun。
- critical recall。
- cost reduction。
- stale false negative。

## 14. Human study

後段で実施可能な問い:

- ReviewGraphen reportでsign-off判断が速くなるか。
- unknown/staleの理解が改善するか。
- finding noiseへの信頼低下を防げるか。
- human reviewerが重要obligationへ集中できるか。
- projection lossを認識できるか。

human studyなしに「developer productivityが向上」と主張しません。

## 15. Success thresholds

固定絶対値よりbaseline relative improvementを重視します。

MVP research success:

- free-form baselineよりseverity-weighted recallが改善。
- precisionを大きく損なわない。
- relation/path issue recallに明確な寄与。
- evidence verificationでfalse positive低下。
- fixed budgetでrisk schedulingがcritical recallを改善。
- run-to-run variance低下。
- uncovered/unknownの定量表示が可能。

threshold値はpilot後に事前登録します。

## 16. Failure reporting

公開すべきnegative result:

- obligation生成漏れ。
- context projectionで必要情報を落とした例。
- verifierがtrue issueを棄却した例。
- risk schedulerが低scoreのcritical bugを後回しにした例。
- gluing false positive。
- staleness漏れ。
- provider/model依存。
- language adapter差。

ReviewGraphenの信頼性は成功例の数ではなく、失敗境界を再現可能にすることで高まります。

## 17. Reproducibility package

- fixed profile/rule hashes。
- dataset references。
- run manifests。
- prompt/context projection hashes。
- model descriptors。
- raw and judged outputs（license/policy範囲）。
- evaluation scripts。
- metric definitions。
- environment。
- cost log。
- adjudication records。
- known limitations。

## 18. Research milestones

1. benchmark adapter。
2. baseline reproduction。
3. Node-only prototype。
4. Relation/Path ablation。
5. context projection study。
6. evidence verification study。
7. gluing fixture study。
8. incremental study。
9. integrated evaluation。
10. paper/report and public replication bundle。
