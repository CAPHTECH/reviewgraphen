# D1 独立契約レビュー

**BLOCKING: 7 / SHOULD-FIX: 4 / NOTE: 3**

判定の一次根拠はレビュー対象本文と現行 `crates/` 実装である。`tmp/orchestration/A1/` と `A2/` は論点探索にだけ使用し、その主張はコードと対象契約で再検証した。対象ファイル、`crates/`、既存ADRは変更していない。

## BLOCKING

### D1-B1 — 主要指標の abstention 経路が arm 非対称で、B に構造的加点を与える

- **分類:** BLOCKING
- **対象:** `benchmarks/m20-changed-public-callee-utility-v1/preregistration.json:120-168`; `benchmarks/m20-changed-public-callee-utility-v1/PROTOCOL.md:164-180,233-248`; `benchmarks/m20-changed-public-callee-utility-v1/README.md:43-58`
- **問題:** 主要値 `1` は「property-specific claim **または** admitted limitation/information-loss item に結び付いた typed abstention」で得られる。一方、A は ReviewGraphen limitation/unknown/loss records を明示的に受け取らず、B はそれらを必須入力として受け取る。したがって、同じモデルが両armで慎重に abstain しても、B は既知 loss ID に結び付けて `1`、A は結び付ける対象が原理的に無く `0` になり得る。`same output schema` という宣言は、参照可能な basis inventory の非対称を解消しない。また scorer が検証する `target`/`projection` closure も、A が D relation と obligation を受け取らず、B が受け取る以上、共通 target の正確な意味が凍結されていない。
- **なぜ測定不正か:** これは packet が有用だから disposition が改善したという効果と、「B にだけ成功扱いされる abstention 証明書を渡した」という scoring affordance を分離できない。慎重な baseline を機械的失敗へ落とすため、主要 endpoint 自体が介入armを優遇する。
- **具体的修正:** benchmark 契約担当は、実装前に arm-neutral output schema と scorer schema を本文へ固定すること。その上で (a) abstention を主要成功から両armとも除外する、または (b) 両armに同じ型・同じ closure 規則の arm-neutral limitation/loss inventory を与え、どちらも同じ条件で abstention を `1` にできるようにすること。共通の commit-level target/projection ID と、arm固有 source inventory の関係も明記し、A/Bそれぞれについて claim と abstention の成功例・失敗例を schema fixture として凍結すること。

### D1-B2 — ADR が必須とする fan-out/deferred gate が Stage 0 の決定境界から欠落している

- **分類:** BLOCKING
- **対象:** `docs/adr/0038-changed-public-callee-relation-slice.md:315-334,685-697`; `benchmarks/m20-changed-public-callee-utility-v1/preregistration.json:212-243`; `benchmarks/m20-changed-public-callee-utility-v1/PROTOCOL.md:91-132`; `benchmarks/m20-changed-public-callee-utility-v1/README.md:60-67`
- **問題:** ADR は固定300 cluster上の `p95 <= 50` と `deferred applicable D / all applicable D <= 0.05` を product gate とし、どちらか一方でも失敗なら slice failure と規定する。ところが preregistration/PROTOCOL/README の「全5 gate」は prevalence、subject retention、bounded context、determinism、enumeration honesty だけで、fan-out と deferred fraction を含まない。現契約なら p95 が数百、または大半が deferred でもモデル段階へ進める。
- **なぜ境界違反か:** fan-out 制御を測定せずに進行できる経路は、ADRが禁止した暗黙cap・drop・未実行分母の温存を検出できない。ADRの「slice fails」と benchmark の advance rule が矛盾しており、凍結可能な単一契約になっていない。
- **具体的修正:** benchmark 契約担当は Stage 0 gate に2条件を独立項目として追加し、分子・分母の exact ID sets、nearest-rank p95、zero-obligation commit の扱い、`exact` と `+1` fixture、失敗時の停止を preregistration/PROTOCOL/README/decision-boundary 文書で一致させること。「5 gates」という数も更新すること。

### D1-B3 — `cargo test` が実行する repository-controlled arbitrary code の threat model が無い

