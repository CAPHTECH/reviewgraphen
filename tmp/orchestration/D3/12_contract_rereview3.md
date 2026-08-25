# D3 独立契約再レビュー（round 3）

**BLOCKING: 4 / SHOULD-FIX: 2 / NOTE: 5**

**要約:** D2 の4 BLOCKINGのうち3件、2 SHOULD-FIXの両方は閉じた。primary endpoint も前回の blanket-abstention / `inconclusive` / ID-echo 経路そのものは閉じた。しかし endpoint 修正で、(1) closed packetから実際のsource本文が消えた、(2) abstention eligibilityを決める非対称・未導出のseamができた、(3) 1 judge batchからA/B二つのprimary値を得る型がない、という新しい問題が生じた。**全体としてまだ収束していない。**

対象は ADR 0038 と m20 の4ファイルである。`tmp/orchestration/F1/`、`F2/` に反論ファイルは存在しなかった。対象と過去レビューは読み取りだけに用い、対象ファイル・実装は変更していない。v2実装はまだ `crates/` / `schemas/` にないため、閉鎖判定は凍結前契約の一意性に対するものである。

## D2 指摘の閉鎖判定

| D2指摘 | 判定 | round 3 結論 |
| --- | --- | --- |
| D2-B1: primary endpoint空洞化 | **PARTIALLY-CLOSED** | common routine loss、`inconclusive`、ID echoは両armで明示的に0となり、claim/abstentionの機械条件とprimary utility judgeも追加された。埋込みfixtureに対して実際にその3種が0になることも確認した。しかし source本文欠落、eligible loss seam、judge batch型の欠落により「usable」のprimaryはまだ成立しない。D3-B1〜B4。 |
| D2-B2: baselineへのproperty lens漏れ | **CLOSED** | visible taskはopaque `review-task:sha256:<64hex>`、instruction/schemaはbyte-identical、property/rule/relation/endpoint/subject fields・値・cueはvisible packetで禁止された。埋込みA/B packetの再帰文字列検査でも禁止文字列は0件だった。propertyはhidden bindingとterminal auditに残り、reviewer inputへjoinすることは禁止される（ADR:805-889; preregistration:64-132,374-447,760-769）。 |
| D2-B3: inventory normative schema/fixture欠落 | **CLOSED** | inventory v2は全nested objectがclosedで、role/reason/scope、range、canonical order、ID/hash preimage、cross-field eligibilityを固定する。A/B packet fixtureは完全inventoryを含み、schema、各source/loss/inventory ID、packet hash、base64 canonical bytesを再計算して全件一致した。property field、foreign/extra roleはschemaで落ち、unknown loss/sourceはclosureで落ちることも独立に確認した（preregistration:133-209,512-769）。D3-B1はinventory metadataではなくpacket payloadの新規欠落である。 |
| D2-B4: ADR/m20 Stage 0・時間不一致 | **CLOSED** | ADR §10はseven gate IDsを全列挙し、`enumeration_honesty` と `deferred_fraction` を含む。全5ファイルで Stage 1/2A は `cumulative_model_ceiling=18,900/75,600 s` (5.25/21 h)、`wall_clock_envelope=21,600/86,400 s` (6/24 h) に一致する（ADR:975-1024; README:85-103; preregistration:795-872; PROTOCOL:78-138,351-361; POWER:99-151,195-210）。 |
| D2-S1: control label順序矛盾 | **CLOSED** | rankの秘匿を主張せず、全eligibleをarm outcome前にlabelし、public rankを知ってもmembership/orderへ影響しない契約へ統一された（preregistration:820-832; PROTOCOL:151-167; POWER:153-170）。 |
| D2-S2: subject-retention exact-ID不足 | **CLOSED** | `S_c/S` exact subsets、`A\S`のtyped loss/unknown closure、profile exclusion非混入、`20|S|>=19|A|`、57/60 pass・56/60 fail fixtureが全m20文書とADRに入った（ADR:982-987; README:93-103; preregistration:800-816; PROTOCOL:85-133; POWER:101-148）。 |

## BLOCKING — 今回の修正で生じた／露呈した問題

### D3-B1 — closed reviewer packetにRust source本文が存在しない（新規、最重要）

