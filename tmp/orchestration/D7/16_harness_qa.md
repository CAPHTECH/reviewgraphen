# m20 Python 評価ハーネス テスト品質レビュー

## 結論

**研究利用判定: NO**

現時点の evaluator は研究測定を開始してはならない。既知の 51 attack は全件検出され、raw model output decoder も強い。一方で、独自に追加した 8 個の production mutation が `unit tests + 52 reference vectors + 51 attacks` をすべて通過し、45 個の新規不正入力のうち 12 個が黙って受理された。さらに仕様が要求する 16,384-token preflight が実装されておらず、`verify-run` は再封印された不正 artifact の一部を正当と判定する。

加えて `preregistration.json` の 5 freeze hash と manifest path はすべて `null` のままである。これは仕様自身の pre-execution gate により、現状の production run を禁止する独立した NO 理由である。

集計値:

| 指標 | 結果 |
| --- | ---: |
| 既知 attack | 51/51 pass (`survived=0`) |
| 黙って受理された既知ケース | **0** |
| 独自の不正・境界ケース | 50（不正 45、正当境界 5） |
| 黙って受理された新規不正ケース | **12** |
| typed error でない新規拒否 | **4**（uncaught exception 3、argparse stderr-only 1） |
| 既存の独立 mutation | 10/10 detected |
| 今回追加した mutation | 0/8 detected |
| 検出されなかった mutation | **8** |

## 最重要 findings

### F1 — Blocker: acceptance gate を 8 個の有効な production mutation が通過する

次の各 mutation を `/tmp` の evaluator コピーに一つずつ適用し、各コピーで unit tests、52 vectors、`run-attacks` を実行した。すべて exit code `[0,0,0]` だった。

| ID | mutation | 破る契約 |
| --- | --- | --- |
| X01 | claim 配列の下限を 1 から 0 へ | claim は 1..3 件 |
| X02 | observation 配列の下限を 1 から 0 へ | observation は 1..8 件 |
| X03 | Git tree mode `120000` を許可 | symlink mode の拒否 |
| X04 | first-parent 照合を無効化 | head の第一親が base であること |
| X05 | diff window の前方展開を 3 行から 2 行へ | exact excerpt plan |
| X06 | primary utility 判定から judge verdict `usable` を除去 | 4 次元、total、verdict の conjunction |
| X07 | context/support observation も changed-source 根拠として扱う | changed source observation 必須 |
| X08 | source ceiling を admitted source 合計でなく unique payload 合計にする | dedup 後も source bytes を重複計上 |

該当 production seam は `model_boundary.py:26,39`、`repository.py:61,72,97`、`pipeline.py:116,124-132,174`。既知 operator はよく検出されるが、隣接する同等 mutation への一般化が不足している。仕様 §11.5 N24 と `must_coverage.md` の「uncovered ... = 0」は成立しない。

### F2 — Blocker: 16,384-token preflight が存在しない

仕様 §3.1/§3.2 は complete packet の 16,384-token preflight と、超過時に両 reviewer call より前で model-ineligible にすることを要求する。しかし evaluator production code に tokenizer、token count、16,384 ceiling の実装がない。存在するのは `pipeline.py:116` の admitted-source 65,536-byte ceiling のみで、`pipeline.py:209` は packet bytes をそのまま 900 秒 reviewer call に渡す。

これは単なる negative-test 不足ではなく current implementation の仕様不適合である。等しい ceiling、prelaunch eligibility、予算監査を実証できない。

### F3 — High: hostile-artifact verifier が再封印された不正 artifact 7 件を受理する

valid run を生成し、対象 JSON/bytes を変更した後に ledger と seal を正しく再計算した。`verify_run(...)["ok"]` は次の 7 件で `true` だった。

| ケース | 受理された不正状態 |
| --- | --- |
| A01 | `repository.json` の unknown field |
| A02 | reviewer `execution.json` の unknown field |
| A04 | `pair.json.arms[*]` の unknown field |
| A05 | `pair.json.slot_map[*]` の unknown field |
| A06 | reviewer request canonical bytes への末尾空白付加 |
| A07 | `pair_opportunity.eligible_question_ids` の forged value |
| A08 | pair 内 binding-view hash の forged value |

`artifacts.py:21-41` は ledger/seal の byte closure を確認するが、semantic replay を委譲された `pipeline.py:226-267` が nested records を十分 closed-decode/reconstruct していない。特に pair の `arms`、`slot_map`、eligible question IDs、pair 内 binding view を正典から再構成していない。仕様 §11.1 の「missing/extra paths and fields」「every ... relation」「replays scoring」を満たさない。