- **分類:** BLOCKING
- **対象:** `docs/adr/0038-changed-public-callee-relation-slice.md:435-548,813-817`; 参照コード `crates/reviewgraphen-verifier/src/lib.rs:121-145`; 参照ADR `docs/adr/0021-m4-evidence-bound-verification.md:1307-1310`
- **問題:** 固定argv、read-only repository、offline、mount/resource limit は書かれているが、`cargo test --workspace --all-targets` が workspace/dependency の `build.rs`、proc-macro、test binary を任意コードとしてコンパイル・実行する事実を一度も threat actor として扱っていない。さらに repository の `.cargo/config.toml` は rustc wrapper、runner、環境設定等の Cargo 実行経路を選び得るが、ADRの「wrapper selected by repositoryは禁止」という結論に対応する admission/refusal algorithm と test がない。unprivileged UID/GID、capability drop、`no_new_privileges`、syscall/device/proc isolation、実行可能mount、file/inode/open-file limit も閉じていない。
- **なぜ security boundary 違反か:** model prose が argv に到達しなくても、レビュー対象repositoryそのものが Cargo 経由で任意コードを実行する。これを通常のテスト実行として扱うと、path/env/network/write制限の一部だけで host containment を主張することになる。build script と proc-macro を無視した verifier は AGENTS.md の「任意shell実行をreviewerへ許可しない」という境界を、別経路から破る。
- **具体的修正:** ADR担当は repository、全dependency、build script、proc-macro、test、Cargo config を一律 untrusted executable input とする threat model を追加すること。sandbox の必須 isolation primitive、非root実行、capability/syscall/device/proc/mount規則、Cargo config の拒否または正規化、cacheに credentials/config を入れない admission、inode/file/open-file等のresource limitを normative に固定すること。実装担当向け test matrix に悪意ある `build.rs`、proc-macro、test、`.cargo/config.toml` wrapper/runner、absolute executable、fork/network/read/write/escape/resource attacks を追加すること。これを閉じられない場合は `workspace.cargo_test@1` を本sliceから外すこと。

### D1-B4 — commit cluster の独立性主張と exact McNemar p 値が両立しない

- **分類:** BLOCKING
- **対象:** `benchmarks/m20-changed-public-callee-utility-v1/README.md:20-41`; `benchmarks/m20-changed-public-callee-utility-v1/preregistration.json:8-26,70-83,372-379`; `benchmarks/m20-changed-public-callee-utility-v1/POWER_AND_DECISION_BOUNDARIES.md:15-32,182-195`; `benchmarks/m20-changed-public-callee-utility-v1/PROTOCOL.md:76-89,250-253`
- **問題:** 文書は各commit clusterを「independent analysis unit」とし、discordant commitを独立な `Binomial(b+c, .5)` として exact p 値を出す。しかし300 unitsは3 repositoryの連続first-parent commitsであり、同一コード、開発系列、作者、変更慣行、モデル難度を共有する。preregistration自身も `within-repository temporal dependence` を認めている。`repository is a blocking stratum` と記録するだけで、検定は層化もcluster調整もしていない。
- **なぜ測定不正か:** 独立でないdiscordanceを独立Bernoulliとして扱うと、`.03906`/`.04329` は nominal alpha=.05 の exact p 値ではない。有限sampleの operational rectangle は定義できても、その有意性説明は成立しない。
- **具体的修正:** 統計契約担当は、(a) p値とalpha claimを削除して frozen-corpus descriptive operational gate と明記する、または (b) repository/development-series dependenceを扱う事前登録済みcluster/randomization analysisへ変更し、十分な独立repository数を確保すること。少なくとも「independent」の語を外し、全repository別 paired cells と leave-one-repository-out sensitivity を必須にすること。3 strataのままなら一般的なconfirmatory exact p値を主張しないこと。

### D1-B5 — denominatorを決める profile/exclusion contract が識別子・matcherとも未固定

- **分類:** BLOCKING
- **対象:** `docs/adr/0038-changed-public-callee-relation-slice.md:75-101,301-313`; `benchmarks/m20-changed-public-callee-utility-v1/preregistration.json:99-118,120-143`; `benchmarks/m20-changed-public-callee-utility-v1/PROTOCOL.md:34-42,98-101,184-189`; 参照コード `crates/reviewgraphen-runtime/src/generic.rs:80-89`, `crates/reviewgraphen-core/src/synthesize.rs:583-590,1022-1066`
- **問題:** D triggerは「exact versioned review profile」による除外を必須にし、profile canonical bytes/hashをuniverse inputとするが、固定契約表にも preregistrationにも profile ID/version/hash、matcher grammar、path normalization、category判定、reason IDs が無い。PROTOCOLは test/example/generated/vendor/docs を除外するとだけ述べる。`non-test Rust source` と `production diff` の定義も無い。現行実装では profile fields は入力値で、既存 exclusion は artifact属性 `reviewgraphen_excluded` を読むだけであり、新しいprofile registry/matcherの意味はコードから補えない。また現行 exclusion ID は `candidate_key + snapshot` だけで、profile/reason/weightをbindしない。
- **なぜ境界違反か:** profileは eligible denominator と baseline source packet の両方を変える。実装時またはfreeze直前にmatcherを選べる契約は分母を事後的に作る余地を残し、同じ `ExclusionRecord` IDがprofile/reasonの異なるbodyを指す危険もある。
- **具体的修正:** ADR/benchmark担当は exact profile ID/version、canonical DTO/hash、全matcherとprecedence、path normalization、test/example/generated/vendor/docs分類、typed reason ID、production/non-test定義を今の契約へ追加すること。D `ExclusionRecord` ID preimageには少なくとも profile identity/hash、rule、candidate key、reason/matcher identity、weightとsource setをbindするか、別profile間で同一IDが同一意味になることを証明すること。preregistrationへprofile hashを置き、freeze manifestで後付けしないこと。