- **根拠:** reviewer packet v2 は `schema/task_id/instruction/output_schema_id/source_inventory` の5 fieldだけでclosedである。`admitted_sources` recordも `source_id/role/path/range/bytes/sha256` だけで、excerpt、content、UTF-8 bytes、blob referenceのいずれもない（preregistration:115-209）。埋込みA/B `packet_body` とbase64 canonical bytesにも本文はない（同:512-612）。一方、README/PROTOCOLは「admitted Rust sourceをinspect」「source bytes constructed/preserved」と主張する（README:49-53,67-74; PROTOCOL:193-216,263-268）。closed schemaなので実装が別fieldで本文を加えることもできない。
- **攻撃構成:** modelが見える `src/lib.rs`、range、source IDを用い、`issue_present`、適当な3語以上のsummary、`input reaches changed branch` / `branch returns invalid value` / `caller may fail now`、既知rangeのobservationを返すとmechanical条件を満たす。source本文がないのでreviewerもjudgeも真偽を照合できない。fixtureの「downstream code assumes a value」はpacket内にそのsource witnessがないのにjudge passが手書きされている。
- **帰結:** 「source-grounded」「source specificity」はhash/range metadataへのgroundingでしかなく、usable review判断にならない。lens漏れを避ける修正でsemantic input自体を落とした典型的な回帰である。
- **必要な閉鎖:** source recordへexact excerpt bytes（またはpacket内のclosed content-addressed payload tableとsource-to-payload closure）を追加し、byte count/hash/range/line encodingと一致させること。A/B完全fixtureは実際のRust本文を含み、claim observationとjudge rubricがそのbytesを参照してのみpassするnegative/mutation fixtureを持つ必要がある。

### D3-B2 — task-blocking loss eligibilityが機械導出されず、arm間の成功機会も対称でない（新規）

- **根拠:** inventory validatorは `task_blocking_*` reasonなら `primary_abstention_eligible=true` とするだけである（preregistration:201-208）。hidden `loss_support` は任意の `undecidable_question_id: minLength 1` を持ち、「actual task-blocking lossesだけ」と散文でいうが、固定question registry、loss→question導出、許されるID集合、source/context obstructionからreasonへ変換するalgorithmがない（同:332-338,425-447）。さらにA/Bの実inventoryで eligible loss setのcardinality・reason・questionを等しくする条件はない。fixtureだけは各arm一件ずつを人工的に置く（同:515-605,614-677）。
- **帰結:** evaluatorが同じ観測をroutineにもtask-blockingにも分類でき、hidden bindingにquestion IDを追加すればprimary abstention機会を作れる。Bのsubject-window projectionはAのdiff-only packetと異なるlossを生成するので、「同一schema/algorithm」は同一成功機会を意味しない。これは指示書が特に要求したloss集合サイズの非対称である。
- **必要な閉鎖:** property/task contract内に閉じた undecidable-question registry と、canonical extraction/context lossから eligible loss-support setを再構築するtotal functionを固定すること。primaryにabstentionを残すなら、paired armでeligible question opportunityを一致させるか、arm間で一致しないabstentionをprimary比較から外すこと。cardinality/reason/question mismatch fixtureを必須にすること。

### D3-B3 — 1 commit judge batchからA/B二つのprimary scoreを得る入出力型がない（新規）

- **根拠:** 予算は10/40 `judge_batches`、各90秒、すなわち1 commitにつき1 batchである（preregistration:340-370,845-870）。しかし utility-judge schemaは四dimension・total・verdictの**単一record**だけで、opaque candidate ID、二candidate配列、packet/output hash、task IDのいずれもない（同:353-368）。fixtureも同じ `common.judge_pass` recordをA/B双方へ再利用する（同:623-677）。別々にjudgeを呼べば20/80 callsとなり時間予算が倍、同じ一recordを両armへ使えば異なる二dispositionを個別評価できない。さらにjudge inputとされるhidden bindingは必須 `hidden_arm_id` を持つが、opaque derivation/除去規則がなく、「arm labels omitted」と両立しない（同:346,374-447; PROTOCOL:338-349）。
- **judge failureの帰結:** batch failureで両armを0にする規則は明記され、`n` は減らず `n00` へ移るので、指示書が懸念した denominator変更は起きない。ただし上記型欠落により、個別threshold failureとbatch failureをどう区別し、A/Bへ戻すかは未定義である。
- **必要な閉鎖:** 一つのbatch input/outputを、armを明かさない二つのopaque candidate IDsと各packet/output/binding hashへ閉じるclosed schemaとして固定すること。judge出力は両candidateの独立scoreを一度ずつ含み、callerがsealed permutationでA/Bへ戻す。missing one、duplicate、cross-bind、swapped、one/both threshold failure、whole-batch timeout exact fixturesと、予算計算を一致させること。`hidden_arm_id` はjudge inputから除外する。

