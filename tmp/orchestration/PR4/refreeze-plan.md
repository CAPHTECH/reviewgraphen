# m20 Wave-5 atomic re-freeze plan（seal 前）

Status: **準備中 / seal 禁止**。旧 manifest byte SHA-256
`95b75a62353b56111fe8913700c72bf9881f93efdc99629e72adb3e2c6a9fa73`
は ADR 0038 §5.4.3 の案 C より前の acceptance を表すため失効済みであり、active freeze として表示・検証・Stage 0 入力に使わない。

## 1. 固定する研究契約

- Primary endpoint は `usable_grounded_disposition_completed(commit, arm)` のまま。`m20.primary_score.v3.completed`、failure=`0`、judge reconciliation、authority ceilingを変えない。
- 分母を変えない。analysis unit は base→head commit cluster、Stage 0 は `C=300` と exact `A_c/A`, `S_c/S`, `D_c/D`、model result は固定 hash sample の model-eligible commit だけから `n/b/c/n00` を作る。obligation、relation、claim、finding を独立 trial にしない。
- `rust.production.v1`、`relation.changed_public_callee@1`、`rust.callee_contract_review@1`、repository/sample seeds、7 gates、65,536 admitted-source-byte ceiling、failure/stop/control rulesを変えない。
- Operational rectangle は Stage 1 `b>=8,c<=1`、Stage 2A cumulative `b>=18,c<=7` のまま。
- 変更可能面は、既に採択済みの occurrence-summary **record shape**、active `context.subject_windows@3` **policy reference**、および今回の **semantic acceptance algorithm/reference** に限る。今回の Wave-5 delta は最後の一つだけであり、context-v3 DTO/hash、run-v3 schema、canonical context/run bytes、reviewer-visible packet、baseline algorithm/task-ID preimageを変えない。

## 2. ADR が要求する再凍結条件

1. Stage 0、model result、arm outcomeを未観測のまま行う。部分 freeze、occurrence-only freeze、pre-oracle manifest の再利用は禁止。
2. Runtime の run-scoped immutable basis を `Arc` 共有し、obligation view は semantic validation 直後に破棄する。validated run が ProgramSpace/source bytes/basis/per-obligation deep clone を保持しないことをテストで示す。
3. Basis は immutable snapshot の accepted ProgramSpace、source-registration closure/index、relation adjacency、request `ingest.max_files`、snapshot/profile/rule/extractor/obligation/relation/endpoints、policy bytes/hash に束縛する。caller-supplied set/count/digest は basis にしない。
4. 案 C を executable acceptance にする。単純 oracle は production の `prepare_subject_windows_v3`、reached/anchor enumeration helper、subject-first/materialization indexを呼ばず、accepted/reached/support-anchor exact ID sets と count/digest を全 adjacency/containment 走査から再構築する。
5. Oracle sets と production builder sets を先に一致させ、その後だけ production を再実行して materialized commitment、latent state、subject outcomes、windows、admitted/lost-anchor partition、losses、projection/context IDs、canonical bytesを照合する。不一致は typed semantic failure。
6. `literate_access.rs` support-anchor omission、reached-file omission、別 support-anchor omission（および accepted-set omission）を、他の subject/source probes が通る状態でも oracle が拒否する。wire validator が semantic completeness を主張しない境界は維持する。
7. Four real-pair fixtures は current builder から捕捉した期待値ではなく、checked-in Git/source witnesses から独立作成した literal oracle sets/digests と比較する。live basis と sealed-replay basis の双方を通し、wrong binding 群を全拒否する。
8. Oracle operation counts と elapsed time を seal 前に記録する（§4）。計測値は診断であり canonical artifact、gate threshold、endpoint、denominatorには入れない。
9. 下表の spec/check/vector/mutant/preregistration/manifest を同一候補 revision に揃え、既存全 acceptance と新 oracle acceptance を通した後にのみ一回の atomic manifest を生成する。
10. CPython 3.13.5 / Unicode 15.1.0 で generated fixtures、全 vectors、全 named attacks、freeze round-trip、`verify-frozen` を通す。新しい全 hash を同時転記し、old hash を superseded provenance としてのみ残す。

## 3. `95b75…fa73` からの差分計画

