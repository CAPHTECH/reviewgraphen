# 10. Evidence and Verification

> Status: Draft v0.1  
> Principle: Claim is not Evidence

## 1. 目的

AIレビューのprecisionを損なう主因の一つは、もっともらしい説明が証拠と混同されることです。

ReviewGraphenは次を分けます。

```text
ReviewClaim
  「この問題が存在する可能性がある」

Evidence
  「この観測がclaimを支持または反証する」

Verification
  「指定手続きでclaimとevidenceの関係を評価した」

Decision
  「authorityが採否、例外、sign-offを決めた」
```

## 2. Evidence kinds

### 2.1 Structural evidence

- resolved call edge。
- ownership relation。
- direct DB access path。
- type mismatch。
- missing test relation。
- changed public symbol。
- invariant violation witness。

### 2.2 Executable evidence

- unit/integration/e2e test result。
- reproduction script result。
- compiler diagnostic。
- sanitizer/race detector。
- model checker counterexample。
- symbolic execution path。
- API compatibility checker。

### 2.3 Runtime evidence

- trace。
- log event。
- metric。
- crash report。
- transaction record。
- user interaction replay。

### 2.4 Documentary evidence

- requirement。
- ADR。
- API contract。
- policy。
- incident report。
- owner declaration。

### 2.5 Human evidence

- reviewed decision。
- domain expert statement。
- exception approval。
- reproduction confirmation。

human statementもscope、authority、timestamp、sourceを持ち、無条件の真理にはしません。

## 3. Evidence record

```yaml
id: evidence:test:duplicate-submit:...
kind: executable_test
subject_refs:
  - path:submit-to-charge
artifact:
  content_hash: ...
  media_type: application/json
producer:
  kind: test_runner
  name: cargo-test
  version: ...
snapshot_id: ...
valid_contexts:
  - context:payment
result:
  outcome: failed
  details: charge_count == 2
review_status: machine_checked
freshness:
  generated_at: ...
  expires_on_change_of:
    - symbol:CheckoutController.submit
    - symbol:PaymentRepository.charge
```

## 4. EvidenceBinding

Evidenceはclaimへ明示relationで結びます。

```yaml
binding:
  claim_id: claim:...
  evidence_id: evidence:...
  relation: supports
  strength: direct
  scope:
    property: payment.at_most_once
    contexts: [payment, ui-event]
  rationale: Duplicate submit test observes two charge calls.
```

relation:

- `supports`
- `refutes`
- `qualifies`
- `reproduces`
- `contradicts`
- `supersedes`

## 5. Verification

VerificationはVerifierが実行した手続きです。

```yaml
verification:
  id: verification:...
  claim_id: ...
  verifier:
    kind: executable_test
    id: duplicate-submit-test@1
  procedure_version: 1
  input_ids: [...]
  result: passed
  evidence_ids: [...]
  limitations:
    - gateway mock does not verify provider-side idempotency
```

`passed`は「claimが正しい」ではなく「このverification procedureがclaimを支持する結果になった」という意味です。

## 6. Verification facets

単一のlevelへ潰さず、facetを持ちます。

| Facet | 問い |
| --- | --- |
| source grounded | claimはsource factへ追跡できるか |
| reproducible | 同じ手続きで現象を再現できるか |
| path feasible | path conditionは実行可能か |
| property matched | evidenceは対象propertyを実際に検証するか |
| environment valid | build/runtime contextは対象と一致するか |
| independent | proposerと異なるmechanismか |
| fresh | current snapshotに有効か |
| authority accepted | 必要な人間判断があるか |

## 7. Negative claimの難しさ

`issue_absent`を検証する方が難しいことがあります。

例:

> このpathにnull dereferenceは存在しない。

これには、bounded path enumeration、type proof、static analyzer coverage、test samplingなどの限界が伴います。

したがってnegative claimは次を持ちます。

- checked scope。
- analysis bound。
- excluded paths。
- unknown dispatch。
- verifier completeness。
- counterexample search result。

単に「見つからなかった」を`verified_absent`へしません。

## 8. Evidence requirements resolution

一つのobligationが複数代替evidenceを許可できます。

```yaml
evidence_requirement:
  all:
    - source_grounding
  any:
    - executable_counterexample
    - static_path_proof
    - accepted_human_reproduction
```

critical propertyでは独立した二種類を要求できます。

```yaml
policy:
  critical_issue_acceptance:
    require:
      - one executable or formal evidence
      - one human decision
```

## 9. Verifier adapter

Verifierはcapabilityを明示します。

```yaml
verifier:
  id: semgrep@...
  supports:
    properties:
      - security.untrusted_input_flow
    languages:
      - python
      - javascript
  completeness:
    sound: false
    complete: false
  execution:
    network: false
    writes_repository: false
```

soundness/completenessが不明なtoolに強い保証を与えません。

## 10. LLM verification

別LLMに「本当に正しいか確認させる」ことは、reviewer cross-checkです。Evidence verificationとは区別します。

用途:

- source citationの誤読検査。
- claimの内部矛盾。
- alternative explanation。
- requested evidenceの提案。

制約:

- 同じsourceと同じmodel familyではfailure modeが相関する。
- LLM同士の一致はexecutable evidenceではない。
- `machine_checked`を付与しない。
- `cross_reviewed`等の別facetにする。

## 11. Evidence freshness

Evidenceは変更で失効します。

stale条件:

- subject source hash変更。
- dependency cone変更。
- test fixture変更。
- environment/config変更。
- tool version semantic change。
- policy version変更。
- human exception expiration。
- runtime evidence retention expiration。

stale EvidenceBindingは削除せず、current supportから除外します。

## 12. Contradictory evidence

矛盾を多数決で消しません。

```text
test evidence: issue reproduced
static analyzer: no path found
```

考えられる理由:

- analyzer incompleteness。
- test fixture artifact。
- different build config。
- stale evidence。
- path model mismatch。

Evidence conflict Obstructionを作り、resolution actionを要求します。

## 13. Derivation

複数evidenceからclaimを導く場合、推論過程をDerivationとして残します。

```yaml
derivation:
  premises:
    - event can re-enter
    - charge has external side effect
    - no idempotency guard on path
  rule:
    id: payment.duplicate-side-effect
  conclusion:
    claim_id: ...
  excluded_premises:
    - provider may deduplicate requests
  verification:
    status: partial
```

provenanceは「どこから」、derivationは「なぜ」を表します。

## 14. Human acceptance

ReviewDecisionに必要なもの:

- authority。
- decision scope。
- accepted claim/evidence IDs。
- known limitations。
- exception policy。
- timestamp。
- expiration。
- signatureまたはidentity assurance。

`human_reviewed`をsource code全体へ一括付与せず、obligationまたはdecision scopeへ付与します。

## 15. Evidence quality policy

例:

```toml
[policy.evidence]
critical = ["source_grounded", "reproducible", "human_accepted"]
high = ["source_grounded", "property_matched"]
medium = ["source_grounded"]
```

実際のpolicyはorganizationとprofileでversion管理します。

## 16. Evidence invariants

1. Evidenceはsnapshotとproducerへ追跡できる。
2. ClaimとEvidenceは別IDを持つ。
3. confidenceはEvidence kindではない。
4. stale evidenceはcurrent supportへ数えない。
5. verifierはprocedure versionとlimitationsを持つ。
6. LLM cross-checkをmachine proofと呼ばない。
7. negative claimはchecked scopeとboundを持つ。
8. contradictory evidenceはObstructionとして残す。
9. human decisionはscopeとauthorityを持つ。
10. projectionはevidenceのqualificationを落とさない。