### D3-B4 — mechanical usefulnessのテキスト判定とloss mappingが同一実装へ収束しない（新規）

- **根拠:** positive/zero fixtureのASCII proseは記載algorithmで期待値と一致した。しかし規範algorithmは「every exact ... enum literal and punctuationをremove」とだけ書き、対象lexemeのexact finite set、case、overlap時の置換順、Unicode punctuationの定義、削除で隣接tokenを連結するか、NFKC後のpath/IDも正規化するかを固定していない（preregistration:324-338; PROTOCOL:226-232）。これらは自然言語の有用性判断ではないが、二つのdeterministic scorerが異なるbooleanを返せるalgorithm gapである。abstention側はさらにD3-B2の「actual task-blocking」という未規定の意味判断へ依存する。
- **帰結:** primary predicate・fixture判定・rectangle cellsがscorer実装依存となる。現在の10 fixtureは単純ASCIIだけで、境界・overlap・Unicode・substring casesを検出しない。
- **必要な閉鎖:** exact preprocessingをコードポイント単位の擬似コードまたはversioned reference vectorsで固定し、remove対象をpacketから構成するsorted setとして列挙すること。24/1024 byte、2/3 token、case、NFKC、punctuation、overlap、ID/path substringのexact/+1 mutation matrixを追加すること。

## SHOULD-FIX

### D3-S1 — ADR §10だけprimary endpointの旧称を使う

ADR:1015 は `grounded-disposition completion` のままだが、m20全4ファイルの正規名は `usable_grounded_disposition_completed` である。旧endpointと新endpointはjudge/usefulness predicatesが異なるため、単なる略称に見せずexact nameへ直すべきである。

### D3-S2 — POWERの「judgeはsecondary」がprimary utility judgeと衝突する

POWER:216-217 は “The judge ... remain secondary” とする一方、同文書:21-29 と全m20契約ではutility judgeはprimary必要条件である。secondaryなのは同batchから派生する defect/safety projectionだけである。`utility judge = primary proxy` と `defect projection/clean-control safety = secondary` を明示的に分けるべきである。

## NOTE

### D3-N1 — fixtureを独立に実行検査した結果

`preregistration.json` はparse可能。埋込みpacket/inventory/output/score/judge schemaに対しA/B全packetと10 scorer fixtureはschema-validだった。RFC8785/JCS相当（当該fixtureは整数・文字列・object/arrayのみ）の再canonicalizationで、全source/loss/inventory IDs、packet SHA-256、base64 bytesが本文と一致した。記載mechanical algorithmを適用するとA/B claim successとabstention successはtrue、blanket/inconclusive/ID echoはfalseになった。property field、`callee` role、extra `subject_role`はschema failure、unknown loss/sourceはclosure failureとなった。

### D3-N2 — D1-B3/B4/B5/B6/B7のCLOSEDは維持

- verifierは依然process-free typed `unsupported`で、command/path/env等をrequestから構造的に排除する。
- p値は成功条件に戻らず、repository/leave-one-out cellsは必須のままである。
- profile DTO、obstruction occurrence/global macro unknown、context v2 exact discovery/closureは変更されていない。
- profile/context DTO hashを再計算し、`4b6cca93794ab03b1576e17d2e395ec43f731d316685247a89363ae2e840dd96` / `7c4ceca165588cd38b28cc6882bb68a1ff4040dbb19ee34dfd792216d70bbf26` と本文値が再度一致した。

### D3-N3 — utility judgeの非権威性は明示されている

judgeはusable proxyであり、defect truth、verification、evidence support、human acceptanceではないこと、model-family correlated errorを持つことがREADME:55-56,120-122、preregistration:340-372,901-906、PROTOCOL:234-241,384-391、POWER:21-29,220-225に明記される。primaryへ入れたことによる意味の上限自体はlaunderingされていない。