| 面 | 必要な差分 | 不変確認 |
|---|---|---|
| `EVALUATOR_SPEC.md` | §1.4/10/11/12 に案 C の二段 acceptance、basis authority、oracle/helper 非共有、typed mismatch、実測、new vectors/mutants、old freeze superseded を規定する。 | §8–9 endpoint/scorer、budgets、rectanglesは変更しない。 |
| Executable checks (`stage0_contract.py`, pipeline/freeze gates, tests) | wire check と basis-bound semantic check を分離し、oracle→builder set equality→production replay→canonical byte equality の順を強制する。runtime acceptance receipt/revision/hashを authenticated reference として freeze gate で照合する。 | context projection DTO、packet/source inventory、primary score bytesは変えない。 |
| Reference vectors | 現行 original 52 と atomic 16 を byte/order 不変で残し、accepted/reached/support-anchor oracle literal、oracle-builder mismatch、live/replay basis、wrong-binding rejectionを追加する。件数・set hash・generated vector hashを更新する。 | 既存 expected values を再生成関数から作らない。 |
| Mutation tests / attacks | 新しい named applied mutants（reached omission、`literate_access.rs` anchor omission、別 anchor/accepted omission、oracle bypass/helper reuse）を追加し、real acceptance path で kill する。attack manifest と hash を更新する。 | pass 数や manifest row 数を acceptance evidence にしない。 |
| Generated fixtures | real pipeline fixture に semantic receipt/reference を含め、byte-identical regeneration と hostile/tampered receipt rejectionを追加する。inventory hashを更新する。 | reviewer/judge raw result 以外を scoring authority にしない。 |
| `preregistration.json` | status を `refreeze_pending_wave5_oracle` にして旧5 hashを非active化し、案 C algorithm reference、measurement provenance、vector/mutant totalsを記述する。全 checks 後だけ新5 hash/pathと frozen status を同時確定する。 | endpoint、exact sets、7 gates、Stage 1/2A rectangles、profile/rule/property/seedsを byte-equivalent に保つ。 |
| `freeze-manifest.json` | evaluator/spec/generated artifactsに加え、semantic acceptance algorithm/runtime revision・oracle test/measurement receiptへの content-addressed referenceを束縛し、全 bundle/execution/artifact hashを再計算する。 | context-v3/profile DTO hashesは同じ literal を再確認する。 |
| README / PROTOCOL | “freeze complete / 52 / context@2” 等の stale 文言を Wave-5 pending→sealed status、new vector count、active @3、old manifest supersededへ整合させる。 | prose に executable algorithm を複製しない。 |

## 4. seal 前の実測ゲート

Runtime の Arc 接続完了後、同一候補 revision・同一 pinned inputs で次を取得する。

1. 既存の4実ペア（FSL `2be279ed…→ec0a40a9…`, `fbcb62df…→fd5b8c68…`; CaseGraphen `947f347f…→095f1fbf…`, `9a63d0ad…→56f2ef5d…`）について、literal oracle の accepted/reached/support-anchor sets・counts・digestsが builder と一致し、window/loss/partition/canonical bytes再実行も一致すること。2 clean runs で canonical result と deterministic operation counts が一致すること。
2. 4,816-file positive pair `a8b6b24d…→8569a226…` について、accepted=4,816、reached=1、subject/reached source access=1、および exact support-anchor oracle/builder equalityを確認する。oracle operation counts、elapsed、peak/RSS、basis clone/drop/lifecycle traceを記録する。
3. Stage 0 representative cluster 1件を clean output/cache で2回実行し、oracle operation counts と canonical commitments/bytes の一致、elapsed milliseconds（各回と測定環境）を記録する。これは Stage 0 gate/resultの観測ではなく pre-seal deterministic/operational rehearsal と明記する。
4. Full 300-cluster の wall/operation metrics は Stage 0 実行時の診断として全 cluster に残すが、pre-seal proxyから外挿して gate、deadline、sample、denominatorを変更しない。

## 5. seal 可否チェックリスト

- [ ] sollow-runtime の Arc sharing / immediate drop / no retained basis が完了し、core/runtime/report/CLI の型境界テストが緑。
- [ ] 4 real pairs + 4,816-file pair + representative cluster の上記実測 receipt が揃う。
- [ ] New oracle vectors と omission mutants が current では通り、mutated production では失敗する。
- [ ] Existing 68 vectors・全既存 attacks・generated full-run fixturesに退行なし。
- [ ] endpoint/denominator/rectangles/context-v3/profile hashes/canonical packet bytesの不変差分チェックが緑。
- [ ] preregistration と manifest の全 hash が同一 candidate revision を指し、compatible runtime の `verify-frozen` が全項目 true。
- [ ] Stage 0 / model / corpus outcomeを一切読まず、protocol custodian が一回だけ atomic seal する。
