# 11. Coverage and Scheduling

> Status: Draft v0.1  
> Principle: coverageは安全性ではなく、明示された義務集合に対する処理状態

## 1. なぜcoverageが必要か

free-form AI reviewでは、出力コメント以外に工程状態が残りません。ReviewGraphenは、何を見たかより先に「何を見る義務があるか」を定義し、その充足度を測ります。

ただし、coverageという語は誤った安心を生みやすいため、単一数値にしません。

## 2. Universe descriptor

coverageの分母は次で一意に決まります。

```yaml
universe:
  snapshot_id: ...
  profile_id: code-review@1
  rule_set_hash: ...
  extractor_set_hash: ...
  policy_version: ...
  generation_time: ...
  limitations:
    - dynamic_dispatch_partial
```

このdescriptorが異なるcoverageを比較する場合、差分を明示します。

## 3. Coverage dimensions

### 3.1 Extraction coverage

ProgramSpaceをどこまで構築できたか。

- files parsed。
- symbols resolved。
- calls resolved。
- paths modeled。
- tests mapped。
- contexts assigned。
- runtime traces observed。

### 3.2 Obligation generation coverage

有効ruleのうち、必要capabilityを満たしてobligationを生成できた割合。

### 3.3 Visit coverage

reviewerへ実際に渡したobligation。

### 3.4 Completion coverage

structured resultまたはvalid abstentionを得たobligation。

### 3.5 Evidence coverage

evidence requirementの一部または全部を満たしたobligation。

### 3.6 Verification coverage

required verifier procedureを完了したobligation。

### 3.7 Resolution coverage

claim、finding、exception、unknownがpolicy上解決されたobligation。

### 3.8 Fresh coverage

current snapshotでstaleでないcoverage。

## 4. Weighted coverage

obligation \(o\) のweight \(w(o)\) を用います。

\[
C_k =
\frac{\sum_{o \in U} w(o) I_k(o)}
     {\sum_{o \in U} w(o)}
\]

ただしweightが不透明だと数字を操作できます。raw countとweighted countを併記します。

```text
verified coverage:
  raw:      324 / 500 = 64.8%
  weighted: 82.1%
```

## 5. Weight model

候補dimension:

- impact。
- exposure。
- change proximity。
- structural centrality。
- reachability。
- historical defect density。
- test gap。
- ownership gap。
- semantic novelty。
- uncertainty。
- public API。
- data sensitivity。
- business criticality。

例:

\[
w(o)=
w_{impact}
\times w_{exposure}
\times w_{change}
\times w_{criticality}
\times w_{uncertainty}
\]

積は極端値を生みやすいため、実装ではlog変換、bounded scale、またはlexicographic classを検討します。

## 6. Riskとpriorityの分離

- **risk**: 欠陥が存在した場合の影響と可能性に関するdescriptor。
- **priority**: 現在のbudget、dependency、cost、information gainを考慮した処理順。

criticalでも既に強いevidenceがあるobligationより、highでunknownなobligationを先に処理する場合があります。

## 7. Scheduling objective

budget \(B\) の下で、選択集合 \(S\) を決めます。

\[
\max_{S \subseteq U}
\sum_{o \in S}
\left(
expected\_risk\_reduction(o)
+
information\_gain(o)
+
coverage\_gain(o)
\right)
\]

subject to:

\[
cost(S)\le B
\]

これは真のriskを知っているという意味ではありません。明示したheuristicによるreview plan candidateです。

## 8. Scheduling features

### 8.1 Structural role

- dominator。
- articulation point。
- bridge。
- central dependency。
- fan-in/fan-out。
- SCC membership。

### 8.2 Change impact

- changed directly。
- impact cone。
- public API。
- schema/config migration。
- downstream tests。

### 8.3 Review state

- unseen。
- previous abstention。
- conflicting claims。
- evidence missing。
- stale。
- gluing blocker。

### 8.4 Cost

- context size。
- expected tokens。
- analyzer runtime。
- verifier availability。
- human capability requirement。

## 9. Scheduling modes

| Mode | 用途 |
| --- | --- |
| exhaustive | 小規模repo、研究fixture |
| changed-region | PR review |
| risk-first | limited budget |
| evidence-gap-first | sign-off前 |
| gluing-blocker-first | local review完了後 |
| stale-first | incremental update |
| policy-required | regulated/critical workflow |
| representative sampling | repetitive low-risk structures |

## 10. Stop conditions

reviewを止める条件は「コメントが出なくなった」ではありません。

例:

- budget exhausted。
- policy-required coverage reached。
- no ready obligations。
- unresolved critical obstruction。
- human decision required。
- remaining obligations below accepted risk threshold。
- extractor limitation prevents further progress。

stop reasonをreportへ残します。

## 11. Gate status

```text
pass
blocked
incomplete
```

- `pass`: policy-required obligationsがfreshかつ必要levelへ到達し、blocking obstructionがない。
- `blocked`: accepted finding、failed invariant、policy violationがある。
- `incomplete`: capability不足、未処理、stale、unknown、human decision待ち。

`incomplete`をfailure扱いするかはCI policyが決めますが、意味を`pass`へ変えません。

## 12. Coverage report example

```yaml
coverage:
  universe:
    obligations: 500
    critical: 12
    high: 88
    limitations:
      - interprocedural_data_flow_partial
  extraction:
    symbol_resolution: 0.96
    call_resolution: 0.71
  stages:
    visited:
      raw: 420
      weighted: 0.91
    completed:
      raw: 398
      weighted: 0.88
    evidence_supported:
      raw: 250
      weighted: 0.79
    verified:
      raw: 180
      weighted: 0.72
    fresh_verified:
      raw: 174
      weighted: 0.69
  unresolved:
    unseen: 80
    abstained: 22
    unverifiable: 18
    stale: 6
```

## 13. Calibration

risk scoreと実際のbug発生を継続評価します。

- score binごとのtrue positive率。
- severity predictionとhuman severity。
- rule別precision/recall。
- high-risk false negative。
- scheduler選択外から見つかったbug。
- model/provider差。
- language/project差。

calibrationされていないscoreはrank signalとしてのみ使います。

## 14. Coverage gaming

防止すべき例:

- low-risk getter obligationを大量生成して分母を膨らませる。
- unsupported ruleをnot_applicableへする。
- skipped filesをuniverseから消す。
- 一度LLMへ渡しただけでverifiedにする。
- staleを除外して高coverageを表示する。
- evidence requirementを弱める。
- rule packをPRごとに都合よく変える。

対策:

- profile/rule hash固定。
- rawとweighted併記。
- excluded/deferred/unsupported件数表示。
- policy変更audit。
- comparison時のuniverse diff。
- benchmark用profile固定。

## 15. HigherGraphen weighted coverage

HigherGraphenのcoverage selection、candidate optimization、graph analyticsを利用できます。ただしReviewGraphen固有のcoverage semanticsはprofileとReviewObligation stateで定義します。

coreへrisk formulaを埋め込まず、ReviewGraphenのscheduler policyとして保持します。

## 16. Invariants

1. coverageはuniverse descriptorなしに出力しない。
2. rawとweightedを区別する。
3. visitedをverifiedと呼ばない。
4. unsupported、excluded、deferred、staleを表示する。
5. risk scoreをtruth probabilityと呼ばない。
6. schedulerは選ばなかったobligationをuniverseから削除しない。
7. policy変更でcoverage意味が変わる場合、比較不能を示す。
8. pass、blocked、incompleteを別状態にする。
