# D5 evaluator independent re-review

判定: **BLOCKING 8 / SHOULD-FIX 4 / NOTE 4**。**評価器を凍結してよいか: NO**。収束判断: **NO**。

40/40 testsとclass別16/16 mutation表示は再現した。D4の既知攻撃への耐性は明確に改善し、D4の10 production mutations中6件、8 negative probes中7件を今度は検出・拒否した。しかし、残りは**5/18件**がまだ通る。さらにD4で使わなかった新規攻撃25件を実行すると**22/25件**が通った。修正は既知症状に対して有効だが、nested DTO、source/loss authority、candidate content closure、primary provenanceという同じ境界原則へ一般化されていない。

## 集計

| 項目 | 結果 |
| --- | ---: |
| unit tests | 40 / 40 pass |
| `run-mutations` | 16 / 16 class pass表示 |
| D4 production mutations再実行 | 6 detected / **4 undetected** |
| D4 negative probes再実行 | 7 rejected / **1 accepted** |
| D4攻撃合計 | **5 / 18 が今も通る** |
| 新規攻撃 | **22 / 25 が通る** |
| runtimeだけ変えたfreeze実測 | file records同一、bundle hash不一致 |

復元確認: repository evaluator tree（`__pycache__`/`.pyc`除外）のcontent manifest SHA-256は作業前後とも `46a43dd906e3b92244201c67f3c3f51c7f5be68d97abf76fa2d6f2f34e919265`。全production/data変異は`/tmp`のisolated copyだけへ適用した。`git diff -- benchmarks/m20-changed-public-callee-utility-v1/evaluator` と `git diff -- tmp/orchestration/D5` はともに出力なしだった（対象がuntrackedなのでtree digestを主証拠とする）。

## 現在のBLOCKING findings

### D5-B1 — schemaは非空になったがnested contractは依然hollowで、decoderを迂回できる

16 schemaはtop-level `required` と `additionalProperties:false` を持つようになったが、各propertyはほぼ `{}` である。例えば `mechanical_request.v1.json` のpacket/binding/opportunity/execution、`pair_build_request.v1.json` のarms/core、judge candidates/scoresのnested shapeはschemaで制約されない。さらにschema engineは使われず、`decode_command` (`types.py:110-143`) もscore-mechanical/reconcile-judgeのnested値を検証しない。N01ではpacket=1、execution=7等のscalar envelopeをdecoderが受理し、N02ではjudge candidate `[1,2]` を受理した。closed schemaはtop-level field名だけでなく全nested DTOを閉じる必要がある。

### D5-B2 — source availabilityを二重自己申告させ、loss/opportunityを操作できる

§2.1/§6はsource availabilityをsource request unionと`EXTRACT`から再構成し、別のcaller source statusを受けない。しかし `_pair` はavailable source bytesをpacketへ入れる一方、loss derivationは別の `arm["status_records"]["sources"]` を信頼する (`cli.py:40-68`)。N03では同じsourceを「bytes available」かつ「status unavailable」と宣言し、source本文を持ちながらeligible source-blocking lossを両armへ生成できた。逆にN18ではavailable unionのnon-UTF8 extraction failureをstatus=availableのままにしてtask lossを生成しなかった。N19では仕様上reject必須の`span_invalid`もaccepted obstructionになった。これはtotal functionの入力authorityが一意でないためである。

### D5-B3 — packet closureはtop-levelだけ直り、source/payload/loss/path recordがclosedでない

`validate_packet_closure`はpacket/inventory top-level、ID/hash、array sort/uniquenessを追加したが、各payload/source/loss recordのexact field setを検証しない。N08/N09/N10でそれぞれextra fieldを追加し必要なinventory hashを再計算すると全てclosure passした。`build_packet`自体はduplicate source IDsを閉じず、N17で同じrequestを2回与えるとduplicate admitted sourcesを含むpair resultを正常生成した。N05ではprofile-normalized relative pathではないabsolute pathを受理した。pinned tree/blob manifestとのpath closureは依然存在せず、property-lens path exceptionもcaller pathへ与えられる。

### D5-B4 — mechanical hashは個別bytesを閉じるだけで、raw→parsedとloss/opportunity provenanceを閉じない

