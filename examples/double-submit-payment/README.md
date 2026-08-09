# Reference Scenario: Double Submit Payment

このfixtureは、ReviewGraphenがAST nodeの個別レビューを超えて、Relation、Path、Invariant、Context Gluingを必要とすることを示す最小シナリオです。

## 1. Scenario

購入ボタンをほぼ同時に二回押すと、二つの処理が`loading` guardを通過します。guardはvalidation boundaryの後に書き込まれるためです。その後、PaymentRepositoryはidempotency keyなしで外部chargeへ到達します。

```text
Buy tap #1 ─┐
             ├─> CheckoutController::submit
Buy tap #2 ─┘       │
                     ├─ check loading == false
                     ├─ validation boundary / yield
                     ├─ set loading = true     ← 遅い
                     └─ PaymentRepository::charge
                              │
                              └─ StripeClient::charge
                                   no idempotency key
```

## 2. Why node-only review is insufficient

各要素を単独に見ると、次のような局所判断になり得ます。

- Controllerにはloading guardがある。
- Repositoryは単純にgatewayへ委譲する。
- StripeClientは一回の呼び出しにつき一回chargeする。

しかし欠陥は次の組み合わせで成立します。

1. UI eventが繰り返し可能。
2. guardのcheckとwriteの間にyieldがある。
3. Controllerから外部side effectへのpathがある。
4. payment boundaryにidempotency contractがない。
5. `at most once` invariantが大域的に要求される。

## 3. Files

| File | Purpose |
| --- | --- |
| `fixture/` | 標準libraryだけで動く意図的に脆弱なRust fixture。 |
| `program-space.json` | accepted ProgramSpace input。 |
| `review-plan.json` | risk-first実行順。 |
| `review-report.json` | claim、evidence、verification、finding、gluing、coverageの完成例。 |

同じJSONは`../../schemas/`にもschema exampleとして置いています。

## 4. Run the reproduction fixture

```bash
cd examples/double-submit-payment/fixture
cargo test
```

testは「安全性を満たした」ことではなく、**同じorderに対して二回external chargeが観測される反例を再現した**ことをassertします。したがってbugが存在するfixtureでtestはpassします。

## 5. Obligation universe

このfixtureは、五つの具体的なreview obligationと、部分的な
`concurrency_model` capabilityを明示する四つのcapability-gap obligation、計九つの
obligationを生成します。後者は分母から除外せず、未解決として残ります。

| Obligation | Kind | Property |
| --- | --- | --- |
| Guard order | Node | `async.concurrent_reentry` |
| Tap → submit | Relation | `async.concurrent_reentry` |
| Repository → gateway | Relation | `payment.idempotency_contract` |
| Tap → external charge | Path | `payment.at_most_once` |
| Payment requirement | Invariant | `payment.at_most_once` |

各obligationの完全なrecordは`review-plan.json`ではなく、`../../schemas/reviewgraphen.obligation.example.json`にあります。

## 6. Evidence interpretation

Counterexample testは次を支持します。

- duplicate eventがsubmitへconcurrent re-entryする。
- external charge pathが二回実行される。
- `payment.at_most_once` invariantが反証される。

一方、testだけでは次を証明しません。

- あらゆるschedulerで必ず二重課金する。
- production gatewayの全挙動。
- fix candidateが正しい。
- 他のpayment pathが安全。

この限定はclaimとverificationの`limitations`へ残します。

## 7. Gluing failure

二つのlocal contextは、重複防止責任を互いへ委譲しています。

```text
UI context:
  「loading guardがduplicate submissionを防ぐ」

Payment context:
  「callerがduplicate charge attemptを防ぐ」
```

overlap上では、UI guardは実効的でなく、payment boundaryにもidempotencyがありません。したがってlocal reviewを単純集約せず、`obstruction:ui-payment-assumption-conflict`を生成します。

## 8. Expected gate

```text
report status: partial
CI gate: blocked
critical finding: duplicate charge
blocking obstruction: cross-context duplicate-protection gap
fresh verified coverage: 5 / 9 for this bounded universe
```

5/9はこのfixtureとrule setに対するcoverageです。残る4件は
`concurrency_model`の部分性に由来するcapability-gap obligationであり、passや
verifiedには変換されません。アプリ全体の安全性や無制限concurrencyの形式証明ではありません。`limitation:bounded-concurrency`がその境界を宣言します。

## 9. Candidate repairs

reportは二つのCompletionCandidateを持ちます。

- guardを最初のyieldより前に原子的に獲得する。
- payment boundaryへidempotency keyまたはserver-side deduplicationを追加する。

候補は自動適用・acceptedにはなりません。修正後は新snapshotを作り、obligation、verification、gluing、fresh coverageを再計算します。
