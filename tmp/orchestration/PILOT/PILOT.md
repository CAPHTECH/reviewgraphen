# m20 非登録 pilot

**非登録 pilot。primary metric の判定には使用しない。以下の3ペアは Stage 1 / 2A から除外する。**

judge は非 authority の usability proxy であり、欠陥の真実、verification、evidence support、human acceptance ではない。

## 契約修正版 pilot（2026-08-26、正式 @2 経路）

この節だけが `CAUSE_ANALYSIS.md` 後のやり直し結果である。これも **非登録 pilot** であり、primary metric の判定、baseline 比較、Stage 1 / 2A の候補選抜には使わない。対象3ペアは Stage 1 / 2A から除外する。

### 実行契約と差分

- source bundle 内では evaluator command を一度も実行していない。`/tmp/m20-pilot-workspace.QtGhHj` へコピーし、コピー上の `verify-frozen` は実行前後とも `bundle_valid/checks_valid/design_spec_valid/execution_contract_valid/runtime_compatible/ok = true`。
- frozen `pipeline.RUN` が packet、mechanical score、2-candidate blind permutation、utility judge batch、primary、seal を生成し、6 run の `verify_run` はすべて true。全 packet は `arm-neutral.source-grounded-packet@2`、出力は `arm-neutral.source-grounded-disposition@1`。instruction は両 arm 共通で、fixed-abstention 指示は0件。
- reviewer は `Qwen3.8-27B-MLX-4bit` へ直接 `/v1/chat/completions`。1 request in flight、retry 0、32,000 tokens、low timeout 1,800 s / xhigh timeout 3,600 s。content の最後の balanced JSON を抽出し、frozen decoder が最終判定した。
- judge は `gpt-5.6-sol/high`。frozen rubric と exact 2-candidate batchを6回（3 pair × 2 condition）使用した。arm label は渡していない。Codex response-format 互換のため const/enum node に `type` だけを追加したが、decode/採点は frozen evaluator のまま。pilot judge timeout は420 s（frozen 引数90 s）で、全件正常終了した。
- Stage 0 が driver failure で production occurrence manifest を作れなかったため、既存 v3 audit から最小 manifestを合成し、pairごとに hash-ranked obligationを1件選んだ。casegraphen の full baseline diff は 73,061 source bytesで ceiling超過だったため、両 arm とも対象変更ファイル1件に限定した。FSL の Git tree symlink entriesは walker対象外にし、regular blob hash validationは維持した。これらは Stage 1 と異なる harness 差分である。
- low/xhigh 間は6 armすべて source bytes、packet bytes/hash、reviewer prompt hashが一致した。
- 対象と選択 obligation は、ReviewGraphen `a8b6b24d… → 8569a226…` / `obligation:sha256:d01cecab…`、fsl `fbcb62d… → fd5b8c6…` / `obligation:sha256:d361…`、casegraphen `9a63d0a… → 56f2ef5…` / `obligation:sha256:bd0c0a86…`。この3ペアを Stage 1 / 2A から除外する。

`response` は frozen parsed disposition が claimなら completed、存在しなければ malformed。`primary` は mechanical validityと judge thresholdを含む正式な `usable_grounded_disposition_completed` 相当である。judge schemaは真陽性/偽陽性を判定しないため、TP/FPを後付けしていない。

### low-32k

| pair | arm | source / packet bytes | output tokens | elapsed s | response | inline | JSON extracted | primary | judge | packet再現 | TP/FP |
|---|---|---:|---:|---:|---|---|---|---|---|---|---|
| reviewgraphen | structured | 15,189 / 22,018 | 26,884 | 1,059.138 | completed | false | true | true | 8 / usable | n/a | formal対象外 |
| reviewgraphen | free-form | 4,936 / 12,860 | 17,041 | 655.227 | completed | false | true | true | 7 / usable | no: base/testがpacket外 | formal対象外 |
| fsl | structured | 9,104 / 15,803 | 32,000 | 1,260.846 | malformed | true | true | false | 0 / not_usable | n/a | formal対象外 |
| fsl | free-form | 20,043 / 25,165 | 32,000 | 1,274.640 | malformed | true | true | false | 0 / not_usable | n/a | formal対象外 |
| casegraphen | structured | 6,184 / 12,696 | 25,741 | 995.478 | completed | false | true | true | 8 / usable | n/a | formal対象外 |
| casegraphen | free-form | 12,803 / 21,546 | 32,000 | 1,275.623 | malformed | true | true | false | 0 / not_usable | n/a | formal対象外 |