raw base64とraw hashの一致、binding/opportunityの自己hashは追加された。しかしraw bytesをJSON parseして`parsed_output`へ結び付けない。既存test baseline自身がraw=`raw-output`と別のstructured parsed objectでfailureなしであり、N06で再確認した。またscore-mechanical commandはpair-build artifactを要求せず、callerがpacket内eligible lossとcomparable opportunityを同時に捏造して各hashを再計算できる。N07/D4P4ではそのabstentionがfailureなしになった。`validate_serialized_losses`の修正testはこのproduction pathへ接続されていない。

### D5-B5 — judge sealはreverse mapを閉じたがcandidate内部contentをhash fieldへ再closureしない

frozen seed、stale reverse-map seal、response extra fields、boolean dimensionsは修正された。一方`reconcile`はcandidateのecho hash fieldsからbatch IDを再計算するだけで、full `candidate.packet`→`packet_sha256`、`binding_view`→`binding_view_sha256`、mechanical state/result→`mechanical_score_sha256`を再計算しない。N11はpacket instructionをstale hashのまま変更し、N12はbinding viewのruleをstale hashのまま変更したが、どちらも両candidate passした。N13ではfailure codeを持つ矛盾したmechanical recordをbooleansだけでjudgeableにし、public stateからfailureを消せた。

### D5-B6 — primaryは件数だけ閉じ、型・candidate/batch provenanceを閉じない

exact two/unique arm/same mechanical taskのguardは追加された。しかしnested score DTOはdecodeされず、`utility = bool(judge.get(...))` (`primary.py:27`) により文字列`"false"`がtrueになる。N14では両primaryが`completed=true`になった。さらにjudge resultのcandidate ID/batch ID/mechanical hashを元batch/mechanical recordへ結ぶ入力がなく、N15では完全にforeignなbatch/candidate IDsでも両primaryがcompletedになった。

### D5-B7 — generated fixturesとmutation suiteは依然spec expansionでなく、既知patchへのregression suiteに留まる

`fixtures.generated.json` は依然332 bytesのcase名一覧、mutation manifestも名前/code一覧で、§11.1のfull A/B DTO、expected mechanical/primary results、52 full expansionを持たない。`run-mutations`はclass別結果を返すよう改善したが、各classは同じbehavioral unit testでありproduction mutationを適用しない。D4 mutationのlimit 65536→65537、missing raw hash check削除、unknown role追加、runtimeのhash preimage除外は40/40を維持した。class別表示は検出力の一般証明ではない。

### D5-B8 — frozen dataはruntime authorityでなく、freeze gateも全acceptance testsを実行しない

`common_instruction.txt` をisolated copyで別文へ変更してもruntime `INSTRUCTION` は旧hard-coded文字列のまま、40/40 testsもpassした。dataはhash対象だが挙動のauthorityではない。さらに`freeze_manifest`はreference/mutationの2 modulesだけを実行し、command-boundary testsを実行しない。N24では`decode_command`を即returnへ破壊し、command-boundary testがfailすることを確認しながらfreeze manifest生成は成功した。required 3 hashesとsymlink checkは追加されたが、broken command evaluatorをfreezeできる。加えてpreregistrationの4 hashes/manifest pathは今もnullである。

## SHOULD-FIX / NOTE

### SHOULD-FIX

1. **D5-S1 — deep JSONがuntyped `RecursionError`でcrashする。** 2000 nested arraysを`parse_json_bytes`へ渡すと`CanonicalError`でなく`RecursionError`になった。CLIのcatch対象外で、入力境界のtotality/DoS耐性がない。
2. **D5-S2 — typed errorが空である。** `_pair`は多数のinvalid nested inputへ`TypedError([])`を投げ、CLIはstderrへ例外class名だけを書く。§2のordered `{code,json_pointer,detail_id}` recordを満たさない。
3. **D5-S3 — `stdlib_only`は依然自己申告。** current importsはstdlib/localだけだったが、freezeは依存を検査せず常にtrueを書く。
4. **D5-S4 — generatorのpure責務違反が残る。** `generated_values()`は`reference_vectors/*.json`をfile I/Oで読み、§1のpure library entry pointと一致しない。

### NOTE

