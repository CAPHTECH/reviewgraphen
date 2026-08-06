# 07. Review Obligation Model

> Status: Draft v0.1  
> Central object: `ReviewObligation`

## 1. ReviewObligationを導入する理由

AIレビューで見落としが起きる最大の理由の一つは、「何を確認すべきか」の集合が存在しないことです。

ReviewGraphenは、repositoryを読ませる前に、ProgramSpaceとReviewProfileから有限かつversionedなReview Obligation Universeを生成します。

```text
U = synthesize(snapshot, profile, rule_set, extractor_capabilities)
```

この宇宙 \(U\) が、coverageと未処理frontierの分母になります。

## 2. Obligation schema

```yaml
id: obligation:code-review:payment.idempotency:...
profile_id: code-review@1
rule_id: payment.idempotency@2
target:
  kind: path
  refs:
    - symbol:CheckoutController.submit
    - relation:calls:PaymentRepository.charge
property:
  id: payment.at_most_once
  statement: A user action must cause at most one external charge.
context_requirement:
  contexts: [ui-event, payment, persistence]
  neighborhood:
    callers: 2
    callees: 3
  include_paths: true
evidence_requirement:
  accepted_any:
    - executable_test
    - verified_idempotency_guard
risk:
  impact: critical
  exposure: external
  prior: 0.8
applicability:
  status: applicable
provenance:
  generated_by: payment.idempotency@2
  source_ids: [...]
state:
  lifecycle: generated
```

## 3. Target model

### 3.1 Node obligation

例:

- public functionのinput validation。
- serializerのfield compatibility。
- lifecycle methodのresource release。
- configuration keyのdefault。

### 3.2 Relation obligation

例:

- callerとcalleeのerror contract。
- UIからpersistenceへの直接依存。
- owner境界を越えるDB access。
- modelとwire schemaのnullability。
- eventとhandlerのdelivery semantics。

RelationはReviewGraphenの重要な差分です。Nodeがすべて局所的に正しくても、relationが誤っていることがあります。

### 3.3 Subgraph obligation

例:

- login feature全体。
- payment pipeline。
- background sync。
- data export。
- authentication boundary。

subgraphは単なるk-hop neighborhoodではなく、profileが定義したsemantic groupingを持ちます。

### 3.4 Path obligation

例:

```text
tap -> controller -> repository -> external API
source -> validation -> persistence
untrusted input -> parser -> command execution
begin transaction -> writes -> commit/rollback
```

pathはsource/sink、required waypoint、forbidden edge、temporal orderを持ち得ます。

### 3.5 Invariant obligation

例:

- unauthenticated stateからPIIへ到達できない。
- paymentは高々一回成立する。
- public API changeはbackward compatibility policyを満たす。
- dispose後にstate updateしない。
- requirementはdesignとtestへmappingされる。

### 3.6 Morphism obligation

変更前後で保持すべき構造を確認します。

- interface preservation。
- invariant preservation。
- lost test relation。
- direct dependencyの新規発生。
- projection lossの増加。

## 4. ReviewRule

ReviewRuleは次を持ちます。

```yaml
id: architecture.no_cross_context_db_access
version: 1
applies_to:
  target_kinds: [relation, path]
requires_capabilities:
  - ownership
  - dependency
match:
  relation: persists_to
  where:
    source_context_ne_target_owner_context: true
generates:
  property: architecture.context_boundary
  evidence_requirement:
    - accepted_boundary_exception
    - mediated_api_path
default_risk:
  impact: high
```

ruleの種類:

- **enumeration rule**: changed symbolごとにobligationを作る。
- **pattern rule**: structural patternに一致する対象へ作る。
- **invariant rule**: invariant scopeごとに作る。
- **gap rule**: test、owner、contract等の欠落へ作る。
- **change rule**: morphismのlost/distorted structureへ作る。
- **candidate rule**: AIが提案したrisk hypothesisをunreviewed obligation candidateにする。

## 5. Deterministic generation

同じ入力から同じobligationを作るため、IDは次のcanonical tupleから生成します。

```text
(profile_id,
 rule_id,
 normalized_target_refs,
 property_id,
 snapshot_semantic_id,
 applicability_scope)
```

説明文、priority score、timestampはIDへ含めません。

## 6. Obligation candidate