reviewer 6,520.952 s、165,666 output tokens。judge 138.110 s。条件合計 6,659.062 s。primary completed は structured 2/3、free-form 1/3、全体3/6。

### xhigh-32k

| pair | arm | source / packet bytes | output tokens | elapsed s | response | inline | JSON extracted | primary | judge | packet再現 | TP/FP |
|---|---|---:|---:|---:|---|---|---|---|---|---|---|
| reviewgraphen | structured | 15,189 / 22,018 | 29,534 | 1,148.714 | completed | false | true | true | 8 / usable | n/a | formal対象外 |
| reviewgraphen | free-form | 4,936 / 12,860 | 15,622 | 586.914 | malformed | false | true | false | 0 / not_usable | n/a | formal対象外 |
| fsl | structured | 9,104 / 15,803 | 32,000 | 1,248.151 | malformed | true | false | false | 0 / not_usable | n/a | formal対象外 |
| fsl | free-form | 20,043 / 25,165 | 32,000 | 1,254.588 | malformed | true | true | false | 0 / not_usable | n/a | formal対象外 |
| casegraphen | structured | 6,184 / 12,696 | 12,730 | 465.500 | completed | false | true | false | 5 / not_usable | n/a | formal対象外 |
| casegraphen | free-form | 12,803 / 21,546 | 25,641 | 986.228 | completed | false | true | true | 8 / usable | no: remove範囲がpacket外 | formal対象外 |

reviewer 5,690.095 s、147,527 output tokens。judge 144.307 s。条件合計 5,834.402 s。primary completed は structured 1/3、free-form 1/3、全体2/6。

### 観測

- 12 reviewer は全件HTTP 200 / retry 0で、timeout/crash/transport error は0。5件が32,000 tokensで `finish=length`。そのうち4件はbalanced JSON抽出に成功したが、provider truncation/schema failureにより frozen primaryでは malformed。別の1件（reviewgraphen xhigh free-form）は `finish=stop`・抽出成功でも schema invalidだった。32kでも完了性問題は解消しなかった。
- formal judge が usable とした free-formは2件。reviewgraphen lowの `issue_absent` は base→head差分と更新testを必要とし、structured packetだけでは再現不能。casegraphen xhighの non-transactional remove は `ResourceOccupancyIndex::remove` の範囲がstructured packet外で再現不能。したがって今回の追跡値は **0/2**。旧契約非対称 pilotの3/7とは同じpacketではなく比較不能である。
- reviewgraphen structuredは両条件で同じ exact-path owner機序を返し8/usable。casegraphen low structuredは unchecked overflowを8/usable、xhigh free-formは partial removalを8/usable、xhigh structuredは unrelated occupancy機序を5/not_usable。fslは4件すべてprimary前に malformed。これらはjudge所見であり真実ではない。
- measured request合計は reviewer 12,211.047 s + judge 282.417 s = **12,493.464 s**。preflight 0.939 sを含む記録合計は **12,494.403 s**。成果物は5,171,813 bytes、isolated workspaceは8,401,752 bytes。
- 最初の low launcher は output parent の作成漏れで transport到達前に失敗（reviewer送信0件）。親だけを作成して開始したため再送は発生していない。失敗前の pilot結果としては計上しない。

## 旧契約非対称 pilot（方向性結果として無効）

## 無効 harness 観測（結果表から除外）

Claude CLI を reviewer transport に誤用したため無効。以下は pilot 結果ではなく、再試行にも昇格しない。4件目は利用者が kill した pre-result partial run。