production は artifact を読み戻さないため arm score の直接汚染経路ではない。しかし第三者監査が改ざん artifact を genuine と認定できるので、研究証拠の完全性に対して High である。

### F4 — High: generated fixtures / mutation manifest に hollow な自己整合性がある

`freeze.py:8-15` は `vector_T01` から `vector_J12` までの 52 full-run case を追加するが、すべて同じ `("claim","pass")` transport を使う。vector ごとの hostile input/boundary behavior は full pipeline に注入されず、case 名だけが異なる。`attack_probe.py:260-263` の H3F01 oracle も required case 名と `vector_` prefix 件数だけを確認するため、この hollow expansion を検出しない。

また `attacks.generated.json` の operation/oracle は `semantic_mutation_or_hostile_boundary_<id>` と `section_11_named_oracle_<id>` という汎用文字列で、仕様 §11.2 が要求する semantic operation、具体的 named oracle、expected changed result を記録していない。実行側の `attack_oracles.py` には実 mutation があるが、生成 manifest 自体は十分な監査証拠ではない。

### F5 — High: Git object/parser の malformed input closure が不完全

独自 Git probes の結果:

| ケース | 結果 |
| --- | --- |
| symlink `120000` | typed reject |
| gitlink `160000` | typed reject |
| non-UTF-8 path | typed reject |
| slash/empty name、noncanonical mode | typed reject |
| SHA-256 の正当 object | accept（期待どおり） |
| 2 MiB の正当 blob | accept（明示 object-size ceiling なし） |
| batch size `01` | **silent accept** |
| batch size `+1` | **silent accept** |
| duplicate `tree` commit header | **silent accept** |
| non-ASCII batch OID header | **uncaught `UnicodeDecodeError`** |

`repository.py:34-38` は size token を `int()` へ直接渡すため、digits-only canonical decimal を要求しない。`repository.py:43-50` は最初の `tree` header だけを確認し、重複 header を拒否しない。`repository.py:33` の ASCII decode は `PreflightError` に totalize されていない。仕様 §1.2/§1.3 の malformed framing/object を typed preflight failure にする契約と不一致である。

大きな valid object は仕様に explicit ceiling がないため silent-invalid 件数には含めなかった。ただし `subprocess.run(..., stdout=PIPE)` で object 全体を一括保持するので、authority-controlled repository に対する memory/latency DoS は残る限界である。

### F6 — High: freeze の静的 security gate を qualified dynamic code が迂回する

一時 evaluator tree に以下を置くと `freeze.file_records()` は受理した。

- `import builtins; builtins.eval("1")`
- `import importlib; importlib.import_module("os")`

`freeze.py:32-44` は bare-name の `eval`、`exec`、`__import__` だけを拒否する。仕様 §12 の dynamic import/exec/eval rejection を満たさず、bundle 内に禁止コードを追加しても freeze gate を通せる。この 2 件を新規 silent accept に含めた。

### F7 — Medium: authenticated input と CLI error が total typed ではない

次の hostile inputs は semantic score を出さなかったが、要求された typed error にもならなかった。

- Git batch header の non-ASCII OID: `UnicodeDecodeError`
- `repository_allow_list` に object 要素: `TypeError`
- integer `obligation_id`: `AttributeError`
- `python3 -m evaluator run` の引数不足: exit 2、stdout 0 bytes、argparse usage は stderr のみ

`pipeline.py:9,30,35-44` は型確認前に hashing/`.encode()` を行う経路がある。`cli.py:17-18` は argparse の既定 error path を canonical typed record に変換しない。仕様 §13 の「Stdout is one canonical JSON record」と total preflight refusal を満たさない。

## §11 classification の検証

### UNREPRESENTABLE

P01-P05、P08、N01、N03、N04、N07、N11、N12、N17 の production-input 部分は分類どおりである。

