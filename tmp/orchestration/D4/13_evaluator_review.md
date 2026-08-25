# D4 独立 evaluator review

判定: **BLOCKING 9 / SHOULD-FIX 5 / NOTE 4**。**評価器を凍結してよいか: NO**。

27 tests、52 vectors、16 mutations の表示値は再現した。しかし、現在の suite は evaluator の実際の command/closure contract を検査していない。独立に production code のコピーへ10変異を入れたところ8変異が27/27を維持し、無変異実装への直接プローブでも8種類の不正入力が成功扱いになった。したがって「tests pass」と「§1–§14に準拠」は分離して判断する必要がある。

## 集計

| 項目 | 結果 |
| --- | ---: |
| 既存 unit tests | 27 / 27 pass |
| CLI `verify-reference-vectors` | 52 / 52 と表示 |
| §10 静的 expected の独立照合 | **52 / 52 一致、0 不一致** |
| CLI `run-mutations` | 16 / 16 と表示 |
| §11.2 mutation class の十分な検出 | 2 adequate / 10 partial / 4 inadequate |
| 独立 production-code mutations | 10 |
| そのうち既存27 testsに検出されなかったもの | **8** |
| 無変異実装への独立 negative probes | 8 / 8 が不正に受理 |

`§10 静的 expected 不一致=0` は、ベクトルファイルの表値が書き換えられていないという意味に限る。各ベクトルが完全な DTO/CLI 経路で実行されているという意味ではない。

## BLOCKING findings

### D4-B1 — 16個の「closed schema」が全て空の placeholder で、command DTO decode がない

`evaluator/schemas/*.json` は16ファイル全て同じ103 bytesで、`properties:{}`、`additionalProperties:false`、`required`なしである。実装もこれらをロードせず、`cli.py:78-84` は parsed dict を直接各関数へ渡す。`build-pair` は仕様の `experiment_id/unit_id/comparison_manifest_id/hidden_task_binding_core` を受けず caller-supplied `task_id` を要求し、他commandもexact two、same-task、closed unionをdecodeしない。これは §1、§2.1、§13 の境界そのものが未実装である。

### D4-B2 — total loss derivation / paired opportunity は helper に孤立し、pair build と mechanical scoringに接続されていない

`loss_registry.py:32-67` に部分的な導出関数はあるが、`cli._pair` (`cli.py:31-45`) はそれを呼ばず、callerの `declared_losses` をそのまま packet へ渡す。出力には `pair_id`、closure record、completed hidden binding、single paired-opportunity recordもない。`validate_serialized_losses` は単なる equality helperで、production commandから一度も呼ばれない。`mechanical.score` も `pair_opportunity.comparable` を信頼し、reconstruction/hashを検証しない。D3-B2の「唯一のtotal function」「A/B equal opportunity」は実行経路上に存在しない。

### D4-B3 — packet/source contract は hashを再計算すれば偽造でき、path exceptionもtree-closedでない

`mechanical.py:42-50` は packet hash と inventory bodyの `canonical_sha256` は見るが、packetのschema、frozen instruction、complete response schema、`source_inventory_id` preimageを検証しない。独立プローブではそれら4箇所を偽造しhashを再計算しても failure codeは空だった。`validate_payload_closure` はduplicate source/payload、順序、unknown/extra fieldsを閉じず、同一payload recordの重複を受理した。さらに `packet.py:28` はcaller-supplied pathを無条件にsemantic-scan免除し、§3.4/§3.5が要求する pinned tree/blob manifestとのexact path closureがない。D3-B1は本文搬送だけ実装され、authority/graph/closed-schema closureは未完である。

### D4-B4 — raw/parsed/hidden-binding/pair-opportunity hash closure がなく、mechanical scoreを偽造できる

`mechanical.py:35-76` は `raw_response_base64` をdecodeもparseもせず、`raw_response_hash` はnon-emptyかだけを見る。`hidden_binding_hash` は不一致を検出せず、存在しなければscorer自身がcaller bindingをhashする。`pair_opportunity` もcaller値をhashするだけで再構成しない。このため failure orderにある `source_blob_base64_invalid`、`hidden_binding_mismatch`、`pair_opportunity_mismatch` はproduction mechanical pathから生成されない。独立プローブではraw bytesと無関係な `sha256:not-the-hash-of-raw` が `hashes_retained=true`、failureなしになった。