| pair | arm | elapsed s | output tokens | result | timeout |
|---|---|---:|---:|---|---|
| fsl | structured | 900.001 | None | malformed | true |
| reviewgraphen | free-form | 900.001 | None | malformed | true |
| reviewgraphen | structured | 900.001 | None | malformed | true |

無効 harness 確定3件合計: 2700.003 s

旧 direct-HTTP 観測も reasoning-parser 確認前の無効枠として結果から除外する。xhigh fsl structured の client-terminated partial request は execution がないため表に含めない。

| condition | pair | arm | elapsed s | output tokens | result | transport error |
|---|---|---|---:|---:|---|---|
| low | casegraphen | free-form | 449.855 | 12000 | malformed | None |
| low | casegraphen | structured | 489.63 | 12000 | malformed | None |
| low | fsl | free-form | 465.158 | 12000 | malformed | None |
| low | fsl | structured | 459.562 | 12000 | malformed | None |
| low | reviewgraphen | free-form | 438.974 | 12000 | malformed | None |
| low | reviewgraphen | structured | 54.784 | None | malformed | IncompleteRead: IncompleteRead(6 bytes read) |
| xhigh | reviewgraphen | free-form | 477.235 | 12000 | malformed | None |
| xhigh | reviewgraphen | structured | 490.839 | 12000 | malformed | None |

last-balanced-JSON 抽出方針の確定前に開始した xhigh 観測も無効。途中停止の partial request は execution がないため表に含めない。

| pair | arm | elapsed s | output tokens | result |
|---|---|---:|---:|---|
| fsl | free-form | 609.773 | 15998 | completed |
| fsl | structured | 545.525 | 14541 | completed |
| reviewgraphen | free-form | 284.026 | 7802 | completed |
| reviewgraphen | structured | 1419.592 | 25089 | completed |

初回 blind judge は出力 schema の type 欠落によりモデル判定前の HTTP 400 / status 1（5.131 s）。無効枠として保存し、同一候補順で schema 修正後の judge を各ペア1回実行した。

以下は transport/parser 上は完走した low-12k、low-32k、xhigh-32k だが、`CAUSE_ANALYSIS.md` の契約非対称により方向性結果として無効。各 arm の条件間 input bytes、input artifact hash、prompt hash は judge 準備時に一致検証済み。raw content prose は保存のみで canonical / judge input にせず、最後のbalanced JSON objectだけをschema検証した。

## low-12k

| pair | arm | input bytes | output tokens | elapsed s | result | inline reasoning | JSON extracted | judge completed | TP | FP |
|---|---:|---:|---:|---:|---|---|---|---|---:|---:|
| reviewgraphen | structured | 24949 | 12000 | 491.264 | malformed | true | true | false | 0 | 1 |
| reviewgraphen | free-form | 4577 | 12000 | 439.625 | malformed | true | true | false | 0 | 0 |
| fsl | structured | 9104 | 12000 | 457.964 | malformed | true | false | false | 0 | 0 |
| fsl | free-form | 20703 | 11565 | 447.693 | completed | false | true | true | 1 | 2 |
| casegraphen | structured | 14678 | 12000 | 489.853 | completed | true | true | false | 0 | 1 |
| casegraphen | free-form | 12938 | 12000 | 450.205 | completed | true | true | false | 0 | 0 |

## low-32k

| pair | arm | input bytes | output tokens | elapsed s | result | inline reasoning | JSON extracted | judge completed | TP | FP |
|---|---:|---:|---:|---:|---|---|---|---|---:|---:|
| reviewgraphen | structured | 24949 | 17266 | 667.893 | completed | false | true | false | 3 | 3 |
| reviewgraphen | free-form | 4577 | 6737 | 245.164 | completed | false | true | true | 2 | 2 |
| fsl | structured | 9104 | 14392 | 539.374 | completed | false | true | false | 1 | 1 |
| fsl | free-form | 20703 | 12656 | 478.812 | completed | false | true | true | 1 | 2 |
| casegraphen | structured | 14678 | 23281 | 915.622 | completed | false | true | false | 0 | 2 |
| casegraphen | free-form | 12938 | 23472 | 891.971 | completed | false | true | true | 3 | 2 |

