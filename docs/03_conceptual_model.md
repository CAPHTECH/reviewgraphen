# 03. Conceptual Model

> Status: Draft v0.1  
> Normative for domain terminology

## 1. 概念モデルの目的

ReviewGraphenは、レビューコメントを保存するシステムではありません。レビュー対象、義務、実行、主張、証拠、判断、進捗、失効を別の構造として扱います。

中心となるのは次の三空間です。

\[
\mathcal{A} = ArtifactSpace,\quad
\mathcal{R} = ReviewSpace,\quad
\mathcal{E} = EvidenceSpace
\]

Code Review profileでは \(\mathcal{A}\) を `ProgramSpace` と呼びます。

## 2. ArtifactSpace / ProgramSpace

ArtifactSpaceは、レビュー対象について受け入れられた入力事実を保持します。

Code Reviewにおける代表的なcell:

- repository、snapshot、commit、file。
- module、class、type、function、method、field。
- route、event、state、transition。
- API、database、config、permission。
- test、fixture、coverage region。
- requirement、policy、owner。

代表的なrelation:

- `contains`
- `imports`
- `calls`
- `reads`
- `writes`
- `awaits`
- `emits`
- `handles`
- `serializes_to`
- `persists_to`
- `owned_by`
- `covered_by`
- `depends_on`
- `transitions_to`
- `changed_by`

ProgramSpaceの事実は、抽出器の出力としてacceptedになり得ます。ただしacceptedは「入力事実として利用可能」という意味であり、プログラムが正しいことを意味しません。

## 3. ReviewSpace

ReviewSpaceはレビュー工程の認識状態を保持します。

主要object:

| Object | 意味 |
| --- | --- |
| `ReviewProfile` | 対象領域、rule set、extractor requirement、evidence policyのversioned定義。 |
| `ReviewRule` | ProgramSpaceからobligationを生成する規則。 |
| `ReviewObligation` | target/property/context/evidence requirementを持つ確認責務。 |
| `ReviewPlan` | budget、依存、priorityを考慮した実行計画。 |
| `ReviewContextEnvelope` | reviewerへ渡すbounded projection。 |
| `ReviewExecution` | reviewerが一つ以上のobligationを処理した記録。 |
| `ReviewClaim` | reviewerが提案した問題あり、問題なし、判断不能等の主張。 |
| `FindingCandidate` | actionableなnegative claimの候補。 |
| `ReviewDecision` | claim、exception、sign-offに対する明示判断。 |
| `CoverageRecord` | obligation集合に対する処理段階の集計。 |
| `ReviewObstruction` | sign-off、gluing、verificationを妨げる構造化理由。 |

## 4. EvidenceSpace

EvidenceSpaceは、claimを支持、反証、限定する観測物を保持します。

代表例:

- code locationとsource excerpt hash。
- AST / symbol / call graph fact。
- static analyzer result。
- test result。
- coverage trace。
- runtime trace。
- counterexample path。
- model checker result。
- compiler error。
- API contract。
- human review decision。
- external issue / incident reference。

Evidenceは単なる添付ではありません。どのclaimに、どのcontextで、どのrelationで結びつくかを記録します。

```text
supports
refutes
qualifies
reproduces
contradicts
supersedes
```

## 5. ReviewObligation

ReviewObligation \(o\) を次で表します。

\[
o = \langle t, p, c, e, r, q, v, \pi \rangle
\]

| 記号 | 内容 |
| --- | --- |
| \(t\) | target reference。Node、Relation、Subgraph、Path、Invariant、Morphism。 |
| \(p\) | property。null safety、idempotency、boundary、compatibility等。 |
| \(c\) | context requirement。必要近傍、context type、overlap。 |
| \(e\) | evidence requirement。test、trace、static proof、human decision等。 |
| \(r\) | risk descriptor。impact、exposure、likelihood prior。 |
| \(q\) | qualification。applicability、exclusion、precondition。 |
| \(v\) | version tuple。profile、rule、extractor、snapshot。 |
| \(\pi\) | provenance。生成規則とsource IDs。 |

### 5.1 Target kind

```text
Node
Relation
Subgraph
Path
Invariant
Morphism
Projection
```

`Projection`も対象に含める理由は、レビュー報告が重要情報を落としていないかを確認するためです。

### 5.2 Property

propertyは質問文ではなく、可能な限りversioned identifierとして定義します。

例:

```text
async.concurrent_reentry
payment.idempotency
auth.forbidden_reachability
error.propagation
persistence.transaction_atomicity
api.backward_compatibility
architecture.context_boundary
test.behavioral_coverage
projection.loss_declared
```

## 6. ReviewContextEnvelope

ReviewContextEnvelopeは、ProgramSpace全体ではなく一つのobligationを判断するためのprojectionです。

必須項目:

- obligation ID。
- target source。
- direct structural neighborhood。
- relevant pathsまたはsummary。
- applicable invariants。
- existing tests and evidence。
- known unknowns。
- included source IDs。
- excluded regions。
- unresolved references。
- information loss declaration。
- projection hash。

同じobligationでもprojection strategyが異なれば別Envelopeです。

## 7. ReviewExecution

ReviewExecutionは次を記録します。

