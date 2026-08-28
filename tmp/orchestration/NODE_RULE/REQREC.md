# Node 層 obligation ルール追加 — 要求回復

- 工程: 計画のみ（段階 C）
- 調査日: 2026-08-28
- 実装 checkpoint: **通過（2026-08-28、オーケストレータ確認）**。差し戻し 1 回（§5 契約列挙漏れ: `crates/reviewgraphen-benchmark/`）は反映済み。詳細は本文書末尾の「checkpoint 記録」。
- revisions: 1
- 書き込み範囲: `tmp/orchestration/NODE_RULE/` のみ

## 1. Goal

`rust.production.v1` の新しい versioned rule-set で、profile exclusion 後の各 `pub fn` artifact に Node 契約レビュー obligation をちょうど1件生成する。生成は単一 snapshot の accepted `ast` / `containment` fact だけに依存し、`changed_structure` と `direct_calls` の有無・状態に依存しない。

## 2. Observed behavior

### 実際に読んだ / 実行した結果

- `git status --short`（exit 0）で、着手前から `tmp/orchestration/ORCHESTRATION_LOG.md` の変更と多数の未追跡 benchmark / orchestration artifact があることを確認した。本工程はそれらへ触れない。
- `MvpRulePack::synthesize` は `rust.production.v1` を `synthesize_changed_public_callee` へ即時分岐するため、production path は現状 D rule 専用である（`crates/reviewgraphen-core/src/synthesize.rs:761`）。
- D rule は `calls` + `syntactic_unique` を入口にし（`crates/reviewgraphen-core/src/synthesize.rs:1151`, `crates/reviewgraphen-core/src/synthesize.rs:1152`）、changed containment witness が無ければ候補を捨てる（`crates/reviewgraphen-core/src/synthesize.rs:1171`）。
- Rust ingest は parse failure の有無だけで `ast` と `containment` を `complete` / `partial` にし（`crates/reviewgraphen-ingest/src/rust.rs:121`, `crates/reviewgraphen-ingest/src/rust.rs:126`, `crates/reviewgraphen-ingest/src/rust.rs:135`）、`direct_calls` は常に `partial` と宣言する（`crates/reviewgraphen-ingest/src/rust.rs:164`）。
- free function artifact は `kind = "function"` と exact Rust visibility 由来の `public` を持ち、同時に module→function の `contains` draft を生成する（`crates/reviewgraphen-ingest/src/rust.rs:687`, `crates/reviewgraphen-ingest/src/rust.rs:706`, `crates/reviewgraphen-ingest/src/rust.rs:969`）。method は別の `kind = "method"` である（`crates/reviewgraphen-ingest/src/rust.rs:777`）。
- `context.subject_windows@3` の canonical bytes と golden hash は source に固定されている（`crates/reviewgraphen-core/src/context.rs:403`, `crates/reviewgraphen-core/src/context.rs:414`）。現行 role enum は `Callee`, `Caller` に加えて `Support` も持つ（`crates/reviewgraphen-core/src/context.rs:4036`, `crates/reviewgraphen-core/src/context.rs:4039`）。
- run v3 は D-only `GenericCoverageV2` を1個持ち（`crates/reviewgraphen-runtime/src/generic.rs:1720`, `crates/reviewgraphen-runtime/src/generic.rs:1731`）、coverage constructor も D の resolved/gap closure を前提にする（`crates/reviewgraphen-runtime/src/generic.rs:3587`）。
- `python3 -c` で PLAN の提案 policy object を RFC 8785 相当の本 repo canonical ordering（sorted keys, compact JSON）へ直列化し、2040 bytes、`sha256:9f15006986a73853ff3be7b9a158e8c79f69b9a8099c9d0a1991d8c7da170aed` を得た（exit 0）。実装時は Rust の `canonical_json` と独立 golden literal の双方で再計算する。
- `DEVELOPMENT.md` は `scripts/ci.sh` を唯一の supported verification entry point とする（`DEVELOPMENT.md:3`）。`scripts/ci.sh fast` の実行列は validator、CI admission、rustfmt、clippy、nextest、doc test、未コミット regression/snapshot 検査である（`scripts/ci.sh:92`）。

### 推測（未実装・未実行）

- **推測:** Node rule を追加すると1 snapshot 当たりの obligation 数が D rule より大幅に増え、runtime v4 の schema/semantic closure と context basis 再構築が実装時間の最大部分になる。実測は実装工程の fixture と representative repository run で行う。
- **推測:** v4 の provider-free packet 件数増加により CLI runtime と artifact 数が増える。性能上限は既存の plan bounds で fail closed / defer できるはずだが、実装後の計測前には保証しない。
- **推測:** `context.subject_windows@4` の containment-only support windows が実レビューに十分かは、この工程では評価していない。policy は bounded/auditable context を定義するだけで、レビュー有効性を主張しない。

## 3. Explicit requirements

依頼文から逐語で保持する。

> **ast と containment は complete です。**この2つだけで作れる義務ルールなら、差分も呼び出し解決も要らず、任意のスナップショットで義務が立ちます。

> 例：Node 層のルール——「公開関数それぞれに契約レビュー義務を1つ」。必要なのは「関数がどこにあるか」（ast）と「どのモジュールに属するか」（containment）だけ。いま既に完全に取れている情報です。