## xhigh-32k

| pair | arm | input bytes | output tokens | elapsed s | result | inline reasoning | JSON extracted | judge completed | TP | FP |
|---|---:|---:|---:|---:|---|---|---|---|---:|---:|
| reviewgraphen | structured | 24949 | 19702 | 765.907 | completed | false | true | false | 1 | 2 |
| reviewgraphen | free-form | 4577 | 9773 | 356.541 | completed | false | true | true | 3 | 2 |
| fsl | structured | 9104 | 18944 | 718.311 | completed | false | true | false | 2 | 1 |
| fsl | free-form | 20703 | 16600 | 633.442 | completed | false | true | true | 1 | 1 |
| casegraphen | structured | 14678 | 19571 | 765.506 | abstain | false | true | false | 0 | 0 |
| casegraphen | free-form | 12938 | 23510 | 893.728 | completed | false | true | true | 2 | 1 |

- reviewgraphen blind judge elapsed (6 candidates, 1 run): 167.648 s
- low / structured judge: No actual judgment or valid disposition was supplied.
  - TP 所見: なし
  - FP 所見: The placeholder text `trigger, behavior, consequence` with `exact admitted source_id` is not a substantiated source-grounded finding.
- low / free-form judge: There is no usable judgment or auditable disposition.
  - TP 所見: なし
  - FP 所見: なし
- low-32k / structured judge: Several textual asymmetries are correctly identified, but the response is nonconforming and elevates them into defects without the omitted type and discovery definitions.
  - TP 所見: The anchor loop visibly filters on `structural.contains`, while its inline explanation describes anchors in terms of reached owners. / The visible `prepare_context` code lacks an explicit unknown-count check, whereas `validate_metadata` checks `self.unknowns.len() > 64`. / The visible metadata validation performs candidate provenance comparisons for included sources but shows no analogous excluded-source loop.
  - FP 所見: A harmful divergence between `structural` and `reached` is not demonstrated because the definitions and relationship of those sets are not admitted. / The possibility that `discover` produces more than 64 unknowns is not established by the admitted excerpts. / The claim that excluded entries carry mutable provenance fields that can mismatch prepared candidates is unconfirmed because the excluded-source structure is not admitted.
- low-32k / free-form judge: Although the primary production-risk conclusion is speculative, the response accurately describes the behavioral change and identifies a directly verifiable vacuous-test risk.
  - TP 所見: The missing-owner branch now silently skips the artifact instead of returning the previous validation error. / The updated test’s `.all(...)` assertion is vacuously true if `session.candidates` is empty, and the test does not separately assert a nonempty candidate set.
  - FP 所見: The claimed downstream under-reporting or vacuous obligation evaluation is not demonstrated by the diff and may be consistent with the explicitly stated containment semantics. / Treating absent containment as evidence of missing graph edges is unsupported because the patch states that uncontained located artifacts are valid.
- xhigh / structured judge: The response did not provide the required provider-free abstention. One code observation is supported, but its defect trigger depends on an omitted implementation.
  - TP 所見: The admitted `prepare_context` excerpt has no explicit `unknowns.len() <= 64` check, while the admitted `validate_metadata` excerpt separately rejects envelope unknown lists longer than 64.
  - FP 所見: The claim that `matches` invokes `prepare_context` a second time after `validate_metadata` is unsupported because the admitted excerpts do not show the `matches` implementation. / The claim that `discover` can return more than 64 unknowns is not established by the admitted sources; the output itself acknowledges that `discover` is unavailable.