### D1-B6 — 未解決call occurrenceを D の candidate-space traceへ結ぶ typed seam が無い

- **分類:** BLOCKING
- **対象:** `docs/adr/0038-changed-public-callee-relation-slice.md:243-293,801-804`; 参照コード `crates/reviewgraphen-ingest/src/rust.rs:159-193,1594-1605,1609-1718,1941-1969`; `crates/reviewgraphen-ingest/src/lib.rs:1118-1189`
- **問題:** ADRは candidate-space incompleteness に「unresolved call occurrencesのextraction obstructions」を含めるが、per-rule coverage object の必須集合には obstruction IDs が無い。現行Rust adapterの各unresolved direct/method/macro occurrenceは `IssueDraft.related_capabilities = empty` で生成され、public obstructionにもrelated capabilityは残らない。唯一 `direct_calls` に結び付くのは、全Rust filesと5 capabilityをまとめたglobal limitationである。このため、個々の `RelationUnresolved`/`DynamicDispatchUnresolved` が call enumeration由来なのか、D ruleに投影すべきかをtyped fieldで選べない。
- **なぜ境界違反か:** 実装はglobal `direct_calls=partial` とgapだけを表示し、観測済みのmethod/cross-crate occurrencesをD coverageから落としても、現在列挙された必須fieldを満たせる。これは「unknown totalを数えない」こととは別で、既に観測したunknownのsource traceがper-rule denominator projectionから消える問題である。
- **具体的修正:** ingest/ADR担当は call occurrence obstructionに `related_capabilities` が `direct_calls` を含むこと、call-kind/reason/span/source IDsが閉じることをversioned contractとして追加すること。run-v2 per-rule coverageに必須 `enumeration_obstruction_ids` を追加し、exact extraction obstruction setとのclosure、partial時の非空条件、他capability obstruction混入拒否をsemantic validatorで再構築すること。macro expansionで個数自体が未知な部分はglobal limitationとして別に残すこと。

### D1-B7 — `context.subject_windows@2` の discovery semantics が同一policy IDの下で複数許される

- **分類:** BLOCKING
- **対象:** `docs/adr/0038-changed-public-callee-relation-slice.md:336-433`; 参照コード `crates/reviewgraphen-core/src/context.rs:80-205,1881-2007`; 参照ADR `docs/adr/0016-deterministic-planning-and-context-bounds.md:87-196,265-274`
- **問題:** window bounds/order/hash inputsは詳細だが、support sourceを「deterministically discovered, source-backed anchors」とするだけで、exact discovery graph、directions、depth、path/test capsを定義していない。さらに relation traversal等を v1 と「at least as strictly」とし、同じ `context.subject_windows@2` でより小さいdepth/capを合法にしている。現行v1は `callers_depth=2`、`callees_depth=3`、path/test caps、edge direction order等をcanonical DTOへ固定しており、`at least` ではない。
- **なぜ境界違反か:** 二つの準拠実装が異なるsupport candidate denominator、loss、window、projection hashを生成できる。hashが各結果を識別しても、同一policy IDの意味とreplay semanticsが閉じず、同一input determinismとpolicy versioningを満たさない。
- **具体的修正:** context契約担当は v2 DTOの全fieldとgolden canonical bytes/hashを固定し、support discoveryのseed、accepted edge kinds/directions、depth、BFS tie-break、path/test selection、anchor denominatorを完全に規定すること。v1を継承するなら `ContextPolicyV1::baseline()` のexact hash/field値を埋込み、`at least as strict` を削除すること。全変更は新policy versionを要求すること。

## SHOULD-FIX

### D1-S1 — equal limits は定義されているが、実現byte/token costを監査できない

