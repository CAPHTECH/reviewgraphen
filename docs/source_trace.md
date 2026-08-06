# ReviewGraphen Source Trace

> Status: Draft v0.1  
> Snapshot date: 2026-08-07  
> Purpose: 設計判断がどの議論・現行実装・研究結果から導かれたかを追跡する

## 1. Trace policy

- Sourceは設計判断の根拠であり、sourceに書かれた主張を無条件にaccepted factとしない。
- preprintは査読済み研究と区別する。
- GitHub repositoryはcommit snapshotを固定する。
- 数値結果は元論文/公式artifactへ遡る。
- ReviewGraphen固有の統合仮説は、本source traceが正しさを保証しない。評価計画で検証する。

## 2. HigherGraphen baseline

Baseline repository:

```text
CAPHTECH/higher-graphen
commit: 0f1e1cfe40aaf8af4b40a92ab01743a3c357445f
workspace version: 0.7.1
observed: 2026-08-07
```

| Source path | Used for |
| --- | --- |
| `README.md` | AI-native higher structure、Space/Cell/Context/Morphism/Invariant/Obstruction/Completion/Projection/InterpretationPackage、`pr-review` / `test-gap` current surfaces。 |
| `docs/concepts/core-concepts.md` | Core vocabulary、Gluing Attempt、Witness、Graph Analytic、Temporal Property、Projection Loss等。 |
| `docs/specs/package-boundaries.md` | Core crateとbare `*graphen` intermediate toolの命名・依存境界。 |
| `docs/specs/intermediate-tools-map.md` | Intermediate Toolの定義、Domain Productとの区別。 |
| `docs/specs/pr-review-target-lift-model.md` | PR snapshotのSpace/Cell/Incidence/Context lift、accepted factとAI inferenceの境界。 |
| `docs/specs/pr-review-target-report-contract.md` | report-first、review target、obstruction、completion candidate、ProjectionViewSet。 |
| `docs/product-packages/architecture-product.md` | Domain interpretation packageのreference形。 |
| `docs/specs/engine-traits.md` | Space/Morphism/Consistency/Completion/Projection engine responsibilities。 |
| `docs/specs/ai-agent-integration.md` | CLI/schema/skill先行、MCP/provider integration境界、安全規則。 |
| `COMMERCIAL_BOUNDARY.md` | HigherGraphen public coreとcommercial assetsの現在境界。 |

## 3. Research sources

### 3.1 Program structure

| Source | Contribution to ReviewGraphen |
| --- | --- |
| Ferrante, Ottenstein, Warren, “The Program Dependence Graph and Its Use in Optimization,” 1987 | control/data dependenceをsyntax treeから分離して扱う基礎。 |
| Yamaguchi et al., “Modeling and Discovering Vulnerabilities with Code Property Graphs,” IEEE S&P 2014 | AST/CFG/PDG統合とqueryable code property graph。 |

ReviewGraphenへの反映:

- ASTを入口とし、call/data/control/state/test relationsへ拡張する。
- HigherGraphenをparser/compilerの代替にしない。
- extraction capabilityとcompletenessを明示する。

### 3.2 Graph-guided repository navigation

| Source | Status | Contribution |
| --- | --- | --- |
| CodexGraph, arXiv:2408.03910 | preprint | code graph DBをLLM agentのrepository探索へ利用。 |
| RepoGraph, ICLR 2025 | peer-reviewed | repository-level code graphをagent navigationへ接続。 |

ReviewGraphenへの反映:

- graph retrievalを利用可能なcontext constructorとして認める。
- ただし新規性をretrievalだけへ置かない。
- graphをobligation frontierとcoverageのcontrol stateへ拡張する。

### 3.3 Graph-sliced analysis and validation

| Source | Status | Reported result / lesson |
| --- | --- | --- |
| LLMxCPG, USENIX Security 2025 | peer-reviewed | CPG-guided sliceで入力code量を削減し、vulnerability detectionを改善したと報告。 |
| RepoAudit, ICML 2025 | peer-reviewed | repository-level data-flow/path trackingとvalidatorを組み合わせ、15 projectsで40 true bugs、precision 78.43%と報告。 |

ReviewGraphenへの反映:

- property別のbounded sliceを作る。
- claimをdeterministic evidence/validatorへ接続する。
- full graphをLLMへ直接投入しない。

### 3.4 AI code review benchmark

| Source | Status | Contribution |
| --- | --- | --- |
| SWE-PRBench, arXiv:2603.26130 | 2026 preprint | 350 PR、8 frontier models、context増加に伴う性能低下を報告。 |
| Code Review Bench, Martian | public benchmark | 50 real PR、5 languages、human-verified golden commentsを提供。 |

ReviewGraphenへの反映:

- minimal context hypothesis。
- same model/budgetでharnessを比較。
- benchmark contamination、ground truth incompleteness、LLM judgeを限界として扱う。

