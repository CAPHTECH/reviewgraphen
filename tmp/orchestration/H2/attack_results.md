# H2 evaluator attack results

実行日: 2026-08-23。すべて fresh temporary evaluator copy、raw model bytes、または ledger/seal を再計算した hostile run copy に対して実行した。production mutation は確認後に temporary copy ごと削除しており、作業ツリーの production file は戻し操作を必要としない。

| 攻撃ID | 分類 | 実行方法 | 結果 |
|---|---|---|---|
| M01 | 実装が防ぐ | source ceiling を 65,537 へ実変異し、65,536/65,537 byte 境界を実 packet constructor で実行 | 防げた |
| M02 | 実装が防ぐ | judge total floor を 5 へ実変異し、total=5 と valid total=6 を reconciliation | 防げた |
| M03 | 実装が防ぐ | permutation bit を常時 0 へ実変異し、bit 0/1 task identities を照合 | 防げた |
| M04 | 実装が防ぐ | pair signature の question ID を定数化し、同一 reason・異なる question を照合 | 防げた |
| M05 | 実装が防ぐ | support IDs を先頭1件へ切り詰め、2件対1件の loss identity を照合 | 防げた |
| M06 | 実装が防ぐ | text upper bound を +1 へ実変異し、512/513 byte 境界を実行 | 防げた |
| M07 | 実装が防ぐ＋監査 | orphan payload check を無効化し constructor oracle、別途 reseal 済み run corruption を verify-run | 防げた |
| M08 | 実装が防ぐ＋監査 | raw hash retention を null 化し実 execution oracle、別途 raw.bin を改変・reseal | 防げた |
| M09 | 実装が防ぐ | source role に unknown を許す実変異を行い、changed と unknown の混在 obligation を検証 | 防げた |
| M10 | 実装が防ぐ | runtime provenance を bundle preimage へ入れる実変異を行い、cross-provenance identity を照合 | 防げた |
| P01 | 表現不可能＋監査 | launch/schema に raw hash field がないことを確認し、raw.bin 改変を reseal 後 verify-run | 防げた |
| P02 | 表現不可能＋監査 | launch に packet field がないことを確認し、packet instruction 改変を reseal 後 verify-run | 防げた |
| P03 | 表現不可能＋監査 | launch に payload field がないことを確認し、duplicate payload を reseal 後 verify-run | 防げた |
| P04 | 表現不可能 | CLI/launch/obligation に loss/opportunity/eligibility 入力がないことを surface 検査 | 防げた |
| P05 | 表現不可能＋監査 | production が reverse-map artifact を読まないことを確認し、map swap を reseal 後 verify-run | 防げた |
| P06 | 実装が防ぐ | raw judge dimension に boolean を供給 | 防げた |
| P07 | 実装が防ぐ | raw judge score に unknown sibling を供給 | 防げた |
| P08 | 表現不可能 | primary command 不在を確認し、raw judge output の1 score 欠落を whole-batch decode | 防げた |
| N01 | 表現不可能 | mechanical command と public nested intermediate schema の不在を CLI/schema surface 検査 | 防げた |
| N02 | 実装が防ぐ | judge scores を scalar array に置換して raw decode | 防げた |
| N03 | 表現不可能 | source status field が launch/obligation に存在しないことを surface 検査 | 防げた |
| N04 | 表現不可能 | status-record object が launch/obligation に存在しないことを surface 検査 | 防げた |
| N05 | 実装が防ぐ | frozen source path を絶対 path に置換して prelaunch validator を実行 | 防げた |
| N06 | 実装が防ぐ | retained raw bytes を唯一の引数として reviewer decode/hash を実行 | 防げた |
| N07 | 表現不可能 | caller loss/opportunity/packet input の不在を surface 検査 | 防げた |
| N08 | 監査 | payload unknown sibling を追加して ledger/seal 再計算後 verify-run | 防げた |
| N09 | 監査 | source unknown sibling と rehash を行い ledger/seal 再計算後 verify-run | 防げた |
| N10 | 監査 | loss unknown sibling と rehash を行い ledger/seal 再計算後 verify-run | 防げた |
| N11 | 表現不可能＋監査 | judge から packet が戻らない surface を確認し、candidate nested packet を改変・reseal | 防げた |
| N12 | 表現不可能＋監査 | judge から binding が戻らない surface を確認し、nested binding view を改変・reseal | 防げた |
| N13 | 実装が防ぐ | mechanical failure から forced-zero を構成し、contradictory mechanical artifact を reseal 後 verify-run | 防げた |
| N14 | 実装が防ぐ | raw judge verdict に文字列 `false` を供給 | 防げた |
| N15 | 実装が防ぐ | raw judge batch ID を foreign ID に置換 | 防げた |
| N16 | 実装が防ぐ | frozen obligation の required ID を重複 | 防げた |
| N17 | 表現不可能＋実装が防ぐ | source-request input 不在を確認し、private constructor に duplicate SourceKey を注入 | 防げた |
| N18 | 実装が防ぐ | non-UTF-8 blob を repository outcome として渡し、単一 aggregate loss/support identity を検証 | 防げた |
| N19 | 実装が防ぐ | frozen span start=0 を prelaunch validator へ供給 | 防げた |
| N20 | 実装が防ぐ | depth 33 と depth 2,000 の raw JSON を decoder へ供給 | 防げた |
| N21 | 実装が防ぐ | duplicate JSON key を restricted parser へ供給 | 防げた |
| N22 | 実装が防ぐ | integer 2^53 を restricted parser へ供給 | 防げた |
| N23 | 実装が防ぐ | non-ASCII object key を restricted parser へ供給 | 防げた |
| N24 | 実装が防ぐ | threshold production mutant を含む evaluator copy で complete freeze gate を実行 | 防げた |
| N25 | 実装が防ぐ | frozen instruction file を実変更し、packet authority oracle と portable bundle hash を照合 | 防げた |

Named attacks: 43、通った攻撃: 0。

## 独立変異

| 変異ID | 分類 | 実行方法 | 結果 |
|---|---|---|---|
| I01 | 独立実変異 | model byte limit 1,048,576 → 1,048,577 | 検出 |
| I02 | 独立実変異 | model nesting limit 32 → 33 | 検出 |
| I03 | 独立実変異 | reviewer observation sorted/unique check を無効化 | 検出 |
| I04 | 独立実変異 | pair comparability を常時 true 化 | 検出 |
| I05 | 独立実変異 | permutation hash low bit を digest 先頭 byte へ変更 | 検出 |
| I06 | 独立実変異 | judge per-dimension floor 1 → 0 | 検出 |
| I07 | 独立実変異 | primary conjunction から failure-code emptiness を除去 | 検出 |
| I08 | 独立実変異 | repository absolute/empty-component path rejection を除去 | 検出 |
| I09 | 独立実変異 | freeze symlink rejection を無効化 | 検出 |
| I10 | 独立実変異 | instruction file load を hard-coded shadow literal へ置換 | 検出 |

独立変異: 10、未検出: 0。生成器2回の出力は byte-identical。runtime provenance を変更しても `evaluator_bundle_sha256` は不変だった。