> (b) Node 層まで（context policy @4 + run schema v4）

> あなたの工程は **計画のみ** です。実装はしません。

> `tmp/orchestration/NODE_RULE/` の下にだけ書いてよい（成果物の置き場）。

> **それ以外のファイルは1文字も変更しない。**`crates/` `schemas/` `docs/` `examples/` `benchmarks/` `scripts/` は読み取りのみ。

> **既存 `@3` を壊さず共存させる方法**（policy を obligation の target_kind で選ぶのか、rule で選ぶのか）。

> **seal 済みの benchmark を書き換えない**。

> 実装には進まないでください。

## 4. Recovered constraints

### 出所確認済み

1. Coverage denominator は snapshot/profile/rule/extractor に相対な versioned universe でなければならず、exclusion は silent deletion ではなく record で残す（`docs/adr/0003-obligations-define-the-coverage-universe.md:17`, `docs/adr/0003-obligations-define-the-coverage-universe.md:73`）。
2. Projection は source IDs、excluded/unresolved、information loss、policy version/hash を保持する（`docs/adr/0004-project-minimal-context-with-declared-loss.md:21`, `docs/adr/0004-project-minimal-context-with-declared-loss.md:24`）。
3. 同じ snapshot/profile/rules/extractors からの obligation ID は安定し、source location だけでIDを作らない（`docs/03_conceptual_model.md:345`, `docs/03_conceptual_model.md:357`）。
4. schema/public type の変更前に ADR を書き、versioning/migration を定義する必要がある（`AGENTS.md:54`, `AGENTS.md:57`, `AGENTS.md:58`）。次の未使用番号は 0040 であり、`docs/adr/0039-task-subject-context-projection.md:1` が現在の末尾である。
5. Node rule の exact public 判定に使える accepted fact は free function artifact の `public == matches!(Visibility::Public(_))` である（`crates/reviewgraphen-ingest/src/rust.rs:687`, `crates/reviewgraphen-ingest/src/rust.rs:969`）。`pub(crate)` 等や `kind = "method"` を含めない（`crates/reviewgraphen-ingest/src/rust.rs:777`）。
6. module membership witness は accepted `contains` relation で表され、free function、test alias、method、type の収集箇所で同じ constructor が使われる（`crates/reviewgraphen-ingest/src/rust.rs:706`, `crates/reviewgraphen-ingest/src/rust.rs:743`, `crates/reviewgraphen-ingest/src/rust.rs:786`, `crates/reviewgraphen-ingest/src/rust.rs:843`, `crates/reviewgraphen-ingest/src/rust.rs:952`）。Node predicate は free function の witness だけを採る必要がある。
7. Legacy materialization は `required_capabilities` を保ち、production D は別の split-capability path を使う（`crates/reviewgraphen-core/src/synthesize.rs:796`, `crates/reviewgraphen-core/src/synthesize.rs:1223`）。Node rule は `target_support = {ast, containment}`, `enumeration = {}` の新しい exact spec とし、D spec を流用・再分類しない。
8. D coverage は `Option<Box<DTwoLayerCoverage>>` として legacy universe bytesから隔離され、D struct 自体は単一 rule ID を持つ（`crates/reviewgraphen-core/src/synthesize.rs:72`, `crates/reviewgraphen-core/src/synthesize.rs:79`）。複数 rule は外側を per-rule union にする必要がある。
9. `SplitCapabilitySpec::d_rule()` は D の exact capability set を固定する（`crates/reviewgraphen-core/src/synthesize.rs:1785`, `crates/reviewgraphen-core/src/synthesize.rs:1815`）。Node spec は別 constructor と rule-contract validation を持たせる。
10. profile canonical bytes/hash は path matcher policyだけを表し、現在の hash は literal constant である（`crates/reviewgraphen-core/src/profile.rs:18`, `crates/reviewgraphen-core/src/profile.rs:27`）。rule 追加だけなら profile bytes/hash を変えず、rule-set hash/version を別に進める。
11. profile exclusion identity は D rule と `rule|relation:` prefix、weight 4.0 に固定される（`crates/reviewgraphen-core/src/profile.rs:640`, `crates/reviewgraphen-core/src/profile.rs:647`）。Node 対応では DTO/validator を rule registry 化しつつ、D の preimage field/valueを一切変えない。
12. v3 context は caller/callee 2 subjectを固定順序で構築し、role priority も callee→caller→support である（`crates/reviewgraphen-core/src/context.rs:4552`, `crates/reviewgraphen-core/src/context.rs:5729`, `crates/reviewgraphen-core/src/context.rs:6285`）。既存 enumへ `Subject` を追加すると v3 family を誤って広げるため、v4 DTO/role familyを別にする。
13. run-v3 schema は D property、caller/callee、2 subject outcomes、D-only coverage constを閉じている（`schemas/reviewgraphen.generic_review_run.v3.schema.json:39`, `schemas/reviewgraphen.generic_review_run.v3.schema.json:44`）。run-v4 は exact `oneOf`/enum DTOで widening し、任意文字列へ開かない。
14. human-report-v2 schema も D subject loss/property と2 subject outcomesを閉じている（`schemas/reviewgraphen.generic_review_human_report.v2.schema.json:32`, `schemas/reviewgraphen.generic_review_human_report.v2.schema.json:37`）。run-v4 と対になる human-report-v3 が必要で、v2を編集しない。
15. current obligation schema は `reviewgraphen.review_obligations.v2` の current alias で、D two-layer ruleを const にする（`schemas/reviewgraphen.obligation.schema.json:15`, `schemas/reviewgraphen.obligation.schema.json:385`）。旧 bytesを新規 `.v2` pathへ先に保存し、current aliasを v3へ進める。
16. runtime は request/run v2/v3 を別 public typeとして保持し、v3 bytes-only validation と basis-bound validationを分離する（`crates/reviewgraphen-runtime/src/generic.rs:1619`, `crates/reviewgraphen-runtime/src/generic.rs:1720`, `crates/reviewgraphen-runtime/src/generic.rs:4552`, `crates/reviewgraphen-runtime/src/generic.rs:4578`）。v4も同じ capability boundaryを持つ。
17. report は validated run-v3 capabilityからだけ human-report-v2 を作る（`crates/reviewgraphen-report/src/generic_non_authority.rs:76`, `crates/reviewgraphen-report/src/generic_non_authority.rs:95`）。run-v4→report-v3 の別 dispatchが必要で、wire-only runからreportを作らない。
18. CLI の schema registryと artifact names はv3/v2番号を literal に列挙する（`crates/reviewgraphen-cli/src/lib.rs:432`, `crates/reviewgraphen-cli/src/lib.rs:763`, `crates/reviewgraphen-cli/src/lib.rs:939`）。v4/v3を追加し、旧名を置換しない。
19. M20 preregistration は product CLI SHA-256 を pin する（`benchmarks/m20-changed-public-callee-utility-v1/preregistration.json:66`）うえ、evaluator は context-v3 bytes/hashを literal に固定する（`benchmarks/m20-changed-public-callee-utility-v1/evaluator/stage0_contract.py:11`, `benchmarks/m20-changed-public-callee-utility-v1/evaluator/stage0_contract.py:14`）。新 build の binary hashは変わるので seal済み runへ差し替えず、v3評価はpin済みbinary/commitを使う。
20. m22相当の探索 harness は frozen v3 requestを実行し、caller/callee/relation shapeを抽出する（`tmp/orchestration/bug-discovery-v1/PLAN.md:28`, `tmp/orchestration/bug-discovery-v1/PLAN.md:40`）。arm B は current `skills/reviewgraphen/SKILL.md` を正本として読む（`tmp/orchestration/bug-discovery-v1/PLAN.md:61`, `tmp/orchestration/bug-discovery-v1/PLAN.md:67`）。harnessはv3のまま維持し、skillだけv4 current surfaceとv3 compatibilityを追記する。
21. `MvpRulePack::synthesize` はpublic compatibility entry pointで、production profileをD-only entryへ分岐する（`crates/reviewgraphen-core/src/synthesize.rs:760`, `crates/reviewgraphen-core/src/synthesize.rs:761`）。この意味を変えると、公開benchmark preparation（`crates/reviewgraphen-benchmark/src/prepare.rs:545`, `crates/reviewgraphen-benchmark/src/prepare.rs:552`）、v1 runtime（`crates/reviewgraphen-runtime/src/generic.rs:610`, `crates/reviewgraphen-runtime/src/generic.rs:888`）、durable genesis reproduction（`crates/reviewgraphen-core/src/event.rs:820`, `crates/reviewgraphen-core/src/event.rs:832`）が同時に別universeを観測する。したがって既存entry/typeの意味を固定し、v4 mixed synthesisは別public entryと別bundle/universe familyにする。