- **分類:** SHOULD-FIX
- **対象:** `benchmarks/m20-changed-public-callee-utility-v1/preregistration.json:144-159,186-200`; `benchmarks/m20-changed-public-callee-utility-v1/PROTOCOL.md:164-170,222-231,276-280`
- **問題:** call数、timeout、output ceiling、retryは対称で、文書も「同一実現長ではなく同一ceiling」と正しく限定している。しかし runnerの必須記録に total serialized packet bytes、system/instruction bytes、tokenizer identity、実際のinput/output token数が無い。secondaryは admitted source bytesだけで、B固有metadataのbyte/token costを含まない。
- **なぜ測定上問題か:** 「equal limits」は検査できても、completion差がcontext構造、単なるprompt長、token truncationのどれに由来するか監査できない。
- **具体的修正:** evaluator/runner担当は tokenizer/version/hash、packet全bytes、instruction/metadata/source別bytes、preflight token count、provider-reported usage、truncation有無を両armで封印・公開すること。主要文言は引き続き `equal ceilings` とし、`equal realized budget` と呼ばないこと。

### D1-S2 — 24「model-hours」は21 model-hours + 3 hours reserveである

- **分類:** SHOULD-FIX
- **対象:** `benchmarks/m20-changed-public-callee-utility-v1/preregistration.json:284-296`; `benchmarks/m20-changed-public-callee-utility-v1/PROTOCOL.md:299-305`; `benchmarks/m20-changed-public-callee-utility-v1/POWER_AND_DECISION_BOUNDARIES.md:160-180`; `docs/adr/0038-changed-public-callee-relation-slice.md:685-697`
- **問題:** 80 reviewer calls × 900 s = 72,000 s、40 judge batches × 90 s = 3,600 s、reserve 10,800 s、合計86,400 s = 24 h は正しい。ただしmodel ceilingは合計75,600 s = 21 hで、残り3 hはidentity/validation/sealing等のreserveである。`cumulative_model_hours: 24` は用語上誤り。
- **なぜ測定上問題か:** 実行権限と費用説明で、model計算時間とserial wall-clock planning envelopeを混同する。
- **具体的修正:** `cumulative_wall_clock_envelope_hours=24` と `cumulative_model_ceiling_hours=21` を別fieldで記録し、README/ADRの表現も揃えること。24 model-hoursを本当に許可するならreserveとは別に計算し直すこと。

### D1-S3 — 手動control labelがsample membershipを動かす

- **分類:** SHOULD-FIX
- **対象:** `benchmarks/m20-changed-public-callee-utility-v1/preregistration.json:245-260`; `benchmarks/m20-changed-public-callee-utility-v1/PROTOCOL.md:134-160`
- **問題:** evaluatorはarm outcome前に rationaleを残すが、`intended absence` と `reviewable contract question` の判定規則、二重判定、disagreement処理がない。公開seedによるrankを知った状態で曖昧なcommitをcontrol/substantiveへ動かすと、8+2/32+8 sample membershipが変わる。
- **なぜ測定上問題か:** outcomeを直接見なくても、予想難度やBが有利そうな変更を用いた選択が可能で、frozen sampleの再現性がない。
- **具体的修正:** benchmark担当は機械判定可能なlabel rubricを固定するか、rankを開く前の独立二者label、disagreement=`ineligible`、全candidateのlabel/rationale sealを必須にすること。より安全には全applicable commitsから一つのhash sampleを取り、control labelはsample選択ではなくsecondary解析だけに使うこと。

### D1-S4 — workspace-wide verifierの実行単位・再利用・defer semanticsが未定義

- **分類:** SHOULD-FIX
- **対象:** `docs/adr/0038-changed-public-callee-relation-slice.md:448-452,478-524,536-548`; `docs/adr/0038-changed-public-callee-relation-slice.md:322-329`
- **問題:** descriptorは同じsnapshotへ常に同じworkspace-wide commandを実行するが、obligationごとに最大900秒で再実行するのか、snapshot単位で一度実行して複数obligationへ非権威参照を結ぶのかが不明である。p95=50まで許す契約では前者は1 commit最大12.5 wall-hoursになり得る。
- **なぜ性能/incrementality上問題か:** 準拠実装間でcost、`verifier-observed` numerator、fresh target semanticsが大きく変わり、planning/deferred fractionも比較不能になる。
- **具体的修正:** runtime/verifier担当は execution/dedup key（snapshot tree、descriptor、argv/env/toolchain/sandbox/mount hashes）、一回の観測を複数obligationへbindする非権威参照、再利用可能範囲、failure/timeout時の各ID状態、budget超過時のdeferを固定すること。Stage 0または別performance fixtureでexact/+1 costを検証すること。