### D4-B5 — judge batchのsealed permutation/reverse map/closed outputが閉じていない

`cli.py:81-83` はfrozenであるべき seedを入力の `judge_permutation_seed` から受け取る。`reconcile` (`judge_batch.py:75-102`) は `reverse_map_sha256`、reverse mapの `batch_id`、batch ID preimage、candidate ID preimage、batch hashを再計算しない。stale sealのままreverse-mapのarmだけ交換した独立プローブが両candidate validで通り、arm attributionを反転できた。またPythonの `bool` が `int` のsubclassであるためdimensionsに `true,true,2,2` を入れてもpassし、top-level/score-level extra fieldsも無視された。これはD3-B3のsealed caller mapとclosed judge outputを破る。

### D4-B6 — primary scorerがexact two / same task / complete mappingを要求しない

`primary.score_primary` (`primary.py:11-28`) はmechanical recordsを任意長でloopし、judge resultをarm IDだけでdict化する。duplicate arm、片腕、cross-task、foreign candidate/batchをrejectしない。独立プローブではmechanical 1件 + judge 1件からprimary score 1件を正常生成した。`validate_primary_score` はproduction `score_primary` の入力検証として呼ばれない。

### D4-B7 — generated fixturesは仕様が要求するscored fixture/expansionではない

`fixtures.generated.json` は10個のcase名だけ、`mutations.generated.json` は16個の名前とcode名だけで、A/B packet、reviewer execution、expected mechanical/primary resultを含まない。`reference_vectors.generated.json` はstatic vectorの直接コピーで、§11.1の「full DTOs and hashesへの52 expansion」ではない。`generate_fixtures.py:9-12` はstaticファイルを読むため、§1でpure library entry pointとされた責務にも反する。byte-identicalであることは、不足した同じbytesを再生成できることしか示さない。

### D4-B8 — `run-mutations` は実装mutation testではなく、表示する16/16は検出力を表さない

`cli.py:70-73` はmutation manifestの長さをpassed件数として表示し、1 test module全体の成否を全16件へ複製する。多くのtestはproduction実装を変異させず、deep-copyした値や独立lambda/定数式を負例にする。特に normalization (`test_mutations.py:125-143`) とthreshold (`:145-149`) はproduction functionを壊していない。独立にproductionコピーへ入れた10変異のうち、pair signatureからqidを落とす、support IDsを1件へ切る、judge total閾値を6→5にする、permutationを常にfalseにする等8件が27/27を維持した。D3-B4が要求する「異なる準拠実装の結果を固定」は達成していない。

### D4-B9 — freeze protocolはrequired hashes/test gate/symlink rejectionを実装せず、preregistrationも未freeze

`freeze_manifest` (`freeze.py:46-53`) の出力は6 keysだけで、§12の `generated_fixture_inventory_sha256`、`reference_vector_set_sha256`、`mutation_manifest_sha256` がない。freeze前にcomplete reference/mutation suiteを実行せず、generated byte checkだけでmanifestを作る。`file_records` は `path.is_dir()` を `path.is_symlink()` より先に判定するため、symlink directoryを黙って除外した独立プローブも成功した。`preregistration.json:150-156` の4 hashesとmanifest pathは全てnullである。現状でfreeze round-tripがpassしても§12のfreeze gateではない。

## SHOULD-FIX findings

1. **D4-S1 — frozen dataがruntime authorityになっていない。** instruction、registry、rubric dimensions、failure orderがPythonへ重複hard-codeされ、`data/*.json` はfreeze対象でしかない。dataだけ変わった未freeze実行でもcommand動作は変わらない。
2. **D4-S2 — validationがtotalでない。** DTO decodeがないため、例えばnon-dict admitted sourceは `source.get` でuncaught `AttributeError`になり得る。§8の「total after DTO decoding」を担うdecode層が必要である。
3. **D4-S3 — `binding_view` が入力を変更する。** shallowに取り出したlistへin-place `sort` (`judge_batch.py:27-31`) を行い、callerのhidden bindingも並べ替える。pure functionとして不要なalias mutationである。
4. **D4-S4 — unknown failure codeが黙って消える。** `ordered_codes` はclosed list外のcodeをrejectせずdropする。failure data fileとのbyte-equality/closednessも検査しない。
5. **D4-S5 — `stdlib_only` は自己申告。** 現コード/testのimportを走査した範囲ではstdlib/local moduleのみで適合していたが、`freeze._runtime` は常に `stdlib_only:true` を書くだけで依存を検証しない。

