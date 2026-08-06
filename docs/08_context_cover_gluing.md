# 08. Context, Cover, and Gluing

> Status: Draft v0.1  
> Core idea: 局所レビューを大域的保証へ短絡しない

## 1. なぜContextが必要か

repositoryをfileやfunctionで分割するだけでは、意味の境界と一致しません。

一つのfunctionは次の複数contextに属し得ます。

- feature。
- ownership。
- security。
- lifecycle。
- persistence。
- API compatibility。
- platform。
- deployment。
- policy。

逆に、一つのfeatureは複数fileとserviceにまたがります。

ReviewGraphenは、ReviewObligationを処理する局所領域を`Context`として明示します。

## 2. Contextの種類

| Context type | 例 |
| --- | --- |
| structural | module、package、service、layer |
| behavioral | login flow、payment flow、sync flow |
| ownership | team、bounded context、data owner |
| policy | PII、security、compliance、critical path |
| runtime | mobile lifecycle、request scope、transaction |
| change | PR、commit、blast radius |
| review | target-specific focus、required expertise |
| evidence | test environment、trace window、build profile |

Contextは単なるtagではなく、vocabulary、validity、assumption、rule scopeを持ちます。

## 3. Cover

Repository Space \(\mathcal{P}\) をcontexts \(\{C_i\}\) でcoverします。

\[
\mathcal{P} \subseteq \bigcup_i C_i
\]

完全coverができない場合はuncovered regionを明示します。

cover quality:

- source cells covered。
- critical relations covered。
- changed cells covered。
- reviewable paths covered。
- context overlap resolved。
- uncovered unknown。

contextsは重複してよく、むしろ重要な欠陥はoverlapに現れます。

## 4. ReviewContextEnvelope

LLMまたはhumanへ渡す作業単位です。

```yaml
id: context-envelope:...
obligation_id: ...
target:
  refs: [...]
property:
  id: payment.at_most_once
included:
  source_fragments: [...]
  structural_neighbors: [...]
  relevant_paths: [...]
  invariants: [...]
  tests: [...]
  prior_evidence: [...]
assumptions:
  - UI event may fire more than once
unknowns:
  - gateway-side idempotency behavior unavailable
excluded:
  - analytics
  - localization
unresolved:
  - dynamic call target at symbol:...
information_loss:
  - full repository not included
  - indirect calls beyond depth 3 omitted
source_ids: [...]
projection_hash: ...
```

## 5. Context construction policy

### 5.1 Target first

contextはrepository summaryから始めず、obligation targetとpropertyから構築します。

### 5.2 Property-specific relations

冪等性レビューならevent multiplicity、state transition、external side effect、retryを優先します。null safetyならdef-use、branch、call contractを優先します。

### 5.3 Bounded expansion

例:

- callers depth 2。
- callees depth 3。
- data-flow until sink。
- path count max 20。
- source excerpt max N lines。
- related tests max 10。
- token budget max 8,000。

boundはprojection metadataへ残します。

### 5.4 Unknown before prose summary

unresolved relationやunsupported analysisをsummaryで埋めません。unknownを構造として渡します。

### 5.5 Diff anchoring

PR reviewでは変更点をanchorとし、unchanged contextを役割付きで区別します。

```text
changed
required predecessor
required successor
test
invariant
counterexample
background
```

## 6. Projection loss

contextを小さくすることは情報を失うことです。ReviewGraphenはlossを欠陥ではなく管理対象にします。

代表的なloss:

- indirect call omitted。
- dynamic dispatch unresolved。
- path truncated。
- test implementation omitted。
- historical context omitted。
- generated code excluded。
- runtime configuration unavailable。
- comments/docs not included。
- business requirement unavailable。

lossにはseverityとaffected propertiesを持たせます。

## 7. Section

各Context上で得たlocal review resultをSectionとして扱います。

```yaml
section:
  context_id: context:payment
  assignments:
    idempotency_boundary: absent
    duplicate_event_possible: true
    gateway_contract: unknown
  claim_ids: [...]
  assumptions: [...]
  evidence_ids: [...]
```

Sectionはglobal truthではありません。context restrictionを持ちます。

## 8. Restriction

Context \(C_i\) のSectionをoverlap \(C_i \cap C_j\) へ制限します。

payment contextとUI lifecycle contextのoverlapでは、例えば次だけを比較します。

- event multiplicity。
- submit state。
- cancellation。
- retry。
- side-effect initiation。

全Sectionのすべてを比較する必要はありません。

## 9. Gluing

local Sectionsがoverlapで互換であり、global invariantsを満たす場合にglobal assignment候補を作ります。

```text
Auth Section
  user_id is present after authenticate()

Profile Section
  loadProfile accepts nullable user_id

Overlap
  authenticate -> loadProfile

Gluing result
  conflict: nullability assumption differs
```

この例では、個別reviewがどちらも「問題なし」でもgluing failureが発生し得ます。

## 10. Gluing checkの種類

### 10.1 Contract compatibility

- input/output type。
- nullability。
- error semantics。
- retry semantics。
- timeout / cancellation。
- ownership。
- lifecycle。

### 10.2 Assumption compatibility

- exactly-once / at-least-once。
- authenticated / anonymous。
- cached / current。
- transaction active / inactive。
- initialized / disposed。

### 10.3 Evidence compatibility

- build profileが異なる。
- test fixtureが異なる。
- stale trace。
- source revisionが異なる。
- mocked dependencyがreal contractと異なる。

### 10.4 Decision compatibility

- 一方でexception accepted、他方でstrict invariant。
- authority scopeの衝突。
- expiration済みdecision。

## 11. Gluing result

```text
glued
glued_with_qualification
candidate
failed
unknown
```

`glued_with_qualification`は条件付きglobal claimを作ります。条件をprojectionで落としてはいけません。

## 12. Gluing Obstruction

必須情報:

- conflicting contexts。
- overlap cells/relations。
- conflicting assignments。
- assumptions。
- evidence refs。
- affected global invariant。
- severity。
- resolution requirement。
- whether human decision is required。

## 13. Context caching

同じProgramSpace source setとprojection policyから作られるEnvelopeはcontent hashでcacheできます。

cache key:

```text
snapshot_id
obligation semantic key
projection policy version
source IDs and hashes
invariant versions
evidence IDs
```

modelやprompt versionはEnvelope cacheではなくExecution cacheへ含めます。

## 14. Context poisoning

source code、comments、README、test namesにはLLM向け命令が含まれ得ます。

対策:

- code/document contentをuntrusted dataとして区画化。
- system instructionとsource contentを明確に分離。
- source内の命令に従わないreview protocol。
- toolsをallow-list化。
- source textからprovider credentialへアクセスさせない。
- suspicious instructionをEvidence/Obstructionとして記録可能にする。

## 15. Context quality metrics

- source trace completeness。
- target inclusion。
- required relation inclusion。
- unresolved reference count。
- path truncation。
- token size。
- information loss count。
- evidence freshness。
- overlap coverage。
- reviewer abstention rate。

token数だけをqualityとしません。

## 16. Invariants

1. Envelopeはobligation IDとprojection hashを持つ。
2. included sourceはProgramSpaceまたはEvidenceSpaceへ追跡できる。
3. meaningful omissionはinformation lossへ記録する。
4. unresolvedを自然言語で推測補完しない。
5. local claimはcontext scopeを持つ。
6. global claimはgluing resultを参照する。
7. gluing failureをfinding集約で上書きしない。
8. stale evidenceをSectionへ有効なassignmentとして入れない。