1. **D5-N1:** duplicate JSON key、`2^53`、non-ASCII object keyの3攻撃は全て`CanonicalError`で拒否され、restricted JCS境界は維持された。
2. **D5-N2:** exact excerptとtextnormの既存§3.4/§7 conformanceに新しい反証は見つからなかった。
3. **D5-N3:** `binding_view`のsortはin-place alias mutationからassignment-based sortへ変わり、D4-S3は閉じた。unknown failure code rejectも入りD4-S4は閉じた。
4. **D5-N4:** D4の具体的症状に対するregression testsは有効であり、raw hash mismatch、packet top-level forgery、payload duplicate、stale reverse seal、bool dimensions、judge extra fields、one-arm primaryは今度は拒否された。

## D4 BLOCKING 9件の閉鎖判定

| D4 finding | 判定 | 再レビュー根拠 |
| --- | --- | --- |
| D4-B1 空schema / DTO decode | **PARTIALLY-CLOSED** | top-level requiredとdecoderを追加。しかしnested schemaは`{}`、nested scalar attacks N01/N02を受理。 |
| D4-B2 loss derivation未接続 | **PARTIALLY-CLOSED** | `_pair`からderiveを呼ぶようになった。しかし別source status自己申告、extract結果未反映、mechanical forged lossを許す。 |
| D4-B3 packet forgery / path exception | **PARTIALLY-CLOSED** | top-level constants/inventory preimage/duplicate payloadは閉じた。nested extra record、duplicate source、absolute/unpinned pathは通る。 |
| D4-B4 hash closure欠落 | **PARTIALLY-CLOSED** | raw bytes/hash、binding/opportunity self-hashを追加。raw→parsed、pair artifact provenance、loss reconstructionは未closure。 |
| D4-B5 judge seal | **PARTIALLY-CLOSED** | frozen seed/reverse seal/output closednessは改善。candidate packet/view/mechanical content hashを再計算せずstale contentを受理。 |
| D4-B6 primary exact-two | **PARTIALLY-CLOSED** | exact two/unique armを追加。bool型、candidate/batch/mechanical provenanceは未検証。 |
| D4-B7 fixture expansion | **OPEN** | generated contentはD4時点と同じcase名/vector copyで、full scored DTO expansionなし。 |
| D4-B8 mutation length表示 | **PARTIALLY-CLOSED** | class別表示は追加。production mutationではなく、D4 mutations 4件が生存。 |
| D4-B9 freeze hashes/tests/symlink | **PARTIALLY-CLOSED** | 3 hashes/test gate/symlink順は修正。gateがcommand testsを除外し、data driftとbroken decoderをfreeze可能。prereg hashesもnull。 |

## D4 SHOULD-FIX 5件の閉鎖判定

| D4 finding | 判定 | 実測 |
| --- | --- | --- |
| D4-S1 frozen data runtime authority | **OPEN** | `common_instruction.txt`を変更してもruntime behavior不変、40/40 pass。 |
| D4-S2 validation totality | **OPEN** | nested scalar decoder受理、deep JSONでuncaught recursion。 |
| D4-S3 binding view alias mutation | **CLOSED** | list fieldsは`sorted(...)`で新listへ置換。 |
| D4-S4 unknown failure code silent drop | **CLOSED** | `ordered_codes`がunknownをValueErrorでreject。 |
| D4-S5 stdlib assertion only | **OPEN** | actual importsは適合するが検査はなくliteral true。 |

## D4の10 production mutations再実行

全変異は`/tmp`の独立copyへ入れ、同じ40-test suiteを実行した。

| # | D4 mutation | D5 suite結果 | 判定 |
| ---: | --- | --- | --- |
| M01 | source byte limit `65536→65537` | 40/40 pass | **UNDETECTED** |
| M02 | judge total `>=6→>=5` | threshold test fail | DETECTED |
| M03 | permutation always false | low-bit test fail | DETECTED |
| M04 | pair signatureからqid削除 | qid test fail | DETECTED |
| M05 | support IDsを先頭1件へtruncate | support test fail | DETECTED |
| M06 | text upper bound `+1` | T04/T06 fail | DETECTED |
| M07 | orphan check無効 | mutation test fail | DETECTED |
| M08 | missing raw hash check無効 | 40/40 pass | **UNDETECTED** |
| M09 | source roleへ`unknown`追加 | 40/40 pass | **UNDETECTED** |
| M10 | bundle hashからruntime除外 | 40/40 pass | **UNDETECTED** |

**今も通る: 4 / 10。**