LLMが「このrepositoryではmulti-tenant isolationも確認すべき」と提案することは有用です。ただし、それは既存universeへ自動追加しません。

```text
ReviewObligationCandidate
  -> explicit rule/ad-hoc review decision
  -> accepted into supplemental universe
```

supplemental universeはbase universeと区別してcoverageを報告します。

## 7. Applicability

ruleが生成されなかった理由を区別します。

```text
applicable
not_applicable
capability_missing
input_unknown
excluded_by_policy
deferred
```

`capability_missing`を`not_applicable`へ変換すると、解析不能領域がcoverageから消えます。

## 8. Evidence requirement

obligationは「LLMに一度考えさせる」だけで完了しません。propertyごとに要求するevidenceを宣言します。

例:

| Property | Evidence requirement |
| --- | --- |
| null dereference | path witnessまたはcompiler/static analyzer proof |
| idempotency | guard fact + duplicate request test、またはformal invariant |
| API compatibility | schema diff + compatibility checker |
| missing test | target-to-test mapping + changed behavior relation |
| transaction atomicity | path/trace + begin/commit/rollback relation |
| architecture boundary | ownership fact + dependency path |
| human policy exception | authority、scope、expirationを持つdecision |

evidence requirementが満たせない場合、obligationはreviewedでもverifiedではありません。

## 9. Dependency between obligations

obligation graphはDAGとは限りませんが、実行依存を持ちます。

```text
O1: resolve call target
  -> O2: review call contract
  -> O3: verify payment path idempotency
```

dependency kind:

- `requires_fact`
- `requires_context`
- `requires_claim`
- `requires_evidence`
- `blocks`
- `refines`
- `duplicates`
- `glues_with`

cycleがある場合、固定点iteration、joint context、またはobstructionが必要です。

## 10. Deduplicationと合成

複数ruleが同じtarget/propertyへobligationを生成する場合、単純削除ではなくprovenanceを合成します。

```text
O(rule A) + O(rule B)
  -> one obligation
     with multiple generation reasons
     and strongest compatible evidence requirement
```

合成不能な場合は別obligationのまま`related`で結びます。

## 11. Risk descriptor

riskは単一scoreだけにしません。

```yaml
risk:
  impact: critical
  exposure: public_api
  change_proximity: direct
  structural_role: dominator
  test_gap: true
  uncertainty: high
  business_criticality: payment
```

schedulerはこれをscoreへ投影できますが、raw dimensionsを残します。

## 12. Rule pack

ReviewProfileは複数rule packを組み合わせます。

初期Code Review profile:

```text
baseline/
  changed-symbol
  public-api
  error-propagation
  test-relation

async/
  concurrent-reentry
  cancellation
  lifecycle-after-dispose

security/
  auth-reachability
  untrusted-input-flow
  secret-exposure

architecture/
  context-boundary
  forbidden-dependency

persistence/
  transaction
  idempotency
  cache-invalidation
```

rule packを無制限に有効化するとobligation explosionが起きます。profileは対象riskとbudgetに応じて明示選択します。

## 13. Obligation explosion

対策:

1. deterministic grouping。
2. equivalent target consolidation。
3. relation/path dominance。
4. change proximity filter。
5. risk threshold。
6. budget-aware deferral。
7. low-risk obligationのsampling。
8. profile-specific exclusions。
9. generated code policy。
10. repeated patternのrepresentative review + declared inference。

ただしdeferred obligationはuniverseから消さず、coverageに残します。

## 14. Completion criteria

obligationの`completed`は、reviewerが構造化結果を返したことを意味します。

より強い状態:

```text
visited
completed
claim_supported
verified
decision_resolved
non_stale
```

profileまたはgate policyが必要levelを決めます。

## 15. Generic invariants

1. target refsはProgramSpace内で解決可能、またはunresolvedを明示する。
2. propertyはversioned identifierを持つ。
3. generated obligationはrule provenanceを持つ。
4. evidence requirementなしのcritical obligationを許可しない。
5. capability不足をnot_applicableへ変換しない。
6. merged obligationは全generator provenanceを保持する。
7. deferred obligationはcoverage denominatorに残る。
8. AI-proposed obligation candidateはbase universeへ自動参加しない。
9. snapshot変更後のobligationはpreservation確認なしにcurrent扱いしない。