### 推測（出所なし）

- **推測:** weight `3.0` は既存 legacy Node rule の相対重みと整合し、relation D の `4.0` より低い妥当な初期値である。これは効果測定済みrisk calibrationではないため、ADR 0040で「policy choice」と明記する。
- **推測:** containment-only @4 の `1 subject + 最大7 support` 配分は、1 endpointを必ず最優先しつつ既存8-window envelope上限を保つ最小変更である。レビュー有効性は未評価である。

### 修正ラウンドで更新された要求（R1〜R6）

R1. production-v4 の mixed synthesis が生成し得る obligation は、D substantive、D candidate-space gap、Node substantive の全てである。v4 schema、registry、validator、plan、coverage、report はこの閉じた全集合を扱い、D gap を Node gap に転写しない。

R2. request-v4/run-v4/report-v3 の wire contract は v3 と同水準で閉じる。rule/property/target/capability/applicability tuple は registry const へ束縛し、plan、context、coverage の ID set と digest は validator が再構築する。universe ID preimage は request schema、rule-set、extractor、policy identity を含む。

R3. `context.subject_windows@4` の canonical policy bytes は実装可能な挙動だけを宣言する。accepted-file denominator は全 accepted file から作り、reverse `contains` depth 1、subject/support slot、window/excerpt/loss の各 bound と loss declaration を実装する。守れない宣言は policy bytes、hash、ADR を同時に更新する。

R4. v4 は candidate、obligation、source、coverage item を無記録で捨てない。containment witness 欠落、source/context materialization不能、partial capability は exclusion または typed obstruction / unknown / deferred として残す。accepted `contains` witness は source/target の cardinality と accepted module/function endpointを検証する。