## NOTE

### D1-N1 — exact p値の算術値そのものは正しい

- **分類:** NOTE
- **対象:** `benchmarks/m20-changed-public-callee-utility-v1/POWER_AND_DECISION_BOUNDARIES.md:46-84`
- **確認:** 独立再計算は `(b,c)=(8,1)` で `0.0390625`、`(18,7)` で `0.043285250663757324` となり、本文と一致した。D1-B4のとおり、問題は算術ではなく独立性仮定である。
- **修正:** 算術修正は不要。統計的ラベルだけD1-B4に従って修正すること。

### D1-N2 — authority/state境界と既存ID/version境界は明示的である

- **分類:** NOTE
- **対象:** `docs/adr/0038-changed-public-callee-relation-slice.md:68-73,130-138,544-548,605-648,656-683`; 参照コード `crates/reviewgraphen-core/src/synthesize.rs:654-688,1457-1469`; `crates/reviewgraphen-runtime/src/generic.rs:188-218,524-560`
- **確認:** 新rule/propertyは review questionに限定され、既存 `relation.changed_call_contract@1` の payment semanticsを再利用しない。generic v2/human reportは `non_authority`、`trusted_pass=false`、`incomplete` で、Cargo observationもEvidence/Verification/Decision/Findingへ自動昇格しない。confidence、judge、test successによる accepted/human-accepted promotion経路は契約上禁止されている。
- **修正:** この境界は維持すること。D1-B3のsandbox修正時もCargo exit zeroのauthorityを上げないこと。

### D1-N3 — freeze順序、post-launch failure=0、open-development limitationは明示されている

- **分類:** NOTE
- **対象:** `benchmarks/m20-changed-public-callee-utility-v1/PROTOCOL.md:32-89,204-231,311-350`; `benchmarks/m20-changed-public-callee-utility-v1/preregistration.json:99-118,161-184,347-379`
- **確認:** implementation/protocol freezeがcorpus resolutionより先で、registry/ranges/Stage0/model packets/results/judgeのseal順も規定される。stage launch後のmissing/failure=`0` とno retryは両arm対称で、open development・非holdout・非一般化も過大主張なく明記されている。
- **修正:** この順序と限界記述は維持すること。D1-B5のprofileをfreeze manifestで初めて決めるのではなく、preregistration側へ前倒しして固定すること。

## AGENTS.md 確認順に対する結論

| 確認項目 | 結論 |
| --- | --- |
| 1. 境界違反 | fact/claim/evidenceとauthority ceilingは維持。ただし verifier のrepository-code実行境界はD1-B3で未成立。 |
| 2. ID安定性/versioning | 新rule/schemaとv1 replay分離、旧rule非再利用は妥当。profile/exclusion IDとpolicy semanticsはD1-B5/B7で未閉鎖。 |
| 3. accepted/inferred/verified遷移 | 自動昇格経路は契約上禁止されており妥当。 |
| 4. source trace | admitted window traceは詳細だが、unresolved callのper-rule traceがD1-B6で欠落。 |
| 5. staleness | exact snapshot/tree/hash bindingかつ全出力non-authorityなので、このslice単独からstale evidenceでsign-offする経路はない。stale reasonの強制も記載済み。 |
| 6. coverage denominator | 二層分割の考え方自体は適合可能。しかしprofile未固定、unresolved occurrence closure欠落、fan-out gate脱落の現契約はAGENTS.mdの「分母を暗黙に作らない」に**適合しない**。 |
| 7. security boundary | model prose→argv/path/envは明示的に遮断されるが、Cargoが実行するbuild script/proc-macro/test/config経路が未処理。 |
| 8. performance/incrementality | boundsはあるがworkspace testの実行単位/再利用が未定義（D1-S4）。 |
| 9. projection表現 | subject typed lossとhash preimageは良いが、support discoveryが同一policy IDで一意でない（D1-B7）。 |

## 最終判定

**この契約を凍結して実装に進んでよいか: NO**

理由は、主要endpointがBだけに成功可能なabstention basisを与えること、ADR必須fan-out gateの欠落、Cargo arbitrary-code threat modelの欠落、独立性のないcommitにexact p値を適用していること、そしてdenominator/profile/unresolved-call/projectionの入力が一意に閉じていないことにある。これらは実装詳細ではなく、実装前に直すべき契約・測定・security boundaryである。