### 3.5 Obligation-guided agentic review

| Source | Status | Contribution |
| --- | --- | --- |
| Archer, arXiv:2607.01808 | 2026 preprint | LLVM optimization reviewをobligationとexecutable evidenceで誘導。 |

ReviewGraphenへの反映:

- obligation-guided reviewの直接的先行例として扱う。
- ReviewGraphenの差をgeneral profile、universe/coverage、three spaces、gluing、stalenessの統合へ置く。
- 差分は実験で示すまで新規性として確定しない。

## 4. Conversation-derived hypotheses

本ドキュメント群の発端となる観察:

1. アプリ全体をAIにレビューさせると多数の見落としが生じる。
2. ASTで構造を把握し、部分構造ごとにレビューすれば探索漏れを減らせるのではないか。
3. AST node単位だけではrelation/path/global invariantの問題を落とす。
4. Program GraphをLLM contextではなくreview process controllerへ使うべきではないか。
5. HigherGraphenのContext、Gluing、Invariant、Obstruction、Evidence、Projection、Coverage、Morphismが適合するのではないか。
6. 製品名をReviewGraphenとする。

設計への変換:

| Observation | Design object |
| --- | --- |
| 「何を見ていないか」が不明 | obligation universe / frontier / coverage |
| 部分構造へ分割 | ReviewContextEnvelope / Context Cover |
| relationを見落とす | Relation obligation |
| cross-file behavior | Path obligation |
| 全体性が失われる | Invariant obligation / Gluing |
| LLMの判断を事実化する危険 | ReviewClaim / Evidence / Decision separation |
| 変更で過去レビューが古くなる | Change Morphism / Staleness |

## 5. Document trace matrix

| Document | Primary sources / decisions |
| --- | --- |
| `00_vision_and_scope.md` | conversation observations、SWE-PRBench hypothesis。 |
| `01_product_positioning.md` | HigherGraphen package boundaries / intermediate-tools map。 |
| `02_research_foundation.md` | PDG/CPG、CodexGraph、RepoGraph、LLMxCPG、RepoAudit、SWE-PRBench、Archer。 |
| `03_conceptual_model.md` | HigherGraphen core concepts + review process separation。 |
| `04_highergraphen_mapping.md` | HigherGraphen README/core concepts/architecture product。 |
| `05_system_architecture.md` | HigherGraphen crate boundaries + standalone intermediate tool decision。 |
| `06_program_space_ingestion.md` | Program graph literature + current PR lift model。 |
| `07_review_obligation_model.md` | Archer + current review target contract + coverage hypothesis。 |
| `08_context_cover_gluing.md` | HigherGraphen Context/Section/Gluing concepts。 |
| `09_review_execution_and_agent_protocol.md` | HigherGraphen AI agent integration + claim boundary。 |
| `10_evidence_and_verification.md` | RepoAudit/Archer + HigherGraphen evidence concepts。 |
| `11_coverage_and_scheduling.md` | obligation universe hypothesis + HigherGraphen coverage/analytics。 |
| `12_incremental_review_and_staleness.md` | Morphism/preservation + change impact design。 |
| `13_cli_contract.md` | current HigherGraphen CLI-first/schema-first pattern。 |
| `14_report_and_schema_contract.md` | current HigherGraphen ReportEnvelope/ProjectionViewSet pattern。 |
| `15_storage_and_repository_layout.md` | local-first/auditable process design。 |
| `16_security_and_trust_boundary.md` | untrusted repository/model/tool boundary analysis。 |
| `17_evaluation_and_research_plan.md` | public benchmarks and research hypotheses。 |
| `18_mvp_roadmap.md` | risk-first vertical slice。 |
| `21_migration_from_highergraphen.md` | current `pr-review` / `test-gap` contracts。 |

## 6. Open verification items

Implementation開始前または各integration追加前に再確認するもの:

- HigherGraphen latest release and schema compatibility。
- provider-specific structured output/API contract。
- GitHub/GitLab integration permission model。
- benchmark artifact availability and license。
- Rust parser/macro expansion limitations。
- model data retention/training policy。
- Code Review Bench and SWE-PRBench current versions。
- Archer replication artifacts。

これらは時間とともに変わり得るため、本書の2026-08-07 snapshotを恒久的な事実として扱いません。

## 7. Citation URLs

- <https://github.com/CAPHTECH/higher-graphen>
- <https://arxiv.org/abs/2408.03910>
- <https://arxiv.org/abs/2410.14684>
- <https://www.usenix.org/conference/usenixsecurity25/presentation/lekssays>
- <https://proceedings.mlr.press/v267/guo25n.html>
- <https://arxiv.org/abs/2603.26130>
- <https://github.com/withmartian/code-review-benchmark>
- <https://arxiv.org/abs/2607.01808>