- public CLI は `run`, `verify-run`, `generate-fixtures`, `verify-reference-vectors`, `run-attacks`, `freeze-manifest`, `verify-frozen` のみ (`cli.py:1,17`)。
- launch は 10 selector fields の closed object で、packet、payload、status、loss、opportunity、raw hash、candidate、primary score を持たない (`pipeline.py:11-15`)。
- packet/loss/binding/candidate/primary は `RUN` 内で生成される。judge output は score echoes だけで candidate packet/binding を返せない (`model_boundary.py:55-72`)。
- `ArtifactSink` に read method はなく、production `pipeline` は `verify_run` を import しない (`artifacts.py:3-17`, `pipeline.py:1`)。
- N17 の source request は external input API としては存在しない一方、内部 duplicate SourceKey は `_arm` で拒否される (`pipeline.py:68-73`)。

したがって「分類しただけで実際には旧 input route が残る」ケースは確認しなかった。ただし UNREPRESENTABLE+AUDIT の audit 側保証は F3 の追加ケースまで一般化できていない。

### REQUIRED / AUDIT

`python3 -m evaluator run-attacks` を実行し、全 51 rows が `passed=true`, `survived=0` だった。§11 の REQUIRED/AUDIT rows は実際の decoder、constructor、pipeline、hostile run verifier、または一時コピー mutation を通っている。既知ケースの silent acceptance は 0。

ただし、この結果は named operators の adequacy だけを示す。F1 の隣接 mutation 8 件と F3/F5/F6 の追加 hostile inputs により、negative/mutation universe の網羅性までは示さない。

## 新規 negative / boundary probes

合計 50 件を `/tmp` のみで実行した。正当境界 5 件はすべて受理された: multibyte 1024-scalar string、bracket を含む string、judge total 8、valid SHA-256 object、2 MiB valid blob。

不正 45 件の分類:

| 領域 | 不正件数 | typed reject | silent accept | その他の非 typed reject |
| --- | ---: | ---: | ---: | ---: |
| model decoder | 22 | 22 | 0 | 0 |
| repository | 10 | 6 | 3 | 1 |
| hostile artifact verifier | 8 | 1 | 7 | 0 |
| stage/obligation | 2 | 0 | 0 | 2 |
| freeze source gate | 2 | 0 | 2 | 0 |
| CLI argv | 1 | 0 | 0 | 1 |
| **合計** | **45** | **29** | **12** | **4** |

model decoder では空配列/空文字、bool-as-int、0/逆転 range、unsorted/duplicate observation/loss IDs、unknown nested field、1024/1025 scalars、JSON trailing value/float/unmatched close、wrong raw type、wrong expected kind、judge order/source/forced-zero を検査した。raw decoder は全件期待どおりであり、この領域は良好である。

## MUST/MUST NOT coverage 表の監査

`tmp/orchestration/H3/must_coverage.md` は uppercase normative occurrences を U01-U18 に対応させており、`rg` で確認した範囲では表にない uppercase MUST/MUST NOT はなかった。External gate の対象は概ね evaluator 外の repository scope、担当者 identity、versioning/governance、cross-document review であり、分類自体は妥当である。

ただし次を修正する必要がある。

1. 表の算術が誤っている。実際は Automated 11 rows、External gate 7 rows であり、記載の 12/6 ではない。
2. U10/H3F01 は fixture の semantic expansion を検証せず case 名だけを数えるため、Automated の主張が hollow。
3. U11 の実 attack runner は actual code を動かすが、generated attack manifest の operation/oracle/expected result は具体性を欠く。
4. U12 は列挙済み mutation には成立するが、F1 の 8 mutants がすべて通るため「security/authority/closure/determinism uncovered = 0」という総括は誤り。
5. U05 の「carried forward unchanged」は現 bundle 内の vector behavior と identity だけでは過去の independently reviewed source との不変性を証明しない。比較元 hash/source identity が必要。

## 仕様 §1〜§14 適合表

