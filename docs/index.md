# ReviewGraphen Documentation Index

> Status: Draft v0.1  
> Updated: 2026-08-10

## 1. 目的別の読み順

### 構想を評価する

1. [`00_vision_and_scope.md`](00_vision_and_scope.md)
2. [`01_product_positioning.md`](01_product_positioning.md)
3. [`02_research_foundation.md`](02_research_foundation.md)
4. [`17_evaluation_and_research_plan.md`](17_evaluation_and_research_plan.md)

### 実装設計を理解する

1. [`03_conceptual_model.md`](03_conceptual_model.md)
2. [`04_highergraphen_mapping.md`](04_highergraphen_mapping.md)
3. [`05_system_architecture.md`](05_system_architecture.md)
4. [`06_program_space_ingestion.md`](06_program_space_ingestion.md)
5. [`07_review_obligation_model.md`](07_review_obligation_model.md)
6. [`08_context_cover_gluing.md`](08_context_cover_gluing.md)
7. [`09_review_execution_and_agent_protocol.md`](09_review_execution_and_agent_protocol.md)
8. [`10_evidence_and_verification.md`](10_evidence_and_verification.md)
9. [`11_coverage_and_scheduling.md`](11_coverage_and_scheduling.md)
10. [`12_incremental_review_and_staleness.md`](12_incremental_review_and_staleness.md)

### CLI・schema・保存形式を実装する

1. [`13_cli_contract.md`](13_cli_contract.md)
2. [`14_report_and_schema_contract.md`](14_report_and_schema_contract.md)
3. [`15_storage_and_repository_layout.md`](15_storage_and_repository_layout.md)
4. [`16_security_and_trust_boundary.md`](16_security_and_trust_boundary.md)

### 開発を開始する

1. [`18_mvp_roadmap.md`](18_mvp_roadmap.md)
2. [`19_implementation_backlog.md`](19_implementation_backlog.md)
3. [`21_migration_from_highergraphen.md`](21_migration_from_highergraphen.md)
4. [`../AGENTS.md`](../AGENTS.md)

## 2. 基本文書

| 文書 | 役割 |
| --- | --- |
| [`00_vision_and_scope.md`](00_vision_and_scope.md) | 解く問題、設計原則、非目的、成功条件。 |
| [`01_product_positioning.md`](01_product_positioning.md) | HigherGraphenの中での層、初期profile、境界。 |
| [`02_research_foundation.md`](02_research_foundation.md) | 先行研究、研究上の空白、検証可能な仮説。 |
| [`03_conceptual_model.md`](03_conceptual_model.md) | 三空間、ReviewObligation、Claim、Evidence、状態モデル。 |
| [`04_highergraphen_mapping.md`](04_highergraphen_mapping.md) | HigherGraphen primitiveとの対応と非対応。 |
| [`05_system_architecture.md`](05_system_architecture.md) | コンポーネント、依存方向、決定論境界。 |
| [`06_program_space_ingestion.md`](06_program_space_ingestion.md) | AST、symbol、call、data-flow、test等の取り込み。 |
| [`07_review_obligation_model.md`](07_review_obligation_model.md) | obligation universe、rule pack、target/property。 |
| [`08_context_cover_gluing.md`](08_context_cover_gluing.md) | 局所文脈、投影、overlap、gluing。 |
| [`09_review_execution_and_agent_protocol.md`](09_review_execution_and_agent_protocol.md) | reviewer adapter、agent protocol、abstention。 |
| [`10_evidence_and_verification.md`](10_evidence_and_verification.md) | claimとevidence、verifier、受理規則。 |
| [`11_coverage_and_scheduling.md`](11_coverage_and_scheduling.md) | 多段階coverage、risk weight、budget選択。 |
| [`12_incremental_review_and_staleness.md`](12_incremental_review_and_staleness.md) | commit差分、impact cone、stale伝播。 |
| [`13_cli_contract.md`](13_cli_contract.md) | command family、exit code、CI利用。 |
| [`14_report_and_schema_contract.md`](14_report_and_schema_contract.md) | report envelope、schema versioning、projection。 |
| [`15_storage_and_repository_layout.md`](15_storage_and_repository_layout.md) | local-first event log、derived index、artifact store。 |
| [`16_security_and_trust_boundary.md`](16_security_and_trust_boundary.md) | 悪意あるrepo、prompt injection、秘密情報、実行制御。 |
| [`17_evaluation_and_research_plan.md`](17_evaluation_and_research_plan.md) | RQ、baseline、dataset、metric、ablation。 |
| [`18_mvp_roadmap.md`](18_mvp_roadmap.md) | 実装段階と各exit criteria。 |
| [`19_implementation_backlog.md`](19_implementation_backlog.md) | epic、task、acceptance criteria。 |
| [`20_open_source_and_commercial_boundary.md`](20_open_source_and_commercial_boundary.md) | 公開coreと商用境界。 |
| [`21_migration_from_highergraphen.md`](21_migration_from_highergraphen.md) | 現行 `pr-review` / `test-gap` からの移行。 |
| [`glossary.md`](glossary.md) | 用語と状態の定義。 |
| [`source_trace.md`](source_trace.md) | HigherGraphenおよび研究資料との対応。 |