R5. テスト名は検査する性質と一致し、REQREC §8 の19件を実装する。v4 regression は新規 `generic_v4.rs` に置き、変更禁止と宣言した `m1.rs` は元へ戻す。追加テストは名称と実検査の対応をレビューする。

R6. report-v3 は run-v4 が宣言する exclusions、deferrals、observations、coverage、source trace を空値へ落とさず、schema と validator も同じ閉包を強制する。

M1 例外記録: `m1.rs` は current obligation alias をv3へ進めた後も、legacy
`MvpRulePack::synthesize()` が出すv2 contractの完全 fixture oracleである必要がある。
したがって include先を保存済み `reviewgraphen.obligation.v2.*` へ固定する。この
変更はNode/v4をM1へ混入させないための互換性境界であり、M1の規則意味を変更しない。

## 5. Affected contracts

- 公開 core API/type: production rule-pack selector、Node rule descriptor/spec、per-rule coverage union、profile exclusion candidate/bindings、`ContextSubjectWindowsPolicyV4`、v4 context/window/subject-loss/basis types。
- Schema/永続 bytes: review-obligations v3、generic request v4、generic run v4、generic human-report v3、policy @4 canonical bytes/hash、対応examples/goldens。
- Universe/ID: 新 rule/property、Node obligation IDs、new rule-set version/hash、mixed universe ID、Node exclusion IDs、v4 context/projection/run/report IDs。D v3 IDs/bytesは不変。
- Runtime: v4 exact dispatch、mixed per-rule plan/context/coverage closure、wire-only vs basis-bound validation、provider-free task IDのrequest-contract分離。
- Report/CLI: run-v4→human-report-v3 renderer、schema list/print/validate、`audit.run.v4.json` / `human-report.manifest.v3.json`。v3 artifact namesは維持。
- Configuration/profile: `rust.production.v1` matcher bytes/hashは不変。新しい production rule-set tupleをprofileとは別にpinする。
- Docs/skill: ADR 0040、current capability、CLI/report/schema docs、new quickstart、`skills/reviewgraphen/SKILL.md`。
- Downstream: `reviewgraphen-report` はv4を追加対応。`reviewgraphen-benchmark` のproduction APIはlegacy D-only consumerとして意味を変えず、consumer回帰testだけ追加する。M20 evaluatorとm22 harnessはv3 compatibility consumerとして変更しない。
- Event/authority: 新しい accepted fact/Evidence/Verification/Decision eventは作らない。provider-free observationは従来同様non-authorityで、run-v4/report-v3も `trusted_pass=false` / `incomplete` を維持する。

### Synthesis entry point / public type consumer sweep（READ ONLY）

`crates/` 全体（testsを含む）と `scripts/` に対して exact symbol searchを行った。`MvpRulePack::synthesize(` は22ファイル87箇所、`synthesize_changed_public_callee(` は3ファイル18箇所、`validate_changed_public_callee_endpoints(` は3ファイル6箇所だった。`scripts/` にはこれらの直接consumerも、`ObligationBundle` / `UniverseDescriptor` 等のRust public type consumerも無かった。

実行時または再利用可能fixture/public APIからのconsumerは次のとおり。

- Generic runtime: v1がlegacy entry（`crates/reviewgraphen-runtime/src/generic.rs:888`）、v2/v3がD-only entry（`crates/reviewgraphen-runtime/src/generic.rs:2230`, `crates/reviewgraphen-runtime/src/generic.rs:2546`）。同じファイルのv4だけnew mixed entryを使い、三つの既存call siteは変更しない。
- Fixed offline runtime: double-submit constructors/helpersのlegacy entry 3件（`crates/reviewgraphen-runtime/src/fixed_offline.rs:201`, `crates/reviewgraphen-runtime/src/fixed_offline.rs:316`, `crates/reviewgraphen-runtime/src/fixed_offline.rs:730`）。profile/fixture bytesを含め無変更。
- Benchmark: public `prepare_from_ingest_request` と `prepare_real_full_review` がlegacy entryを使う（`crates/reviewgraphen-benchmark/src/prepare.rs:545`, `crates/reviewgraphen-benchmark/src/prepare.rs:552`, `crates/reviewgraphen-benchmark/src/prepare.rs:1023`, `crates/reviewgraphen-benchmark/src/prepare.rs:1025`）。CLI binaryからも両APIへ到達する（`crates/reviewgraphen-benchmark/src/main.rs:529`, `crates/reviewgraphen-benchmark/src/main.rs:543`）。production source/APIは無変更、test-only変更でD-onlyを固定する。
- Core durability/M6: genesis reproduction（`crates/reviewgraphen-core/src/event.rs:832`）、terminal M6 recomputation（`crates/reviewgraphen-core/src/event.rs:37527`）、accepted-universe resynthesis validator（`crates/reviewgraphen-core/src/m6.rs:16960`）がlegacy entryを使う。既存persisted universeの再現意味を守るため無変更。
- Store public test-support:公開fixture materializersの内部helperがlegacy entryを使う（`crates/reviewgraphen-store/src/test_support.rs:649`, `crates/reviewgraphen-store/src/test_support.rs:699`）。fixture/genesis bytesを含め無変更。