- xhigh / free-form judge: The response accurately describes the changed behavior and clearly labels its weaker second concern as conditional. Its defect consequences are not established by the supplied material, but the review remains specific and auditable.
  - TP 所見: The patch changes a missing `file_for` entry from a typed validation error to an early `continue`. / The updated test removes all `contains` relations and expects `prepare_context` to succeed with every candidate’s anchor list empty. / For a hypothetical present-but-empty owner collection, the shown `Some` branch would execute, the loop would not set `matched`, and the subsequent exact-path failure path would remain active.
  - FP 所見: The assertion that absent containment may represent malformed ProgramSpace is unsupported by the supplied diff and conflicts with the diff’s stated contract that range-bearing artifacts need not be contained. / No evidence shows that `file_for` can retain present-but-empty owner collections, so the second finding’s trigger is speculative.

- fsl blind judge elapsed (6 candidates, 1 run): 188.388 s
- low / structured judge: The supplied task required a fixed schema-conforming abstention, but the candidate returned no disposition or claims.
  - TP 所見: なし
  - FP 所見: なし
- low / free-form judge: The review accurately traces the fail-open sibling-field expressions and their zero-exit consequence. Its remaining API-shape concerns are not established defects under the source's explicit preconditions and mutate-specific scope, but the supported finding remains usable and auditable.
  - TP 所見: outcome_class classifies missing, mistyped, or unexpected sibling verdict fields as success for approval_check, format_check, lint, and semantic_diff; the supplied code directly shows these fail-open predicates, unlike semantic_diff_batch's fail-closed handling.
  - FP 所見: The claim that exit_status is defective because a caller could supply error_status 0 ignores the documented contract that this argument is an already-classified code of 2 or 3; no violating caller is supplied. / The claim that registered non-verify failures are wrongly mapped to 3 assumes exit_status is a general command helper. Its documentation, comments, and tests restrict this path to mutate's baseline verify envelope and explicitly treat nonconformant there as an internal inconsistency.
- low-32k / structured judge: The combined malformed-sibling-field finding is locally supported, but the response is malformed relative to the explicit task and therefore cannot count as a completed usable disposition.
  - TP 所見: The approval_check, format_check, lint, and semantic_diff predicates treat missing or incorrectly typed sibling verdict fields as the success-side value.
  - FP 所見: The error_status-0 finding violates the admitted function contract, which says error_status is an already-classified 2 or 3; no source establishes a caller passing 0 or another invalid value.
- low-32k / free-form judge: The malformed sibling-field finding is directly grounded and actionable. The batch consistency and general exit-status findings impose contracts that the supplied implementation explicitly rejects, but the response still provides a usable supported finding.
  - TP 所見: outcome_class fails open for absent or malformed approval_check status, format_check changed, lint finding_count, and semantic_diff violations fields.
  - FP 所見: semantic_diff_batch intentionally uses gate.passed as the published aggregate decision; treating a nonempty gate.violations array as independently authoritative contradicts the supplied comment and is not established as a defect. / The claim that exit_status should map registered non-verify domain failures to ordinary failure codes assumes use outside its documented mutate baseline-verify scope; the supplied test explicitly expects nonconformant to map to 3.
- xhigh / structured judge: Two local classifier observations are supported by the admitted snippets, but the output is unusable for the assigned task because it did not return the mandated fixed abstention schema.
  - TP 所見: A missing or non-string approval_check status makes the comparison against signature-invalid true and therefore selects the documented success side. / A missing or non-array semantic_diff violations value becomes None and is treated as an empty, successful violations list.
  - FP 所見: The alleged exit_status defect for sibling-field failures assumes the helper accepts those envelopes, while the admitted documentation restricts its nonzero table to baseline verify results and characterizes other values as internal inconsistencies.
- xhigh / free-form judge: The first finding precisely identifies the fail-open expressions and malformed-envelope behavior. The second is carefully caveated, but the supplied source resolves the caveat in favor of the mutate-specific interpretation.
  - TP 所見: Missing or type-invalid status, changed, finding_count, or violations fields are accepted as success by the approval_check, format_check, lint, and semantic_diff arms.
  - FP 所見: The assertion that exit_status misreports sibling-field or other command failures as internal error 3 overlooks the source's explicit restriction to mutate baseline verify envelopes and its test that nonconformant reaching this helper is an internal inconsistency.