## NOTE

1. **D4-N1:** `canonical.py` のrestricted JCS subsetは、ASCII key、duplicate reject、float reject、safe integer、NUL/surrogate reject、UTF-8/sorted compact JSONについて仕様と一致する。command DTOがこの入口を十分使っていないことが問題である。
2. **D4-N2:** `source_payload.extract` のline/LF/CRLF/EOF/UTF-8処理は§3.4と一致し、S01-S07のliteral hash/IDも一致した。
3. **D4-N3:** `textnorm.py` のNFKC→casefold→longest lexeme→punctuation/space→ASCII token→byte/token thresholdは§7と一致し、T01-T20のstatic expectationsも一致した。
4. **D4-N4:** 指示書の事前観測はruntimeをbundle hash外とするが、current `freeze.py:51-52` はruntimeをhash preimageへ含めている。これは§12本文には一致する一方、事前観測の「環境非依存」とは一致しない。予算算術 `10×90=900`、`40×90=3600` はm20文書と一致するが、このevaluator自体に90秒/one-batch消費のenforcementはない。

## §1–§14 適合表

| 節 | 判定 | 独立レビュー結果 |
| ---: | --- | --- |
| §1 closed surface | **PARTIAL** | module名/tree/stdlib importは概ね一致。pure generatorがfile I/Oし、schemasはplaceholder。 |
| §2 canonical/errors/DTO | **PARTIAL** | restricted JCSと38-code順は一致。top-level closed DTO、typed command errors、cross-field decodeは未実装。 |
| §3 packet/source | **PARTIAL** | exact source text/hashは入る。complete response schema、tree/path authority、closed/unique/sorted packet closureがない。 |
| §4 visible/hidden boundary | **PARTIAL** | reviewer output decoderは部分実装。disposition ID derivation、full hidden binding closure/hash checkがない。 |
| §5 registry | **PARTIAL** | 3 entriesのhelperはあるがfrozen dataを権威としてロードせず、production pair pathへ未接続。 |
| §6 total loss/pair | **OPEN** | helper単体のみ。build-pair/mechanicalはcaller serializationを受け入れる。 |
| §7 textnorm | **CONFORMING for specified algorithm** | 20 static vectorsとコード読解で一致。suite全体のmutation adequacyは別問題。 |
| §8 mechanical/primary | **OPEN** | raw/hash/binding/opportunity closureとexact-two primaryがない。 |
| §9 judge batch | **PARTIAL** | 2 opaque public candidates/basic permutationはある。seed/reverse seal/hash/closed DTOが未成立。 |
| §10 vectors | **STATIC DATA CONFORMING** | 52/52 table values一致。ただしfull execution coverageではない。 |
| §11 generated/mutations | **OPEN** | fixtureは名前一覧、mutationは多くがproduction mutationでない。 |
| §12 freeze | **OPEN** | 3 required hashes/test gate/symlink rejection/prereg sealがない。 |
| §13 CLI | **PARTIAL** | subcommand名は全てある。required DTO semantics、typed reject、acceptance criteriaは満たさない。 |
| §14 closure summary | **NOT ESTABLISHED** | B1部分、B2未接続、B3 seal欠落、B4 mutation holesのためsummaryを支持できない。 |

## D3-B1〜B4 収束判定

| D3 finding | 判定 | 根拠 |
| --- | --- | --- |
| D3-B1 source body / payload closure / property lens | **PARTIALLY-CLOSED** | strict UTF-8 excerpt bodyとhashは追加された。しかしclosed packet、unique graph、source-inventory ID、pinned path authorityが閉じず、任意caller pathがlens exceptionを得る。 |
| D3-B2 total loss / symmetric opportunity | **OPEN** | helperは存在するがpair-buildとmechanical scorerへ接続されず、caller loss/opportunityがauthorityになる。実際のspec A/B packetをproduction CLIで構成できない。 |
| D3-B3 one opaque 2-candidate batch / sealed map | **PARTIALLY-CLOSED** | basic 2-candidate batchとvisible `hidden_arm_id`除去はある。frozen seed、reverse seal、candidate/batch closure、exact result pairがなく、arm swapを受理する。 |
| D3-B4 exact normalization / convergence | **PARTIALLY-CLOSED** | text pseudocodeと20 vectorsは収束した。一方judge threshold、permutation、loss signature/support、freeze preimage等のproduction変異がsuiteを通り、evaluator全体は収束していない。 |

