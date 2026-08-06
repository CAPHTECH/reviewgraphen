# 12. Incremental Review and Staleness

> Status: Draft v0.1  
> Principle: 過去のレビューは、構造が保存された範囲でのみ再利用する

## 1. 問題

PRへ追加commitが入るたびに全レビューを再実行すると高コストです。一方、変更fileだけを再レビューすると、依存経路、invariant、evidenceの失効を見落とします。

ReviewGraphenはsnapshot間をChange Morphismとして扱います。

\[
m: ProgramSpace_{S_0} \rightarrow ProgramSpace_{S_1}
\]

morphismはpreserved、changed、added、removed、unresolved、distorted structureを記録します。

## 2. Change set

```yaml
change_morphism:
  source_snapshot: S0
  target_snapshot: S1
  mappings:
    - from: symbol:A@S0
      to: symbol:A@S1
      status: modified
    - from: relation:A-calls-B@S0
      to: relation:A-calls-B@S1
      status: preserved
  added: [...]
  removed: [...]
  unresolved: [...]
```

renameやmoveをdelete/addへ潰さないことで、review recordを継承しやすくします。

## 3. Staleness対象

- ReviewObligation。
- ReviewContextEnvelope。
- ReviewExecution。
- ReviewClaim。
- Evidence。
- Verification。
- ReviewDecision。
- CoverageRecord。
- GluingResult。
- Projection。

すべてを同じ条件でstaleにする必要はありません。

## 4. Direct staleness

recordが参照するsource IDまたはhashが変わった場合。

例:

- target function変更。
- test変更。
- invariant version変更。
- policy変更。
- context projection source変更。

## 5. Indirect staleness

dependency coneに変更があり、propertyへ影響する可能性がある場合。

例:

```text
A unchanged
  calls B changed
```

Aのlocal syntax reviewはfreshでも、A→B error contract obligationはstaleになります。

## 6. Property-sensitive impact

すべての変更がすべてのpropertyを失効させるわけではありません。

例:

- コメント変更はruntime idempotency evidenceを通常失効させない。
- public visibility変更はAPI compatibility obligationを失効させる。
- logging追加はperformance obligationへ影響し得る。
- test renameだけならbehavioral evidenceをmappingできる可能性がある。
- dependency version変更はtransitive security obligationへ影響する。

Change classifierは確信できない場合、保守的にstaleへします。

## 7. Preservation

morphismがpropertyを保存すると検証できた場合、recordを継承できます。

```yaml
preservation:
  property_id: api.backward_compatibility
  source_obligation_id: ...
  target_obligation_id: ...
  status: preserved
  verifier: schema-compat@1
  evidence_ids: [...]
```

単にtarget sourceが同じ文字列だからpreservedとしません。

## 8. Invalidation graph

各recordはsource dependencyを持ちます。

```text
Evidence E
  depends_on source S1, test T1, config C1

Claim Q
  depends_on E, Envelope P

Verification V
  depends_on Q, E, verifier version

Coverage K
  depends_on obligation states
```

source変更から逆向きにstaleを伝播します。

## 9. Staleness reason

```text
target_changed
dependency_changed
context_changed
evidence_changed
test_changed
policy_changed
rule_changed
extractor_changed
model_policy_changed
decision_expired
runtime_evidence_expired
mapping_unresolved
```

複数reasonを保持します。

## 10. Incremental algorithm

1. S0/S1のProgramSpace mappingを作る。
2. added/removed/modified/unresolved structureを分類する。
3. change impact coneを計算する。
4. obligation ruleを変更領域へ再適用する。
5. stable semantic keyでold/new obligationを対応づける。
6. direct/indirect/property-sensitive stalenessを評価する。
7. preservation verifierを適用する。
8. affected Envelopeを再投影する。
9. stale claim/evidence/verificationを伝播する。
10. gluing resultを再計算する。
11. fresh coverageを更新する。
12. next review planを作る。

## 11. New and removed obligations

### Added

新しいstructureまたはrule applicabilityにより生成。

### Removed

target消失またはnot applicable化。old obligationを削除せず`superseded`へします。

### Modified

semantic keyが同じでproperty/context requirementが変化。新versionへmorphismを持たせます。

### Split / merge

一つのfunction分割やrule改訂でobligationが分裂・統合する場合、mappingを明示します。coverageの単純比較をしません。

## 12. Rebase and history rewrite

commit hashだけに依存するとrebaseで全recordが無効になります。

- repository identity。
- tree/content hash。
- symbol semantic key。
- structural mapping。
- diff equivalence。
- profile/rule version。

を組み合わせて再対応します。対応不能な場合は保守的にstaleへします。

## 13. Evidence reuse

再利用可能性:

| Evidence | Reuse rule |
| --- | --- |
| source grounding | source hashが同じ |
| unit test | target、test、config、dependenciesが保存 |
| integration test | environmentとservice contractも保存 |
| static analysis | tool/rule/source graphが保存 |
| human decision | scope、policy、expirationが有効 |
| runtime trace | source/config/environmentとretention policyに依存 |

## 14. Partial rerun

stale dependency graphから最小rerun setを選びます。

```text
stale obligation
  -> reproject context
  -> rerun reviewer only if projection changed materially
  -> rerun verifier if evidence dependency changed
  -> re-glue affected overlaps
```

model outputだけが古くても、deterministic verifier結果を再利用できる場合があります。

## 15. Freshness report

```yaml
freshness:
  total_records: 1840
  fresh: 1602
  stale: 196
  superseded: 42
  stale_by_reason:
    target_changed: 43
    dependency_changed: 88
    test_changed: 17
    policy_changed: 6
    mapping_unresolved: 42
  reusable_verified_weight: 0.73
```

## 16. Gateへの影響

strict gate:

- critical stale obligationがあれば`incomplete`。
- expired human exceptionがあれば`blocked`または`incomplete`。
- unresolved mappingがcritical pathにあれば`incomplete`。
- fresh verified coverageがthreshold未満なら`incomplete`。

過去の総verified coverageではなくfresh verified coverageを使います。

## 17. Invariants

1. stale recordを削除せず履歴を残す。
2. source mapping不能をpreservedとみなさない。
3. obligation universe変更を単純percentage比較しない。
4. decision expirationをstale伝播に含める。
5. evidence reuseはproperty-sensitiveである。
6. rebase後も可能な限りsemantic mappingを使う。
7. partial rerunはaffected gluing resultも更新する。
8. fresh coverageをcurrent gateに使用する。