| 節 | 判定 | 根拠 |
| --- | --- | --- |
| 1 | PARTIAL | single `RUN` と raw-model trust surface、fixed Git argv は存在。malformed Git/typed totality と authenticated DTO type closure に穴。 |
| 2 | PASS | restricted JSON、duplicate/non-ASCII keys、int range、depth/size、closed reviewer/judge decode は追加 probes でも強い。 |
| 3 | FAIL | exact payload/source closure は概ね良好だが 16,384-token preflight が不存在。3-line/ceiling mutants も生存。 |
| 4 | PASS | reviewer output から hidden arm/binding を受け取る経路はなく、binding view は in-memory 生成。 |
| 5 | PASS | task-blocking status/loss は repository/extract/projection outcome から生成され、serialized status authority はない。 |
| 6 | PASS | paired signature、aggregate loss、eligible derivationの既知/独立 mutants は検出。 |
| 7 | PASS | 20 vectors と threshold mutants は有効。code-point invalid inputs も typed reject。 |
| 8 | PARTIAL | current conjunction は正しいが verdict/changed-observation mutants が生存し、test oracle が不足。 |
| 9 | PARTIAL | two-candidate batch、order/echo/type/threshold は実装。verdict を無視する mutation が生存し、pair artifact audit は不完全。 |
| 10 | PARTIAL | 52 vector runner は pass。ただし generated full-run 52 expansions は case-name-only。 |
| 11 | FAIL | one-way production は成立するが hostile `verify-run` が 7 件を受理し、generated attack metadata も hollow。 |
| 12 | FAIL | portable hash preimage は良好だが dynamic-code scan を迂回可能で、8 semantic mutants を許したまま acceptance が通る。 |
| 13 | PARTIAL | command surface/主要 exit は実装。argparse error は canonical JSON を出さず、token gate も acceptance にない。 |
| 14 | PASS | README/PROTOCOL/decision-boundary 文書は evaluator algorithm の正典を `EVALUATOR_SPEC.md` へ委譲している。 |

## 再現性

実行 runtime は CPython 3.13.5 / Unicode 15.1.0 で `runtime_compatible=true`。`identities(evaluator)` の portable bundle hash は mutation 作業の前後で一致した。

```text
before = sha256:b0d163682102e5d27698638e507a7f785741507d2f26f67ff4f1790bc4647266
after  = sha256:b0d163682102e5d27698638e507a7f785741507d2f26f67ff4f1790bc4647266
execution = sha256:e90669849b915a107e8ccc98efa75c05332a7e77639125098af74c881ce3ce77
files = 43
```

この host には別 Python runtime がインストールされていなかったため、異なる実 interpreter での再計算はできなかった。代わりに runtime provenance（version/executable/platform/Unicode 値）を monkeypatch して `identities()` を再計算し、bundle hash が不変であることを実測した。M10 と同じく portable preimage が runtime provenance を参照しないことは確認できた。これは cross-interpreter behavioral replay の代替ではなく、portable identity preimage の確認に限る。

`freeze_manifest()` は in-memory 生成まで成功したが、registered freeze hashes/path はまだ null である。また F1/F3/F6 があるため、現在の acceptance success は十分な freeze 根拠ではない。

## 実行記録

```text
python3 -m unittest discover -s evaluator/tests -t . -p 'test_*.py'
  17 tests, OK

python3 -m evaluator verify-reference-vectors
  52/52 pass

python3 -m evaluator run-attacks
  51/51 pass, survived=0

run_independent_mutations(...)
  10/10 detected, undetected=0

今回追加 mutation suite
  0/8 detected, undetected=8

python3 -m evaluator generate-fixtures --check
  ok=true
```

すべて `PYTHONDONTWRITEBYTECODE=1` で実行した。mutation は `/tmp` の evaluator コピーにのみ適用し、original evaluator bundle hash の前後一致で復元を確認した。`evaluator/` 配下へ作業ファイルは作成していない。

## NO を解除するための最小条件

1. §3 の tokenizer identity/count procedure と exact 16,384-token preflight を実装し、exact/+1 full-pipeline tests を追加する。
2. X01-X08 を恒久 mutation suite に追加し、各 operator が acceptance gate を落とすようにする。
3. `verify-run` で全 artifact record を closed-decodeし、pair/opportunity/binding/candidate/request bytes を正典から再構成する。A01-A08 を regression 化する。
4. Git batch size token を digits-only canonical decimal にし、commit header grammarと例外 totalizationを追加する。first-parent/symlink/context-window tests も追加する。
5. qualified `eval`/`exec`/`__import__`、`importlib`、undeclared process lookup を freeze scan で拒否し、hostile source-tree tests を追加する。
6. 52 generated full-run fixtures を case-name expansion ではなく各 vector の実 input/expected terminal/artifact へ接続し、attack manifest に concrete operation/oracle/expected change を記録する。
7. 上記後に全 gate を再実行し、新しい versioned bundle/execution hashes を preregistration へ non-null 登録してから測定を許可する。

修正後も残る正当な限界は、固定 runtime/tokenizer/judge への依存、local authenticated repository に対する巨大 object の resource risk、open-development corpus の非 holdout 性、utility judge が defect truth ではないこと、`direct_calls=partial` により全 caller coverage を主張できないことである。
