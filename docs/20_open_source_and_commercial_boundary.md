# 20. Open Source and Commercial Boundary

> Status: Draft v0.1  
> Purpose: 公開再現性、HigherGraphenとの整合、事業余地を同時に守る  
> Note: 本文書は製品設計上の境界であり、法的助言ではない

## 1. 基本方針

ReviewGraphenの中核主張は研究・工学的に検証可能でなければなりません。obligation universe、coverage、evidence state、projection lossを閉じた実装だけで示すと、利用者は「AIがレビューした」という従来の不透明性から抜けられません。

一方、企業導入で価値を持つのはcore algorithmだけではありません。組織固有のpolicy、repository integration、運用、private evaluation、hosted execution、責任分界が大きな価値になります。

したがって、推奨境界は次です。

```text
Public reproducible core
  + Open schemas / reference profiles / local CLI
  + Published evaluation protocol

Commercial operational layer
  + Hosted execution / enterprise connectors / private policies
  + Managed evidence / governance / support / private evaluation
```

## 2. 公開を推奨する範囲

### 2.1 Core contracts

- Artifact / Review / Evidence Spaceのdomain model。
- ReviewObligation、Claim、Evidence、Verification、Decision、Coverage。
- state transitionとvalidation。
- canonical IDs / JSON。
- report schemas。
- projection loss contract。
- event log format。

これらを閉じると、ReviewGraphenという方法論の相互運用性が成立しません。

### 2.2 Local CLI baseline

- snapshot、ingest、obligations、coverage、report、gate。
- manual JSON adapter。
- baseline Rust/Git adapter。
- deterministic fake reviewer。
- verifier adapter interface。
- local-first store。

誰でもprovider keyなしでreference scenarioを再現できる必要があります。

### 2.3 Reference profile and rules

- Code Review baseline profile。
- Node / Relation / Path / Invariantの代表rule。
- double-submit fixture。
- schema examples。
- negative fixtures。

Rule engineだけ公開してruleをすべて閉じる形は、研究上の比較とcommunity validationを損ないます。

### 2.4 Evaluation protocol

- baseline condition定義。
- metric計算。
- run manifest。
- mutation framework。
- 公開可能な結果とnegative result。

第三者が「同じモデルでもprocess構造が効いたか」を検証できる最小経路を公開します。

### 2.5 Agent skill

- toolをいつ使うか。
- claim/evidence/acceptance境界。
- output interpretation。
- safety rules。

agent integration contractを閉じると、AI operator paradigmの再現性が失われます。

## 3. 商用に適する範囲

### 3.1 Hosted execution

- managed worker isolation。
- provider routing。
- cache / concurrency / retry。
- tenant separation。
- artifact encryption / retention。
- cost control。
- availability and operations。

### 3.2 Enterprise connectors

- GitHub Enterprise、GitLab、Bitbucket等。
- SSO / SCIM / directory。
- ticket、incident、SIEM、policy system。
- proprietary test/coverage platform。
- internal model gateway。

### 3.3 Organization-specific interpretation

- private rule packs。
- architecture / compliance policy packs。
- domain invariants。
- accepted exception workflow。
- ownership and escalation model。
- organization-specific risk calibration。

抽象engineを公開しても、組織の意味・責任・運用へ接地する部分は顧客資産または商用サービスになります。

### 3.4 Private evaluation assets

- 顧客repositoryから作るadjudicated corpus。
- historical incident mapping。
- false-positive suppression data。
- confidential mutation scenarios。
- organization-specific calibration。

顧客sourceそのものを製品学習へ転用しない。利用目的、保持、匿名化、二次利用を契約とpolicyで分けます。

### 3.5 Governance and audit

- approval authority。
- policy-as-code管理。
- audit retention。
- regulatory evidence package。
- exception expiry。
- organizational dashboards。
- SLA / support。

## 4. 推奨repository境界