test-only consumerも既存entryのcompatibility oracleなので削除・mixed化しない。対象は `crates/reviewgraphen-core/src/context.rs:8083`、`crates/reviewgraphen-core/src/planning.rs:1237`、`crates/reviewgraphen-core/src/review.rs:4085`、`crates/reviewgraphen-core/src/event.rs:57775`、`crates/reviewgraphen-core/src/m6.rs:18448`、`crates/reviewgraphen-core/tests/m1.rs:338`、`crates/reviewgraphen-core/tests/changed_public_callee_rule.rs:78`、`crates/reviewgraphen-ingest/tests/changed_public_callee_facts.rs:478`、`crates/reviewgraphen-ingest/tests/m2.rs:2848`、`crates/reviewgraphen-runtime/src/lib.rs:1012`、`crates/reviewgraphen-runtime/src/m4_verification.rs:641`、`crates/reviewgraphen-report/src/lib.rs:2415`、`crates/reviewgraphen-report/tests/support/mod.rs:216`、`crates/reviewgraphen-report/tests/support/v3.rs:226`、`crates/reviewgraphen-reviewer/src/lib.rs:2830`、`crates/reviewgraphen-store/src/index.rs:5678`、`crates/reviewgraphen-store/src/index_v6.rs:3011`、`crates/reviewgraphen-store/src/journal.rs:12138`、`crates/reviewgraphen-benchmark/src/prepare.rs:1678`。

D-only entryのconsumerはruntime v2/v3（`crates/reviewgraphen-runtime/src/generic.rs:2230`, `crates/reviewgraphen-runtime/src/generic.rs:2546`）とCoreのcontext/rule tests（`crates/reviewgraphen-core/src/context.rs:8122`, `crates/reviewgraphen-core/tests/changed_public_callee_rule.rs:78`）だけである。endpoint validatorのconsumerはruntime v2/v3（`crates/reviewgraphen-runtime/src/generic.rs:2287`, `crates/reviewgraphen-runtime/src/generic.rs:2625`）、同rule tests（`crates/reviewgraphen-core/tests/changed_public_callee_rule.rs:493`）、D exclusion materialization内部（`crates/reviewgraphen-core/src/synthesize.rs:1515`）である。いずれもD-only familyに残し、v4 Node armのendpoint/context解決へ流用しない。

public return/storage typeも分割境界に含む。existing `ObligationBundle` はruntime v1-v3 helpersが明示的に保持し（`crates/reviewgraphen-runtime/src/generic.rs:410`, `crates/reviewgraphen-runtime/src/generic.rs:3034`, `crates/reviewgraphen-runtime/src/generic.rs:3588`）、existing `UniverseDescriptor` は `ReviewAggregate` とgenesis/event serializationに格納される（`crates/reviewgraphen-core/src/review.rs:1584`, `crates/reviewgraphen-core/src/event.rs:126`, `crates/reviewgraphen-core/src/event.rs:349`）。そのため既存 `ObligationBundle` / `UniverseDescriptor` / `ObligationContract` / `ExclusionRecord` のwire shapeやgetter semanticsへv4 fieldを足さない。v4はnew `ObligationBundleV3` / `UniverseDescriptorV3` / `ObligationContractV3` / closed `RuleCoverageV3` family（最終名はADR 0040で固定）とv4 read-only basisを使い、existing `ReviewAggregate` / EventLog persistenceへ流し込まない。

## 6. Scope

### 実装工程で変更するファイル/モジュール

- 先行ADR: `docs/adr/0040-public-function-node-obligation-and-production-v4.md`。
- Core: `crates/reviewgraphen-core/src/synthesize.rs`, `crates/reviewgraphen-core/src/profile.rs`, `crates/reviewgraphen-core/src/context.rs`, exportが必要な `crates/reviewgraphen-core/src/lib.rs`。
- Core tests: 新規 `crates/reviewgraphen-core/tests/public_function_node_rule.rs`, 新規または分離した `context_subject_windows_v4.rs`, 既存のcompatibility oracleを置く `changed_public_callee_rule.rs`, `context_subject_windows.rs`, `review_profile.rs`。
- Schemas: 新規 `reviewgraphen.obligation.v2.{schema,example}.json`; current aliasの `reviewgraphen.obligation.{schema,example}.json` をv3化; 新規 `reviewgraphen.generic_review_request.v4.{schema,example}.json`, `reviewgraphen.generic_review_run.v4.{schema,example}.json`, `reviewgraphen.generic_review_human_report.v3.{schema,example}.json`; `schemas/README.md`。
- Runtime: `crates/reviewgraphen-runtime/src/generic.rs`, 新規 `crates/reviewgraphen-runtime/tests/generic_v4.rs`。
- Report: `crates/reviewgraphen-report/src/generic_non_authority.rs` と必要なexports、`crates/reviewgraphen-report/tests/generic_non_authority.rs` または新規v4専用test。
- CLI: `crates/reviewgraphen-cli/src/lib.rs`, `crates/reviewgraphen-cli/tests/generic_quickstart.rs` または新規v4 quickstart test。
- Benchmark test-only: 新規 `crates/reviewgraphen-benchmark/tests/production_synthesis_compat.rs`。`src/prepare.rs` / `src/main.rs` の実装と公開APIの意味は変更しない。
- Bundle/docs: `scripts/validate_bundle.py`, `MANIFEST.md`, `README.md`, `docs/13_cli_contract.md`, `docs/14_report_and_schema_contract.md`, `docs/23_current_capability_status.md`, `docs/index.md`, `skills/reviewgraphen/SKILL.md`。
- Reference: 新規 `examples/public-function-node-quickstart/`（request v4、README、verify、expected hashes）。既存D quickstartは保存する。