結論として、前2回の「修正で別の穴を作る」パターンは止まっていない。今回は仕様を大きく executable codeへ移したが、helper単体の正しさをcommand境界のclosureと取り違え、さらにtest countsが個々の実行検査を代替している。

## §10 static reference vectors — 1本ずつの照合

### TEXT_V1 — 20 / 20 match

| ID | 結果 | 照合した規範値 |
| --- | --- | --- |
| T01 | MATCH | 24 bytes、4 tokens、pass |
| T02 | MATCH | 23 bytes、同normalized text、fail |
| T03 | MATCH | 512 bytes、4 distinct、pass |
| T04 | MATCH | 513 bytes、fail |
| T05 | MATCH | 1024 bytes、pass |
| T06 | MATCH | 1025 bytes、fail |
| T07 | MATCH | exactly 3 distinct、pass |
| T08 | MATCH | 2 distinct、fail |
| T09 | MATCH | casefold後 `alpha beta gamma delta` |
| T10 | MATCH | NFKC全角後同値 |
| T11 | MATCH | `ﬁ`→`fi` |
| T12 | MATCH | Unicode Pをspace化 |
| T13 | MATCH | longest `source:abcdef` を除去 |
| T14 | MATCH | substring除去後 `prefix suffix` 非連結 |
| T15 | MATCH | lexeme NFKC+casefold |
| T16 | MATCH | adjacent removalがspaceを挿入 |
| T17 | MATCH | enum echoはempty/非substantive |
| T18 | MATCH | 非ASCII語をtokenに数えず1 distinct |
| T19 | MATCH | U+0000を`invalid_text_codepoint` |
| T20 | MATCH | unpaired U+D800を同error |

### SOURCE_V1 — 10 / 10 match

| ID | 結果 | 照合した規範値 |
| --- | --- | --- |
| S01 | MATCH | 20 bytes/two LF、literal payload/source hashes一致 |
| S02 | MATCH | `a\r\n` 3 bytes、CRLF保持、hashes一致 |
| S03 | MATCH | EOF final `b`、hashes一致 |
| S04 | MATCH | exact `b\n`、adjacent lineなし、hashes一致 |
| S05 | MATCH | `span_out_of_bounds` |
| S06 | MATCH | `span_invalid` |
| S07 | MATCH | `non_utf8_source` |
| S08 | MATCH | stale hashに`payload_hash_mismatch` |
| S09 | MATCH | unpadded base64に`source_blob_base64_invalid` |
| S10 | MATCH | payload/path exception、metadata leak rejectというstatic expected |

S10のstatic文字列は仕様と一致するが、実testはpathがpinned tree由来かを確認しないためimplementation conformanceは不合格である。

### LOSS_V1 — 10 / 10 match

| ID | 結果 | 照合した規範値 |
| --- | --- | --- |
| L01 | MATCH | comparable、eligible 0、routine false |
| L02 | MATCH | source/source、eligible 1 |
| L03 | MATCH | reference/reference、eligible 1 |
| L04 | MATCH | projection/projection、eligible 1 |
| L05 | MATCH | source+reference、eligible 2、sorted reasons |
| L06 | MATCH | source/reference、noncomparable、eligible 0 |
| L07 | MATCH | source/(source+reference)、noncomparable |
| L08 | MATCH | 2 support IDs対1、aggregate 1 opportunity |
| L09 | MATCH | unknown statusはclosed-input rejection |
| L10 | MATCH | serialized eligible flipはpair mismatch |

### JUDGE_V1 — 12 / 12 match

| ID | 結果 | 照合した規範値 |
| --- | --- | --- |
| J01 | MATCH | `1,1,2,2` total 6、both pass |
| J02 | MATCH | bit 1、reverse-map only |
| J03 | MATCH | duplicate candidate、whole batch invalid |
| J04 | MATCH | missing candidate、whole batch invalid |
| J05 | MATCH | third candidate、whole batch invalid |
| J06 | MATCH | echoed hash change、whole batch invalid |
| J07 | MATCH | total mismatch、whole batch invalid |
| J08 | MATCH | total 4は当該candidateだけthreshold fail |
| J09 | MATCH | one dimension 0はcandidate fail |
| J10 | MATCH | hidden arm injectionはjudge input reject |
| J11 | MATCH | valid + exact forced-zero、independent |
| J12 | MATCH | unavailable、both fail/no retry |