## 3. ADR

| ADR | 決定 |
| --- | --- |
| [`adr/0001-reviewgraphen-is-an-intermediate-tool.md`](adr/0001-reviewgraphen-is-an-intermediate-tool.md) | ReviewGraphenをIntermediate Toolとする。 |
| [`adr/0002-separate-artifact-review-evidence-spaces.md`](adr/0002-separate-artifact-review-evidence-spaces.md) | 三空間を分離する。 |
| [`adr/0003-obligations-define-the-coverage-universe.md`](adr/0003-obligations-define-the-coverage-universe.md) | obligation universeをcoverageの分母とする。 |
| [`adr/0004-project-minimal-context-with-declared-loss.md`](adr/0004-project-minimal-context-with-declared-loss.md) | LLMへ最小投影を渡しlossを宣言する。 |
| [`adr/0005-llm-output-is-a-reviewable-claim.md`](adr/0005-llm-output-is-a-reviewable-claim.md) | LLM出力をaccepted factにしない。 |
| [`adr/0006-standalone-repository-over-highergraphen.md`](adr/0006-standalone-repository-over-highergraphen.md) | 独立repoとしてHigherGraphenへ依存する。 |
| [`adr/0007-local-first-event-log-and-derived-index.md`](adr/0007-local-first-event-log-and-derived-index.md) | event logを永続記録、DBを派生indexとする。 |
| [`adr/0008-language-neutral-core-profile-specific-extractors.md`](adr/0008-language-neutral-core-profile-specific-extractors.md) | coreを言語非依存、抽出器をadapter化する。 |
| [`adr/0009-rust-development-harness.md`](adr/0009-rust-development-harness.md) | product crateより先にRust開発・検証基盤を固定する。 |
| [`adr/0018-d2-execution-claim-report-and-index-v3.md`](adr/0018-d2-execution-claim-report-and-index-v3.md) | D2 execution/claim atomicity、report v2、derived-index schema v3のAccepted設計契約。 |
| [`adr/0019-replayed-v2-run-session.md`](adr/0019-replayed-v2-run-session.md) | admission-bound V2 replay session、durability uncertainty、exact replay count/byte limits。 |
| [`adr/0020-minimal-deterministic-fake-runtime.md`](adr/0020-minimal-deterministic-fake-runtime.md) | admission-bound deterministic fake D2 runtime と crash/resume 境界。 |
| [`adr/0021-m4-evidence-bound-verification.md`](adr/0021-m4-evidence-bound-verification.md) | M4のD2 claim authority bridge、固定verifier、human admission、index v4/report v3のAccepted設計契約。 |
| [`adr/0022-m5-context-cover-and-gluing.md`](adr/0022-m5-context-cover-and-gluing.md) | M5のContext Cover、閉じたSection assignment、source-bound gluing、index v5/report v4のAccepted設計契約。 |
| [`adr/0023-m6-incremental-review-and-staleness.md`](adr/0023-m6-incremental-review-and-staleness.md) | M6のtwo-run change morphism、property-sensitive staleness、preservation verification、partial rerun、index v6/report v5/gateのAccepted設計契約。 |

## 4. Schemaと参照シナリオ

- [`../schemas/README.md`](../schemas/README.md)
- [`../examples/double-submit-payment/README.md`](../examples/double-submit-payment/README.md)
- [`../skills/reviewgraphen/SKILL.md`](../skills/reviewgraphen/SKILL.md)

## 5. 文書の規範性

優先順位は次の通りです。

1. Accepted ADR。
2. Schema contract。
3. Conceptual modelとarchitecture。
4. Workflow文書。
5. Exampleとprojection。
6. 説明用README。

矛盾が見つかった場合、低い優先度の文書を暗黙に解釈して合わせず、ADRまたはschema変更として解消します。