## D4の8 negative probes再実行

| # | D4 probe | D5結果 |
| ---: | --- | --- |
| P01 | rawと無関係なnon-empty hash | REJECTED (`missing_raw_hash`) |
| P02 | packet schema/instruction/response schema/inventory ID偽造+rehash | REJECTED |
| P03 | duplicate payload record | REJECTED |
| P04 | caller-authored eligible loss + comparable opportunity | **ACCEPTED、failureなし** |
| P05 | stale sealのままreverse arm swap | REJECTED |
| P06 | boolean judge dimensions | REJECTED |
| P07 | judge top/score extra field | REJECTED |
| P08 | one mechanical + one judge primary | REJECTED |

**今も通る: 1 / 8。D4攻撃全体では5 / 18。**

## 新規攻撃25件

`PASS`は「不正状態を受理した／untyped crashを起こせた」を意味する。

| # | D4で未実施の攻撃 | 実結果 |
| ---: | --- | --- |
| N01 | mechanical commandのnested packet/execution等をscalar化 | **PASS**: decoder受理 |
| N02 | judge candidatesをscalar `[1,2]` にする | **PASS**: decoder受理 |
| N03 | source bytes=available、別status=unavailable | **PASS**: 本文とeligible lossを同時生成 |
| N04 | status_recordsへunknown top-level field | **PASS**: 無視してpair生成 |
| N05 | absolute/unprofiled source path | **PASS**: packetへそのまま採用 |
| N06 | raw bytesとparsed outputを別内容にし、各hashだけ正しくする | **PASS**: mechanical failureなし |
| N07 | caller-generated eligible loss/opportunityでabstention | **PASS**: mechanical failureなし |
| N08 | payload record extra field | **PASS**: closure pass |
| N09 | source record extra field + inventory rehash | **PASS**: closure pass |
| N10 | loss record extra field + inventory rehash | **PASS**: closure pass |
| N11 | judge candidate packet contentをstale packet hashのまま変更 | **PASS**: 両candidate pass |
| N12 | binding view contentをstale binding hashのまま変更 | **PASS**: 両candidate pass |
| N13 | true booleans + nonempty mechanical failure codes | **PASS**: judgeableとなりfailureを隠す |
| N14 | primary `utility_judge_valid="false"` | **PASS**: truthy変換でcompleted=true |
| N15 | foreign batch/candidate IDsのjudge result | **PASS**: 両primary completed |
| N16 | duplicate required source IDs | **PASS**: duplicate support IDsをloss identityへ採用 |
| N17 | duplicate identical source requests | **PASS**: duplicate admitted sourcesをpair outputへ生成 |
| N18 | available unionのnon-UTF8 extraction failure、status=available | **PASS**: obstructionは出るがtask lossなし |
| N19 | `span_invalid` request (`start_line=0`) | **PASS**: rejectせずobstruction result |
| N20 | 2000-level nested JSON | **PASS**: untyped `RecursionError` |
| N21 | duplicate JSON key | DEFENDED: `CanonicalError` |
| N22 | integer `2^53` | DEFENDED: `CanonicalError` |
| N23 | non-ASCII object key | DEFENDED: `CanonicalError` |
| N24 | `decode_command`を即returnへproduction変異後freeze | **PASS**: command testはfailするがfreeze manifest生成成功 |
| N25 | frozen `common_instruction.txt`を変更 | **PASS**: runtime instruction不変、40/40 pass |

**新規攻撃で通った件数: 22 / 25。**

## §1〜§14 適合表（更新版）