## §11.2 mutation matrix の検出力

`restore` はproduction fileの復元ではなく、多くがdeep-copy/local値のbaseline比較である。従ってここでは「実際のproduction変異を戻すとpass」の証明とは数えない。

| # | Class | 変異は実際に適用? | 規定結果をassert? | restore/baseline | 判定 |
| ---: | --- | --- | --- | --- | --- |
| 1 | D3-B1 body absence | packet値から`payloads`削除 | `payload_missing`。規定のclosed schema failureではない | baseline closure確認 | **INADEQUATE** |
| 2 | D3-B1 body tamper | payload textを変更 | `payload_hash_mismatch`をassert | baseline closure確認 | **ADEQUATE (代表1変異)** |
| 3 | D3-B1 orphan/foreign | copied payloadへforeign ID | graph closure codeをassert | baseline closure確認 | **ADEQUATE** |
| 4 | D3-B1 lens | metadataへliteral注入 | leakとpayload exceptionをassert | baseline確認 | **PARTIAL**: exact tree-closed pathを検査せず逆に任意pathを許す |
| 5 | D3-B2 registry | input statusをunknown化 | helperのTypedErrorのみ | valid helper callあり | **INADEQUATE**: registry file delete/add/rename/hashを変異していない |
| 6 | D3-B2 derivation | visible flagをflip | disconnected equality helperがfalse | original equality確認 | **INADEQUATE**: production pair/mechanical gateに未接続 |
| 7 | D3-B2 symmetry | helper inputのreason変更 | noncomparable/flags false | comparable baselineあり | **PARTIAL**: A/B packetsと両arm mechanical zeroをassertしない |
| 8 | D3-B2 routine | routine-only basisをscore | `routine_loss_only`/zero | 後段はloss削除のみ | **PARTIAL**: mixed routine/task未検査、restore後の全pass未確認 |
| 9 | D3-B3 arity | responseを2→1 | both invalid | 2件baseline pass | **PARTIAL**: 0/duplicate等をclass testで網羅せず |
| 10 | D3-B3 closure | response echo hash 1個変更 | both invalid | baseline pass | **PARTIAL**: candidate/batch/reverse seal closureなし |
| 11 | D3-B3 blindness | public candidateへhidden key注入 | both invalid | explicit baseline assertなし | **PARTIAL**: A/B/baseline/ReviewGraphen/control variants未検査 |
| 12 | D3-B3 permutation | response score順だけreverse | both invalid | baseline生成 | **PARTIAL**: reverse-map seal mutationをせず、実装はstale sealを受理 |
| 13 | D3-B4 normalization | productionでなく別lambdaを評価 | static期待値との差だけassert | production restoreなし | **INADEQUATE**: casefoldの外部試行のみproduction mutation evidence |
| 14 | D3-B4 thresholds | productionでなく定数式を評価 |式のtrue/false | production restoreなし | **PARTIAL**: 独立total 6→5 mutantが生存 |
| 15 | Existing closure | foreign source helperを直接call | foreign sourceだけassert | baseline helperあり | **PARTIAL**: unknown role/extra field mutantが生存、loss/rangeも不足 |
| 16 | Endpoint completed | validation helperのboolをflip | semantic errorをassert | baseline helperあり | **PARTIAL**: exact-two/cross-task production endpoint未検査 |

## 独立に仕掛けたproduction-code mutations

全て `/tmp` の各独立コピーで行い、repositoryのevaluator原本は変更していない。同じ27-test suiteを各copyで実行した。

復元確認: 原本evaluator tree（`__pycache__`/`.pyc`除外）のcontent-manifest SHA-256は作業前後とも `7a45f79a70617a72328343972185fc84ae81928690d001768ff86df1005903d4`。`git diff -- benchmarks/m20-changed-public-callee-utility-v1/evaluator` と `git diff -- tmp/orchestration/D4` はともに出力なしだった（対象tree自体がuntrackedなので、前者はtree digestを主証拠とした）。

