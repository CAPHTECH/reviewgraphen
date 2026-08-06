# 00. Vision and Scope

> Status: Draft v0.1  
> Updated: 2026-08-07

## 1. 問題設定

AIコードレビューの主要な失敗は、モデルが個々のコードを読めないことだけではありません。より構造的な失敗は、**レビュー対象空間の探索、対象の選定、文脈構築、判断、証拠確認、進捗管理を一つのLLMセッションに押し込んでいること**です。

「アプリ全体をレビューせよ」という依頼では、次が暗黙になります。

- 何をもって「全体」とするか。
- どの関係、経路、不変条件を確認すべきか。
- どの部分を実際に確認したか。
- どの結論がコード事実で、どれがAI推論か。
- 問題なしという判断に必要な証拠は何か。
- 変更後にどの判断が無効化されたか。
- 局所的に成立した判断を大域的に接続できるか。

この暗黙性が、見落としの再現性と監査不能性を生みます。

## 2. Vision

ReviewGraphenは、レビューを次のように再定義します。

> レビューとは、対象構造から明示的なReview Obligationsを生成し、限られた文脈で実行し、ClaimをEvidenceへ結び、局所結果をGluingし、未充足義務とObstructionを残す工程である。

従来:

```text
Repository
  -> LLM
  -> Review comments
```

ReviewGraphen:

```text
Repository
  -> ProgramSpace
  -> Review Obligation Universe
  -> Review Plan
  -> Context Projections
  -> Review Executions
  -> Claims
  -> Evidence / Verification
  -> Coverage / Gluing / Obstructions
  -> Human, Agent, Audit Projections
```

## 3. 目的

### 3.1 見落としを「注意力」ではなく「未充足義務」として表す

レビュー漏れを、モデルの内的状態ではなく外部状態として管理します。

```text
unseen
planned
executed
abstained
evidence_supported
verified
stale
unverifiable
```

### 3.2 関数単体レビューから関係・経路・不変条件へ拡張する

ASTノードを入口としつつ、欠陥が現れやすい次の構造をfirst-class targetにします。

- call / dependency / ownership relation。
- data-flow / error-flow / lifecycle path。
- feature / bounded context subgraph。
- state transition。
- global invariant。
- change morphism。

### 3.3 LLMへ渡す文脈を外部構築する

全体グラフをプロンプトへ投入しません。各obligationに必要な最小projectionを作り、含めた構造、除外した構造、解決できなかった構造を記録します。

### 3.4 レビュー結果と証拠を分離する

LLMの「問題がある」「問題は見当たらない」はClaimです。静的解析、テスト、実行トレース、反例、人間判断などがEvidenceです。ClaimはEvidenceなしにverifiedになりません。

### 3.5 変更に追従する

commit間をChange Morphismとして扱い、影響範囲に応じてobligation、context、evidence、coverageをstaleへ戻します。

## 4. 設計原則

### P1. Exploration is external state

対象探索をLLMの自由行動に閉じず、obligation frontierとして保持します。

### P2. Graph is controller before context

グラフはまずレビュー工程を制御するために使います。関連コード検索はその一機能です。

### P3. Local context, global accountability

LLMには局所文脈を渡しますが、全体の進捗、overlap、gluing、stalenessはReviewGraphenが保持します。

### P4. Facts, claims, and evidence are different objects

コード解析で得た事実、AIが出した主張、主張を支える証拠を分離します。

### P5. Coverage is relative, versioned, and plural

coverageは絶対的な安全性ではありません。snapshot、profile、rule set、extractorに相対的であり、visited、reviewed、evidence-supported、verifiedを別々に測ります。

### P6. Abstention is valid output

判断不能を失敗として隠しません。`abstained`、`unverifiable`、`unknown`を正規状態にします。

### P7. Deterministic facts before probabilistic interpretation

AST、symbol、diff、test resultなどは可能な限り決定論的に抽出し、LLM推論はその上に置きます。

### P8. Human responsibility remains asymmetric

AI、静的解析器、テスト、ポリシーはレビュー工程へ参加できますが、組織的な採否責任や例外承認を自動的に代理しません。

## 5. 対象範囲

### 5.1 ReviewGraphen core

- ArtifactSpace / ReviewSpace / EvidenceSpace。
- ReviewObligationとrule pack。
- ReviewContextEnvelope。
- execution、claim、evidence binding、verification。
- coverage、scheduling、staleness。
- projectionとaudit trace。
- agent / analyzer / human adapter contract。

### 5.2 Initial Code Review Profile

- Git snapshotとdiff。
- AST / symbol / containment。
- import / dependency / direct call relation。
- changed symbolとtest relation。
- node、relation、bounded path、invariant obligations。
- Rust reference extractor。
- LLM reviewer adapter。
- deterministic verifierとhuman decision record。
- PR向けreport。

### 5.3 後続profile

- Architecture Review。
- Specification Review。
- Test Review。
- Configuration Review。
- AI-generated document / artifact review。
- Policy and contract review。

## 6. 非目的

### 6.1 完全性保証

coverage 100%は、定義したobligation universeを処理したことを意味します。未知のrule、抽出できなかったdynamic behavior、誤ったground truthまで含めて欠陥が存在しないことを意味しません。

### 6.2 LLM万能化

ReviewGraphenは巨大contextやmulti-agent数の増加でモデル能力を補おうとしません。モデルへ渡す仕事を狭くし、外部検証可能にします。

### 6.3 万能CPGの再実装

parser、type checker、compiler、LSP、CodeQL、Joern、Semgrep等が得意な解析をHigherGraphen上で再実装しません。

### 6.4 自動修正

初期版は修正案をCompletionCandidateとして出せても、適用は行いません。レビューと変更生成を分離します。

### 6.5 自動マージ

ReviewGraphenはsign-off材料を作ります。merge権限や組織責任を代替しません。

## 7. 成功条件

### 工学的成功

- 同一snapshotとprofileから同一obligation universeを生成できる。
- すべてのclaimがtarget、context projection、source IDsへ追跡できる。
- evidenceなしのclaimがverifiedにならない。
- 変更時に影響するrecordがstaleになる。
- local reviewの衝突がgluing obstructionとして表現される。
- reportがhuman / AI / audit projectionを持つ。

### 研究的成功

同一モデル、同一token budgetの下で、free-form repository reviewより次を改善すること。

- severity-weighted recall。
- relation/path由来のcross-file issue recall。
- run-to-run variance。
- evidence-backed precision。
- tokenまたは費用あたりのtrue positive。
- 未検証領域の明示性。

### プロダクト的成功

- reviewerが「どこまで見たか」を短時間で判断できる。
- agentが次に処理すべきobligationを問い合わせられる。
- CIがpolicyに基づき、pass / blocked / incompleteを区別できる。
- review結果をcommit更新後も再利用または正しく無効化できる。

## 8. 最も重要な警告

ReviewGraphenは、曖昧なAIレビューへ精密そうな数字を付ける製品になってはいけません。

coverage、risk score、verification statusは、分母、抽出境界、evidence quality、unknownを明示して初めて意味を持ちます。これらを隠した場合、ReviewGraphenは見落としを減らすのではなく、見落としへ誤った確信を与えます。