### D3-N4 — 実用性ゲート#9は契約として閉じた

ADR §11 はchecked-in request、provider-free exact commands、exit 0/0/0/20/0、fresh output root、exact artifact names、literal expected hashes/lengths、read-only stdlib verifier、second-run refusalとmtime/bytes不変、異なるpathのclean throwaway clones二つでのCIを必須化した（ADR:1039-1136,1260-1264）。実装・golden値は未作成だが、「契約実装時」の#9は達成へ更新できる。

### D3-N5 — 反論入力なし

指定された `tmp/orchestration/F1/`、`F2/` にレビュー可能な反論ファイルはなかったため、反論への採否判定はない。

## 収束判断

**収束しているか: NO。**

profile、projection、enumeration closure、security、Stage 0、time accounting、reproducibilityは本質的に収束した。しかし今回もBLOCKINGは4件で、中心問題は3 round連続してprimary endpoint/arm comparisonに残った。D1の非対称abstentionを対称化するとD2でblanket abstentionへ空洞化し、それをopaque schema・judgeで塞ぐとD3ではsource本文欠落、loss-opportunity非対称、judge pair型欠落へ移った。件数だけでなく、同じ「何をusableと数え、両armへ同じ機会で測るか」が形を変えている。

この部分は局所patchを続けるより設計方針を変えるべきである。推奨は次の二層化である。

1. deterministic engineering endpointはschema/source/hash closureとstructured completionだけを測り、実益を主張しない。
2. practical utility endpointはactual source bytesを与え、abstentionを別cellへ分離し、paired opaque recordsを二者の独立blinded human adjudication（または明示的に限定されたjudge study）で評価する。task-blocking lossの機会はpaired designで一致させる。

一つのbooleanへ mechanical formatting、semantic usefulness、abstention opportunity、LLM judge availabilityを継ぎ足す方針は、修正ごとに新しい測定経路を作っている。D3-B1〜B4を同時に一つのendpoint redesignとして閉じない限り、次roundでも同じ轍になる可能性が高い。

## ユーザー定義の実用状態9項目（契約実装時）

| # | 実用性ゲート | 判定 | 根拠・限界 |
| ---: | --- | --- | --- |
| 1 | local Rust repo + base/head → deterministic ingest → versioned universe | **達成** | request v2、profile/rule/extractor/context identities、deterministic rebuildを固定。 |
| 2 | fixture固有でない Relation / Path / Invariant obligation | **達成** | real Rust accepted `calls` relationからD obligationを生成し、fixture/repository分岐を禁止。 |
| 3 | obligationごとのbounded projectionとsource/loss/hash | **達成** | subject-first windows、candidate denominator、unknown/loss、projection hashをexact化。D3-B1は評価packetへのbytes搬送欠落で、canonical projection自体の欠落ではない。 |
| 4 | structured claim/abstention; raw prose非canonical | **達成** | closed dispositionとraw/parsed hashを要求。primary utilityの妥当性はD3-B1〜B4で未成立。 |
| 5 | claim/evidence/verification/human decision分離; unsupported/stale/inconclusive保持 | **達成** | non-authority ceiling、typed incomplete/unsupported、authority record生成禁止を維持。 |
| 6 | allow-list + workspace-scoped verification; reviewer arbitrary shell禁止 | **未達** | reviewer tool/shellは禁止だがworkspace verifierは常にunsupported。successor sandbox ADRが必要。 |
| 7 | 短いPR report + auditable JSONを一つのCLI workflowで生成 | **達成** | run-v2 auditからclosed report manifest/Markdown、CLI major dispatch、quickstart artifact layoutを契約化。 |
| 8 | provider不要quickstart + real-model adapter | **達成** | `deterministic.abstain@1` とCodex/Claude observer pathsをv2 unionへ置く。 |
| 9 | clone後に文書どおり再現可能 | **達成** | ADR §11とRequired verificationが二つのclean clone・exact command/hash/exit/artifact contractを固定。 |

**未達のまま残る実用性ゲート: 1 / 9（#6）**

## 最終判定

**この契約を凍結して実装に進んでよいか: NO**

BLOCKING 4件が残る。修正範囲の多くは収束したが、primary utility designは収束していない。