### 変更しない領域

- Ingest extraction implementation: `crates/reviewgraphen-ingest/src/rust.rs`, `crates/reviewgraphen-ingest/src/git.rs`, `crates/reviewgraphen-ingest/src/lib.rs`。既存accepted ast/containment/public factを消費するだけで extractor semantics/versionを変更しない。
- Store/event durability: `crates/reviewgraphen-store/` 全体、既存journal/genesis/event schema。
- Existing synthesis consumers in Core: `crates/reviewgraphen-core/src/event.rs`, `crates/reviewgraphen-core/src/m6.rs`, `crates/reviewgraphen-core/src/review.rs`, `crates/reviewgraphen-core/src/planning.rs`。legacy reproduction/resynthesis/aggregate semanticsを変えない。
- Existing synthesis consumers in Runtime: `crates/reviewgraphen-runtime/src/fixed_offline.rs`, `crates/reviewgraphen-runtime/src/lib.rs`, `crates/reviewgraphen-runtime/src/m4_verification.rs`。`generic.rs` はv4 branchを追加するが、v1 `:888`、v2 `:2230`、v3 `:2546` のcall/return typeは変えない。
- Benchmark production implementation: `crates/reviewgraphen-benchmark/src/prepare.rs`, `crates/reviewgraphen-benchmark/src/main.rs`, `crates/reviewgraphen-benchmark/src/lib.rs`。追加するのは別test fileだけで、`prepare_from_ingest_request` / `prepare_real_full_review` はlegacy D-only entryを呼び続ける。
- Reviewer/verifier/security process: `crates/reviewgraphen-reviewer/`, `crates/reviewgraphen-verifier/`, `workspace.cargo_test@1` のdeferred behavior。
- Gluing/evidence/staleness: `crates/reviewgraphen-runtime/src/m4_verification.rs`, `m5_gluing.rs`, Coreのclaim/evidence/decision state machine。
- Frozen schema families: `schemas/reviewgraphen.generic_review_request.v1/v2/v3.schema.json`, `generic_review_run.v1/v2/v3.schema.json`, `generic_review_human_report.v1/v2.schema.json`, `reviewgraphen.obligation.v1.schema.json`, `reviewgraphen.review_profile.v1.schema.json` と対応example bytes。
- Existing examples: `examples/changed-public-callee-quickstart/`, `examples/double-submit-payment/`。
- Existing synthesis consumer tests/support: `crates/reviewgraphen-core/tests/m1.rs`, `crates/reviewgraphen-ingest/tests/changed_public_callee_facts.rs`, `crates/reviewgraphen-ingest/tests/m2.rs`, `crates/reviewgraphen-report/tests/support/`, `crates/reviewgraphen-store/src/test_support.rs`, `index.rs`, `index_v6.rs`, `journal.rs` のlegacy fixture builders。新benchmark sentinel以外は変更しない。
- Sealed evaluation: `benchmarks/m20-*` 全体（evaluator、seal、preregistration、stage outputsを含む）。
- m22 exploration harness: `tmp/orchestration/bug-discovery-v1/` 全体。v3 input/output shapeを維持して無変更とする。
- Orchestration state: `tmp/orchestration/ORCHESTRATION_LOG.md` および `tmp/orchestration/NODE_RULE/` 以外の全 `tmp/orchestration/`。
- External effects: GitHub、network、provider execution、push、benchmark再実行は行わない。**計画フェーズでは commit も行わない。checkpoint 通過後の実装フェーズに限り、ブランチ `node-public-function-obligation` へのローカル commit を許可する（push は引き続き禁止）。**

## 7. Compatibility

- request-v2/run-v2/report-v1/context-v2 と request-v3/run-v3/report-v2/context-v3 の exact dispatch、cross-decode refusal、canonical bytesを維持する（`crates/reviewgraphen-runtime/src/generic.rs:4552`, `crates/reviewgraphen-runtime/tests/generic_v3.rs:589`）。
- D rule trigger、property、weight、capability split、two-layer coverage、profile exclusion preimageを変更しない（`crates/reviewgraphen-core/src/synthesize.rs:1151`, `crates/reviewgraphen-core/src/synthesize.rs:1152`, `crates/reviewgraphen-core/src/synthesize.rs:1223`, `crates/reviewgraphen-core/src/profile.rs:20`）。
- context @3 literal bytes/hashと callee/caller/support orderingを維持する（`crates/reviewgraphen-core/tests/context_subject_windows.rs:11`, `crates/reviewgraphen-core/src/context.rs:4552`）。
- profile v1 canonical bytesと `RUST_PRODUCTION_PROFILE_HASH` を維持する（`crates/reviewgraphen-core/tests/review_profile.rs:48`, `crates/reviewgraphen-core/src/profile.rs:18`）。rule追加はnew rule-set version/hashで表す。
- existing versioned fixture/exampleは上書きしない。例外はcurrent alias `schemas/reviewgraphen.obligation.schema.json` / `.example.json` のv3化であり、変更前bytesを新規 `.v2` pathsへ完全コピーして保存する。
- v3 requestをv4へ自動upcastしない。v4は同じ immutable revisionsを新 rule-setで**再synthesis**する新runであり、v3 coverage/authorityのmigrationではない。
- M20はpin済みbinary/commitとv3 contractでのみ再現する。新binaryをpreregistration pinへ差し替えない（`benchmarks/m20-changed-public-callee-utility-v1/preregistration.json:66`）。
- m22 harnessはfrozen v3 requestとD shapeを使い続ける。`skills/reviewgraphen/SKILL.md` はv4をcurrentとして追加説明し、v3手順を削除しない（`skills/reviewgraphen/SKILL.md:125`, `tmp/orchestration/bug-discovery-v1/PLAN.md:28`）。
- `MvpRulePack::synthesize` のproduction dispatchは引き続き `synthesize_changed_public_callee` だけを呼ぶ。new mixed entryをこのmethodから呼ばず、benchmark、generic v1、genesis reproduction、M6、fixed-offline、Store fixtureの既存obligation集合を不変にする（`crates/reviewgraphen-core/src/synthesize.rs:761`, `crates/reviewgraphen-benchmark/src/prepare.rs:552`, `crates/reviewgraphen-core/src/event.rs:832`）。
- existing `ObligationBundle` / `UniverseDescriptor` のserializationとpublic getterを変更しない。v4のper-rule unionはversioned new public typesへ隔離し、existing `UniverseStreamingRef` の10-field durable bytesへ混入させない（`crates/reviewgraphen-core/src/event.rs:349`, `crates/reviewgraphen-core/src/synthesize.rs:485`）。