| # | Production mutation | 期待される検出 | suite結果 | 判定 |
| ---: | --- | --- | --- | --- |
| M01 | admitted source limit `65536→65537` | exact boundary test fail | 27/27 pass | **UNDETECTED** |
| M02 | judge total threshold `>=6→>=5` | JUDGE threshold mutation fail | 27/27 pass | **UNDETECTED** |
| M03 | sealed permutationを常にfalse | J02/permutation fail | 27/27 pass | **UNDETECTED** |
| M04 | pair signatureからquestion IDを削除しreasonだけ比較 | unequal question opportunity fail | 27/27 pass | **UNDETECTED** |
| M05 | aggregated support IDsを先頭1件へtruncate | L08 exact support/loss identity fail | 27/27 pass | **UNDETECTED** |
| M06 | text upper boundを`field_limit+1`へ緩和 | T04/T06 fail | suite fail (T04/T06) | DETECTED |
| M07 | orphan payload set-equality checkを無効化 | orphan mutation fail | suite fail | DETECTED |
| M08 | missing raw hash checkを無効化 | retained-hash negative fail | 27/27 pass | **UNDETECTED** |
| M09 | source roleに`unknown`を追加 | existing closure fail | 27/27 pass | **UNDETECTED** |
| M10 | evaluator bundle hashからruntimeを除外 | freeze preimage test fail (§12基準) | 27/27 pass | **UNDETECTED** |

**独立変異の未検出: 8 / 10。** M06/M07だけが検出された。

## 無変異実装への追加negative probes（新規欠陥の実証）

| Probe | 本来の結果 | 実結果 |
| --- | --- | --- |
| raw responseと無関係なnon-empty raw hash | `missing_raw_hash`またはhash mismatch | failureなし、`hashes_retained=true` |
| packet schema/instruction/response schema/inventory IDを偽造して再hash | closed/hash rejection | failureなし |
| identical payload recordを重複 | unique/graph closure rejection | `validate_payload_closure=[]` |
| caller-authored eligible task lossをpacketへ注入 | reconstruction mismatch | payload closureを通過、production reconstruction gateなし |
| reverse-map arm entriesを交換しseal hashをstaleのまま使用 | whole batch invalid | 両candidate valid、arm mapping交換 |
| judge dimensionsにJSON booleanを使用 | schema/type rejection | utility pass |
| judge top/scoreへunknown field追加 | closed schema rejection | 両candidate pass |
| mechanical/judge各1件だけでprimary scoring | incomplete pair rejection | primary score 1件を生成 |

## 実用性ゲート9項目への評価器の影響

| # | Gate | evaluator review |
| ---: | --- | --- |
| 1 | repo/base/head→deterministic ingest→versioned universe | evaluator外。今回のコードは達成を増減しない。 |
| 2 | fixture非依存Relation/Path/Invariant obligation | evaluator外。ただしempty hidden-binding schemaでは評価時のexact bindingを保証できない。 |
| 3 | bounded projection/source/loss/hash | **未達（評価部分）**。packet bodyはあるがtree/path、inventory ID、loss reconstruction、pair closureがない。 |
| 4 | structured claim/abstention、raw prose非canonical | **未達（評価部分）**。output DTOは部分実装だがraw/parsed bytes/hash対応を検証しない。 |
| 5 | claim/evidence/verification/human decision分離 | authority昇格は行わない点は維持。ただしcurrent scoreのclosureが弱く、監査可能な判定として凍結不可。 |
| 6 | allow-list/workspace scope、reviewer shell禁止 | packet instruction/tool-zero方針はあるが、policy/tool recordはcaller自己申告でhash-bound/closedでない。評価側は**部分的**。 |
| 7 | short report + audit JSON in one CLI | evaluator外のworkflow責務。current primary outputはexact pairを保証しないため下流監査入力として不足。 |
| 8 | provider-free quickstart + real adapter | evaluator外。fake/real双方が同じhash closureを通る保証はcurrent scorerにない。 |
| 9 | clone後のdocumented reproduction | **未達（評価器部分）**。generated bundle不足、freeze hashes欠落、prereg null、freeze gate不完全。 |

## 最終判定

**評価器を凍結してよいか: NO**

静的reference expectedは52/52仕様一致し、exact excerptとtextnormの核も改善している。しかし、command DTO、loss/opportunity derivation、mechanical hash closure、judge reverse-map seal、exact-two primary、generated fixtures、mutation adequacy、freeze gateという採点authorityの境界が未実装または未接続である。この状態でfreezeすると、同じbundleが仕様上異なる・不正な結果を成功として受理できる。
