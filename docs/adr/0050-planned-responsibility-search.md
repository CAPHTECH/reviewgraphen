# ADR 0050: planned responsibility contractから既存実装候補を検索する

- Status: Accepted for benchmark-only trial
- Date: 2026-09-14

## Context

実装に入る前に「この責務は既にどこかに実装されていないか」を調べたい。しかし、完全一致または近似した関数本体だけを起点にすると、構造が異なる同一責務を落とす。一方、自然言語の実装予定をそのままLLMへ渡すと、検索分母、snapshot、抽出器、未検証の契約条項が残らず、候補を既存実装だと誤認しやすい。

ReviewGraphenの責務ファミリー試験には、acceptedなRust構文factから callable / signature / operation signalを抽出する経路がある。このfactを、予定責務を構造化した契約の候補検索にも利用する。

## Decision

benchmark-only CLIへ次を追加する。

```text
reviewgraphen-responsibility-family search-responsibility \
  --program-space FILE \
  --contract FILE \
  --output FRESH_FILE
```

入力は `reviewgraphen.benchmark.planned_responsibility_contract.v1` とし、次をsnapshotへ束縛する。

- 安定したcontract ID
- precondition、postcondition、invariant、typed error、compatibility、performance、purpose constraintの有限な条項集合
- callable、signature、operationの検索signal
- signal extractor version
- 未知事項

候補選択はaccepted Rust function/methodだけを分母にし、profile/test-scope/signal factの除外数を残す。query termごとのdocument frequencyを計算し、選択性のあるsubject signalを1件以上、operation signalを2件以上、全operation queryに対するcoverageを600,000 ppm以上持つ候補を、top-Nで切らず決定論的に列挙する。完全一致、近似一致、異なる構造を区別して除外しない。

search IDはsnapshot、canonical contract hash、rule、signal extractorに加え、ProgramSpaceのprofile ID/version、rule-set hash、extractor-set hash、policy versionへ束縛する。候補IDはsearch IDとartifactへ束縛し、表示順位へは束縛しない。出力は同じProgramSpace basis、検索分母、query termのselective/absent/high-frequency分類、projection loss、unknownを含む。

契約条項の自然言語本文は候補matchingへ使わない。本文はcanonical contract hashで保持し、全candidateに全 `unverified_clause_ids` を付ける。検証分母は `candidate_count * clause_count` とする。したがって出力は候補であり、責務ファミリー登録、既存実装の存在証明、契約充足、抽象化判断、Evidence、Verification、accepted stateではない。

このcommandはproduction CLI/Core/Store/Ingestを変更しない。

## Consequences

- 実装予定を先に構造化すると、同じ記述でない既存関数もsyntax signalから候補化できる。
- absentまたは高頻度のquery termも分母から消えないため、弱い一致を完全一致として扱わない。
- semantic synonym、動的dispatch、外部crate、実行時の振る舞いは未解決のままである。
- 候補が出ても、同じ責務・同じ変更理由か、wrapper抽象化・共有conformance test・意図的分離のどれが妥当かは別のdecision obligationで確認する。
- v1の効用はcandidate recallと有限な未検証分母として評価し、通常レビューとのLLM競争には広げない。

## Validation

acceptance fixtureは次を検査する。

- 同一shapeと異なるshapeの両方を候補へ含める。
- snapshot/contract変更でIDが変わり、rank変更ではcandidate IDが変わらない。
- df=1、absent、高頻度、600,000 ppm境界を正しく扱う。
- 全candidateへ全未検証条項を保持する。
- malformed/semantic mismatch/overwriteをtyped exitでfail closedし、partial outputを残さない。
- 同一入力のcanonical output bytesが一致する。