| 節 | 判定 | D5評価 |
| ---: | --- | --- |
| §1 closed implementation surface | **PARTIAL** | module/tree/stdlib actual importsは適合。pure generator違反、data非authority、nested DTO hollow。 |
| §2 canonical JSON/errors/DTO | **PARTIAL** | JCS主要境界は強い。command nested decode、typed total errors、depth handlingが不足。 |
| §3 packet/source body/lens | **PARTIAL** | exact bodyとtop-level closure改善。record closedness、duplicate source、profile path authorityが不足。 |
| §4 visible/hidden boundary | **PARTIAL** | visible output decoderは維持。full hidden binding schema/hash/provenanceは未closure。 |
| §5 registry | **PARTIAL** | helperとpair接続あり。ただしdata file非authority、source status入力が二重。 |
| §6 total loss / pair opportunity | **PARTIAL** | pair-build内deriveは実装。extract-derived statusとmechanical reconstructionがなくcaller forged lossを許す。 |
| §7 text normalization | **CONFORMING** | 新しい反証なし。boundary mutantも検出。 |
| §8 mechanical / primary | **PARTIAL** | raw hash/exact two改善。raw→parsed、typed score/provenanceは未実装。 |
| §9 sealed judge batch | **PARTIAL** | frozen seed/reverse seal/output type改善。candidate内部hash closureなし。 |
| §10 reference vectors | **STATIC DATA CONFORMING** | D4の52/52照合を変更する差分なし。ただしfull DTO executionではない。 |
| §11 generated fixtures/mutations | **OPEN** | generated scored fixtures/full expansionsなし、production mutation gateでない。 |
| §12 freeze | **PARTIAL** | hashes/symlink/reference+mutation gate追加。全acceptance test gate/data authority/runtime portability未解決。 |
| §13 CLI | **PARTIAL** | surfaceとtop-level decode改善。nested inputs、semantic linkage、typed rejectionが不足。 |
| §14 closure summary | **NOT ESTABLISHED** | B1〜B4全てpartialで、same authority gapsが残る。 |

## D3-B1〜B4

| Finding | 判定 | 根拠 |
| --- | --- | --- |
| D3-B1 source body/payload/lens | **PARTIALLY-CLOSED** | body/hashとtop-level constantsは閉じた。nested record extra、duplicate source、unpinned path exceptionが通る。 |
| D3-B2 total loss/symmetric opportunity | **PARTIALLY-CLOSED** | pair helperは接続されたがsource statusが二重authorityで、mechanicalはcaller loss/opportunityを再構成しない。 |
| D3-B3 opaque 2-candidate/sealed map | **PARTIALLY-CLOSED** | stale reverse seal等は閉じた。candidate内部packet/view/mechanical contentのhash closureとprimary provenanceがない。 |
| D3-B4 exact evaluator convergence | **PARTIALLY-CLOSED** | text/Judge known boundariesは改善。しかしD4 mutations 4件、新規攻撃22件がsuiteを通り、別実装の余地は大きい。 |

## §12 runtime/hash問題と実用性ゲート#9

isolated copyでevaluator filesを一切変えず、`_runtime().platform`だけをsynthetic valueへ変更してmanifestを2回生成した。file recordsはbyte-identicalだったがbundle hashは次のように変わった。

- original runtime: `sha256:917fd460befae19ad041a59d876fe10b2742fe1c599359a56792af6a04d622c8`
- changed runtime: `sha256:b0e96fca5ca3997eeb8ce3ea05e7725171d06e21fa36b9689f528d77b968069a`

よってG5のspec gapは実害がある。§12の現文どおりなら、第三者はevaluator bytesを完全再現してもPython executable/platform/Unicode DBが違えばliteral bundle hashを再現できない。runtime compatibilityを別hash/gateとして記録する、またはreproducibility contractでexact executable artifactとplatformを取得可能に固定する設計判断が必要である。現状はprereg hashもnullで、portable clone reproductionの手順もないため、実用性ゲート#9の**評価器部分は未達**である。同一host・同一interpreter上の二clone再現だけなら可能だが、第三者再現一般を満たさない。

## 収束判断

**収束: NO。**

D4の具体的8 probesのうち7件を閉じた点は実質的改善である。しかし新規25攻撃中22件が通り、その中心は「top-levelだけclosedにしてnested authorityをcallerへ残す」「hash fieldを比較するがhash対象contentを再計算しない」「件数を検査するがprovenance/typeを検査しない」という同じ実装パターンである。既知attackへのpoint fixからcontract全体のdecoder/reconstructionへ移行しない限り、次の修正でも別nested fieldへ穴が移る。

## 最終判定

**評価器を凍結してよいか: NO**

freeze前に少なくとも、全nested DTOの単一decoder、source union/EXTRACTを唯一authorityにしたloss derivation、score-mechanicalでのpair artifact reconstruction、raw→parsed closure、judge candidate full-content rehash、primaryのtyped provenance、full generated fixtures、全acceptance testsを含むfreeze gate、frozen dataのruntime loadingを同時に閉じる必要がある。