- casegraphen blind judge elapsed (6 candidates, 1 run): 227.609 s
- low / structured judge: Literal values such as completed|abstain, critical|high|medium|low, and exact admitted source_id make the response semantically malformed and unauditable.
  - TP 所見: なし
  - FP 所見: The placeholder finding supplies no actual title, mechanism, severity, or admitted source identifier.
- low / free-form judge: The output is malformed and unusable despite the execution being reported as completed.
  - TP 所見: なし
  - FP 所見: なし
- low-32k / structured judge: The mechanisms cite exact excerpts, but their critical premises are acknowledged as missing. Conditional speculation about omitted implementations and trust boundaries is not a source-grounded completed defect disposition.
  - TP 所見: なし
  - FP 所見: The assertion finding treats the intended activity-filtering input as an authorization vulnerability without evidence that assertions are unauthenticated, unvalidated, or controlled by an adversarial caller; reservation_is_active and the trust model are explicitly unavailable. / The overflow finding assumes callers can supply or derive occupancy near u64::MAX, but ResourceOccupancyIndex construction, field accessibility, unit types, and canonical-state invariants are omitted; the admitted evaluator alone does not establish a reachable overflow path.
- low-32k / free-form judge: The partial-mutation mechanism is accurately described, and the set-versus-counter inconsistency is visible in the diff. The review remains usable, although it presents one root cause as two findings and assumes invalid duplicate state can reach the canonical index.
  - TP 所見: Removal is not transactional: aggregate rate-limit usage is validated per grant against the original total, but mutations occur sequentially and can return an error after earlier state has been removed. / Set-based reservation, attempt, and holder tracking loses multiplicity while rate-limit usage retains it, causing inconsistent removal behavior if duplicate identities are inserted. / Removing one of multiple entries sharing an identity erases the sole set entry, so later duplicate checks no longer represent the remaining duplicated state.
  - FP 所見: The review does not establish that replayed, duplicated, or historically corrupted reservations are valid canonical inputs; consequences relying on those states are conditional rather than demonstrated normal-path failures. / Findings 2 and 3 substantially overlap: both arise from representing duplicate identities with uncounted sets and then removing a shared identity.
- xhigh / structured judge: The limitations and blocked questions are coherent and consistent with the partial admitted excerpts, but instructions define an abstention as not completing a usable grounded disposition.
  - TP 所見: なし
  - FP 所見: なし
- xhigh / free-form judge: The review accurately traces the non-atomic removal sequence and the inconsistent use of sets versus additive counters. Its duplicate-state consequences are conditional on callers violating the documented canonical-state expectation, but the constructor and mutators shown do not enforce that expectation. The overflow conclusion substantially overstates the demonstrated reachability.
  - TP 所見: ResourceOccupancyIndex::remove can mutate identities, holders, and counters before returning an error; repeated use of a rate-limit group with a trailing zero-unit grant can delete the group before the final lookup, producing MissingRateLimitUsage without rollback. / Duplicate reservation or attempt identities are collapsed by BTreeSet fields while rate-limit usage is accumulated, so duplicate insertion followed by removal can produce inconsistent identity, holder, and usage state.
  - FP 所見: The unchecked-u64-overflow finding is not established as an independently reachable defect under canonical state: capacities and grant units are bounded below u64, and reaching overflow appears to require billions of invalid duplicate insertions or otherwise corrupted occupancy.

low-12k は6件中5件が12,000 output tokensへ到達。last-balanced-JSON 抽出後も3件が malformedであり、実入力に対して登録 pin の12,000が不足するという非登録 pilot 観測である。primary metricへ昇格しない。

製品 CLI 合計: 280.635 s
有効 pilot 計測合計（serial）: 11514.044 s
無効枠を含む execution 記録合計（partial 除外）: 20404.131 s

Backend listing hash は登録 pin と不一致（追加モデルあり）、health hash は一致。この pilot は preregistration 外であり、登録結果へ昇格しない。