```yaml
id: execution:...
obligation_ids: [...]
reviewer:
  kind: llm | static_analyzer | human | formal_tool
  identity: ...
context_envelope_id: ...
configuration:
  prompt_version: ...
  model: ...
  tool_versions: ...
started_at: ...
finished_at: ...
result:
  claim_ids: [...]
  abstention: null
artifacts:
  raw_output_hash: ...
```

一つのexecutionが複数obligationを処理できるのは、それらが同じcontextで強く結合している場合だけです。バッチ化でtarget traceが失われてはいけません。

## 8. ReviewClaim

ReviewClaimは次のpolarityを持ちます。

| Polarity | 意味 |
| --- | --- |
| `issue_present` | 欠陥または規則違反が存在するという候補。 |
| `issue_absent` | 指定propertyについて問題を発見しなかったという限定主張。 |
| `inconclusive` | 判断材料が不足している。 |
| `not_applicable` | ruleの適用条件を満たさない。 |
| `conflict` | 他のlocal claimまたはevidenceと両立しない。 |

`issue_absent` は「安全」の同義語ではありません。context、property、evidence boundの範囲内だけで成立します。

## 9. 三つの状態軸

状態を一つのenumへ詰め込むと意味が崩れます。ReviewGraphenは三軸に分けます。

### 9.1 Obligation lifecycle

```text
generated
planned
in_progress
completed
stale
superseded
cancelled
```

### 9.2 Claim disposition

```text
proposed
supported
refuted
accepted
rejected
superseded
```

### 9.3 Verification outcome

```text
not_attempted
passed
failed
inconclusive
unsupported
expired
```

例: obligationが`completed`でもclaimは`proposed`、verificationは`not_attempted`であり得ます。

## 10. ReviewDecision

ReviewDecisionはhumanまたは明示policy authorityが行います。

対象:

- FindingCandidateのaccept / reject。
- `not_applicable`の承認。
- policy exception。
- incomplete coverageを伴うsign-off。
- completion candidateの採否。
- gluing conflictのresolution。

AIはDecisionを提案できますが、authority capabilityがない限り確定できません。

## 11. Coverage

obligation universeを \(U(S,P,R,X)\) とします。

- \(S\): snapshot。
- \(P\): profile version。
- \(R\): rule set version。
- \(X\): extractor version and completeness declaration。

各段階 \(k\) のweighted coverage:

\[
C_k = \frac{\sum_{o \in U} w(o) I_k(o)}
           {\sum_{o \in U} w(o)}
\]

代表的な \(k\):

- generated。
- planned。
- visited。
- completed。
- evidence-supported。
- verified。
- non-stale。

coverageは常にuniverse descriptorと一緒に報告します。

## 12. Context、Section、Gluing

Repositoryをcontexts \(C_1,\ldots,C_n\) でcoverします。各contextで得たlocal review assignmentをSection \(s_i\) とします。

overlapでの整合条件:

\[
s_i|_{C_i \cap C_j} \simeq s_j|_{C_i \cap C_j}
\]

ここで \(\simeq\) は単純な文字列一致ではなく、declared equivalence、assumption、invariantを考慮した互換性です。

一致しない場合、global conclusionを作らずGluing Obstructionを生成します。

## 13. Change MorphismとStaleness

snapshot \(S_0\) から \(S_1\) への変更をmorphism \(m:S_0\to S_1\) とします。

obligation、Envelope、Evidenceが参照するsource setに変更があり、relevant propertyのpreservationが確認できない場合、そのrecordをstaleにします。

```text
changed target
changed dependency cone
changed invariant
changed rule/profile
changed extractor semantics
expired runtime evidence
model/prompt policy invalidation
```

## 14. ID規則

IDは再実行時に安定させます。

```text
space:program:<repo-id>:<snapshot-hash>
space:review:<repo-id>:<run-id>
space:evidence:<repo-id>:<run-id>

obligation:<profile>:<rule-id>:<target-hash>:<version-hash>
context-envelope:<obligation-id>:<projection-hash>
execution:<run-id>:<sequence>
claim:<execution-id>:<content-hash>
evidence:<kind>:<content-hash>
decision:<authority>:<timestamp-or-ulid>
```

source locationだけでIDを作ると行移動で不安定になります。symbol identity、structural path、content hash、snapshotを組み合わせます。

## 15. Core invariants

1. AI-generated claimはconfidenceだけでacceptedにならない。
2. `verified`はVerifier recordなしに成立しない。
3. coverageはversioned universeなしに報告しない。
4. stale evidenceはcurrent sign-offを支持しない。
5. projectionはsource traceとinformation lossを持つ。
6. local claimからgluingなしにglobal claimを作らない。
7. `issue_absent`は対象propertyとcontextを越えて一般化しない。
8. deterministic source factとprobabilistic inferenceを同じprovenance classにしない。
9. 同じsnapshot/profile/rules/extractorsから生成されるobligation IDは安定する。
10. exceptionはpolicy、scope、authority、expirationを持つ。

## 16. Findingの位置

FindingはReviewGraphenの中心ではなく、review processから得られる一つのprojectionです。

```text
ReviewObligation
  -> ReviewExecution
  -> ReviewClaim(issue_present)
  -> EvidenceBinding
  -> Verification
  -> ReviewDecision
  -> Accepted Finding projection
```

これにより、コメント一覧だけでは失われる「何を確認し、どう裏付けたか」が残ります。