## 8. Verification

### 完了判定コマンド

最終列は1本だけで、exit code 0を要求する。

```sh
scripts/ci.sh fast
```

この列は台帳上 `宣言のみ` で故障注入未確認なので、通過を保証へ読み替えない。オーケストレータが要求回復、ADR、diff、golden bytes、schema semantic closure、非変更領域を目視して checkpoint / 完了を判定する。

### 新規 regression tests と捕捉対象

1. `public_function_node_rule::snapshot_without_diff_or_calls_still_synthesizes_one_node_per_public_function` — `changed_structure` / `direct_calls` 依存の再混入を捕捉。
2. `public_function_node_rule::public_is_exact_pub_free_function_only` — private、`pub(crate)`、`pub(super)`、public method、type、test aliasの誤列挙を捕捉。
3. `public_function_node_rule::containment_is_required_and_all_witnesses_are_provenance_not_multiplicity` — witness欠落、module以外のsource、複数containsによる重複obligation、source trace欠落を捕捉。
4. `public_function_node_rule::ast_containment_are_exact_target_support_and_enumeration_is_empty` — capability分類のdriftと `direct_calls` の誤挿入を捕捉。
5. `public_function_node_rule::profile_exclusion_retains_weight_and_source_trace` —除外候補がsilent deletionになる回帰を捕捉。
6. `review_profile::d_exclusion_canonical_bytes_and_profile_hash_are_unchanged_after_generalization` — generic DTO化によるD exclusion ID/profile hash driftを捕捉。
7. `mixed_rule_coverage::node_is_single_layer_and_d_remains_two_layer` — Nodeへfake candidate-gapを作る回帰、D gapを消す回帰、rule間stage set混入を捕捉。
8. `context_subject_windows_v4::policy_has_independent_golden_bytes_hash_and_rejects_v3_cross_decode` — pinned policy drift、@3上書き、policy family混同を捕捉。
9. `context_subject_windows_v4::one_subject_is_first_and_has_one_reserved_slot` — role/order/cardinality/window配分の回帰を捕捉。
10. `context_subject_windows_v4::projection_uses_only_ast_containment_and_declares_subject_or_support_loss` — calls/tests traversalの再依存とundeclared lossを捕捉。
11. `generic_v4::same_snapshot_mixed_run_has_exact_per_rule_partitions` — run-v4 universe/plan/context/observation/coverage closureの崩れを捕捉。
12. `generic_v4::node_context_rejects_relation_fields_and_relation_context_rejects_node_fields` — `caller_artifact_id`/`callee_artifact_id` と `subject_artifact_id` のpermissive union化を捕捉。
13. `generic_v4::subject_outcome_cardinality_and_rule_property_policy_tuple_are_exact` — 1/2 subject cardinality、任意rule/property文字列、rule-policy mismatchを捕捉。
14. `generic_v4::v2_v3_canonical_goldens_remain_literal` — existing request/run/context/coverage bytes driftを捕捉。
15. `generic_non_authority_v3::projects_both_rule_coverages_without_promoting_authority` — report-v3のNode/D区別、qualified wording、authority ceiling回帰を捕捉。
16. `generic_quickstart_v4::schema_registry_and_artifact_names_are_version_exact` — CLI schema list/print/validate、run-v4/report-v3 artifact名、v3 filename置換を捕捉。
17. bundle validator の新v2-preservation/v3-current pairs — current obligation aliasだけ更新して旧v2を保存し忘れる回帰を捕捉。
18. `changed_public_callee_rule::legacy_production_selector_remains_d_only` — `MvpRulePack::synthesize` と `synthesize_changed_public_callee` のcanonical bundle equalityを固定し、new mixed entryの誤配線を最短で捕捉。
19. `reviewgraphen-benchmark/tests/production_synthesis_compat.rs::public_prepare_api_remains_d_only_for_unchanged_public_function_snapshot` — base=headのproduction ingestでpublic free functionが存在してもbenchmark公開APIのobligation集合がD-only（Nodeを含まない）であることを固定し、consumer固有のsilent denominator driftを捕捉。