```text
CAPHTECH/reviewgraphen                 public
  crates/
  tools/reviewgraphen-cli/
  schemas/
  profiles/code-review-baseline/
  examples/
  skills/
  evaluation/
  docs/

private / commercial repositories
  reviewgraphen-cloud/
  reviewgraphen-enterprise-connectors/
  reviewgraphen-private-rulepacks/
  reviewgraphen-evaluation-private/
  customer-specific-config/
```

秘密情報をpublic repositoryのfeature flagだけで隠しません。repositoryとrelease artifactを分離します。

## 5. HigherGraphenとのlicense整合

ReviewGraphenはHigherGraphen cratesへ依存します。公開coreのlicenseは、HigherGraphenのpublic coreと互換性を持たせる必要があります。

設計上の第一候補はHigherGraphenと同じApache-2.0です。最終決定前に次を確認します。

- 依存crateのlicense。
- parser/analyzer dependency。
- benchmark/dataset license。
- provider SDK license。
- generated schema/exampleの扱い。
- third-party code attribution。

license fileを置くだけでなく、NOTICE、dependency inventory、source traceをrelease processへ含めます。

## 6. Open-coreで閉じてはいけない箇所

次を商用版だけへ置くと、public版は名前だけのwrapperになります。

- obligation synthesisそのもの。
- coverage denominator。
- accepted/inferred/verified境界。
- projection loss。
- staleness model。
- reference verifier interface。
- end-to-end reference scenario。

これらはReviewGraphenの信頼モデルです。

## 7. 公開しなくても方法論を損なわない箇所

- providerごとの最適化prompt。
- organization-specific policy。
- proprietary risk calibration。
- hosted scheduler implementation。
- private incident corpus。
- enterprise authorization integration。
- customer deployment automation。

ただし、商用結果を研究成果として主張する場合、再現可能なpublic approximationと差分を明記します。

## 8. Data boundary

### 8.1 Source code

- defaultはlocal processing。
- provider uploadは明示policy。
- raw sourceを不要に保存しない。
- source excerptはhash/reference優先。
- hosted版ではtenant-specific encryptionとretention。

### 8.2 Model input/output

- provider、region、training use、retention policyをrun manifestへ記録可能にする。
- promptとraw outputの保存可否を分ける。
- secret redactionによる意味損失をprojection lossへ記録する。

### 8.3 Evaluation data

- benchmark licenseに従う。
- proprietary codeを公開fixtureへ変換するときは再識別とsemantic leakageを確認する。
- customer dataをcommunity benchmarkへ自動混入させない。

## 9. Contribution boundary

Public projectでは次を定めます。

- Code of Conduct。
- contribution guide。
- DCOまたはCLAの採否。
- security reporting path。
- schema compatibility policy。
- generated/AI-assisted contributionのprovenance policy。
- benchmark contamination disclosure。

AI-generated patchであっても、著作権・license・品質責任をagentへ委譲できません。contributorが提出権限と検証責任を持ちます。

## 10. 商用価値の源泉

ReviewGraphenのmoatを「coreを秘密にすること」だけへ置くのは弱い設計です。より持続的な価値は次にあります。

- organization固有のProgramSpace lift品質。
- rule/evidence policyの実運用精度。
- adjudicated private corpus。
- false positive reduction。
- incremental reuseの信頼性。
- enterprise securityと責任分界。
- existing workflowへの統合。
- longitudinal calibration。
- supportと導入設計。

public coreが強いほど、商用層がcore再説明ではなく運用価値へ集中できます。

## 11. Publication rule

公開前に各artifactを分類します。

```text
public
public-after-redaction
customer-owned
commercial-confidential
security-sensitive
third-party-restricted
```

分類不能なartifactを「とりあえずpublic repositoryへcommit」しません。

## 12. Decision gate

正式なlicense/publication decisionは次を満たしてから確定します。

1. HigherGraphenとのdependencyとlicense review。
2. 最初のpublic vertical sliceの価値確認。
3. 商用サービス仮説の分離。
4. benchmark/dataset license確認。
5. contributor model決定。
6. security disclosure process準備。

それまでは本書を設計方針として扱い、法的確定事項として扱いません。