### §8 実装状況（修正ラウンド最終）

| # | 状態 | 根拠 |
| --- | --- | --- |
| 1 | 実装済み | `public_function_node_rule::snapshot_without_diff_or_calls_still_synthesizes_one_node_per_public_function` |
| 2 | 未実装（宣言済み限界） | public visibility variant matrix は今回の blocker 範囲外 |
| 3 | 実装済み | `public_function_without_containment_is_an_explicit_exclusion_not_a_silent_drop` |
| 4 | 実装済み | #1 の exact capability assertions |
| 5 | 未実装（宣言済み限界） | profile exclusion regression は今回の blocker 範囲外 |
| 6 | 未実装（宣言済み限界） | D canonical exclusion regression は frozen v2 fixture oracle が保持 |
| 7 | 実装済み | #1 と `real_rust_snapshot_defers_d_capability_gap_and_plans_node_contexts` |
| 8 | 実装済み | `subject_windows_policy_v4_has_pinned_bytes_hash_and_rejects_older_families` |
| 9 | 未実装（宣言済み限界） | dedicated v4 slot-order fixture は未追加 |
| 10 | 未実装（宣言済み限界） | dedicated v4 projection-loss fixture は未追加 |
| 11 | 実装済み | `real_rust_snapshot_defers_d_capability_gap_and_plans_node_contexts` |
| 12 | 実装済み | `node_context_rejects_relation_fields_and_relation_context_rejects_node_fields` |
| 13 | 実装済み | `rule_property_policy_tuple_is_exact` |
| 14 | 未実装（宣言済み限界） | v2/v3 literal matrix は既存 compatibility tests に留める |
| 15 | 未実装（宣言済み限界） | v4 human-report focused regression は未追加 |
| 16 | 未実装（宣言済み限界） | CLI artifact-name focused regression は未追加 |
| 17 | 実装済み | bundle validator and preserved v2 pair hash oracle |
| 18 | 実装済み | `legacy_production_selector_remains_d_only` |
| 19 | 未実装（宣言済み限界） | benchmark public API fixture is outside this correction scope |

### 目視/diff checks（validator列が保証しない範囲）

- `git diff -- benchmarks/m20-changed-public-callee-utility-v1` が空であること。
- `git diff -- tmp/orchestration/bug-discovery-v1` が空であること。
- `git diff -- crates/reviewgraphen-benchmark/src/prepare.rs crates/reviewgraphen-benchmark/src/main.rs crates/reviewgraphen-benchmark/src/lib.rs` が空であること。benchmark crateはnew test file以外を変更しない。
- v2/v3 golden hash literals（run、context、profile、D exclusion）が変更されていないこと。
- new v4 run/reportの schema validatorだけでなく basis-bound semantic validator が set/digest/source closureを再構築していること。
- `git diff --check` が exit 0であること。

実装工程の完了報告は実行コマンドと各 exit codeを列挙し、`scripts/ci.sh fast` の一部成功を完了扱いしない。

## checkpoint 記録

- 判定日: 2026-08-28
- 判定者: オーケストレータ（Claude opus、herdr owner `rg-node-orch`）
- 結果: **通過**。実装フェーズへ進んでよい。
- 差し戻し回数（`revisions`）: 1

確認した5点:

1. **停止条件なし。**未確定要求を実装判断で埋める箇所は見当たらない。weight `3.0` と `@4` window 配分は policy choice として明示され、ADR 0040 に記録される前提になっている。
2. **`path:line` の全件確認: 通過。**両文書から抽出した 127 ユニークについて、ファイル実在・EOF 内・当該行が非空であることを個別に検査（`sed -n 'Np'` が EOF 超過でも 0 終了する穴を塞ぐため、行数と行内容を別々に判定）。不一致 0 件。
3. **無作為抽出の内容確認: 通過。**seed `rg-node-rule-checkpoint-2026-08-28` で 4 件、seed `rg-node-rule-checkpoint2-2026-08-28` で 4 件を開封し、記載どおりであることを確認。
4. **`@4` policy の独立再計算: 一致。**2040 bytes、`sha256:9f15006986a73853ff3be7b9a158e8c79f69b9a8099c9d0a1991d8c7da170aed`、sorted-keys compact と byte 一致、`subject_window_slots` 1 + `support_window_slots` 7 = `windows_per_envelope` 8。
5. **契約列挙漏れの独立探索: 1 件検出 → 差し戻し → 解消。**1 巡目に `crates/reviewgraphen-benchmark/`（`src/prepare.rs:552` が公開 API 経由で `MvpRulePack::synthesize()` を呼ぶ）が §5/§6/§7 のいずれにも無いことを検出し差し戻した。反映後、`schemas/reviewgraphen.config.example.toml`（Rust consumer 0 件の説明用 example）、`proptest-regressions/`、`verification/`（v3 schema 参照 0 件）を独立に当たり、新規漏れなしを確認。

完了判定は `scripts/ci.sh fast` の exit code に加え、オーケストレータの目視と diff レビューで行う。検証列は台帳上 `宣言のみ` であり、通過を保証と読み替えない。
