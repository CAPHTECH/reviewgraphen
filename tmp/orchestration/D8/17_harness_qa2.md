# m20 Python 評価ハーネス再レビュー

## 結論

**研究利用判定: NO**

報告用集計は **`NO / 4 / 12 / 14`** である。

- X04/X05/X07/X08: **4/4 検出**
- 表外で実適用した新規 adjacent mutation: **13 件中 12 件未検出**
- 前回未使用の新規不正入力: **37 件中 14 件を黙受理**、さらに 1 件は非 typed な `AttributeError`

既知の修正は有効である。61 attack は全件通り、前回の再封印 artifact 7 件と、残る Git/freeze/authenticated-input/CLI 9 件も今度は拒否された。F2 の token-budget 裁定も 5 文書では一貫し、equal token budget を主張していない。

しかし F1 の一般化は成立していない。仕様表のすぐ外側にある 12 個の有効な production mutation が `21 unit tests + 52 vectors + 61 attacks` をすべて通過した。また freeze security、transport metadata、authenticated obligation の新規不正入力に閉鎖漏れがある。加えて、新しい byte-budget 仕様が要求する `budget.json`、pair-wide sealed `model_ineligible`、closed `token_observation` は production pipeline に実装されておらず、現行テストはこの仕様不適合を検出しない。

## 実行結果

| 検査 | 結果 |
| --- | ---: |
| unit tests | 21 tests, OK |
| reference vectors | 52/52 pass |
| named attacks | 61/61 pass, `survived=0` |
| generated fixtures byte check | `ok=true` |
| X04/X05/X07/X08 の再適用 | 4/4 detected |
| 前回の再封印 artifact | 7/7 rejected |
| 前回の残り hostile regression | 9/9 typed/canonical reject |
| 新規 adjacent mutations | 1/13 detected、**12/13 survived** |
| 新規不正入力 | 22 typed reject、**14 silent accept**、1 untyped reject |

最終 evaluator identity は次のとおり。mutation はすべて `/tmp` のコピーに一件ずつ適用し、原本へ一時変更していない。

```text
files       = 48
bundle      = sha256:4cf90ddcbd92df245036e185bfae5edf26411c1656881d1e66f4ef4aea465052
requirements= sha256:41b1d1a61974544a28e2d6b9bd96e7eb62977742e08a137f09c092c06d433596
execution   = sha256:5c6766c8e0bd441003728c4aefd772ec038563ddc0a86e6b27c9eb8e4b32a980
```

## F1 — 既知修正は通ったが、一般化は未達

### 表の独立性

`evaluator/tests/spec_contracts.py:3-40` は production module を import せず、16 numeric、8 enum、5 conjunction、計 29 項目を手書き転記している。`derived_cases()` は exact/±1、allowed enum と invalid enum、各 conjunction term の欠落を 106 cases に展開する (`spec_contracts.py:43-59`)。production から表を生成する循環はない。実行 adapter が production 関数を呼ぶ構造も通常の black-box oracle として妥当である (`contract_runner.py:8-15,30-135`)。

X04/X05/X07/X08 は `attack_oracles.py:33-37` の production-copy mutation を直接再実行し、全件で oracle process が失敗した。したがって、この 4 件の修正は有効である。

### 表外 adjacent mutation

既存 mutation 定義にも 29-row 表にもない隣接 seam を選び、`/tmp` の fresh evaluator copy に一件ずつ適用した。各 copy で unit tests と public `run-attacks` の両方を実行した。

| ID | production mutation | 結果 |
| --- | --- | --- |
| Y01 | observation の `end_line >= start_line` を無効化 | **survived** |
| Y02 | reviewer `task_id` echo 照合を無効化 | **survived** |
| Y03 | reviewer `source_inventory_id` echo 照合を無効化 | **survived** |
| Y05 | judge `total == sum(dimensions)` を無効化 | **survived** |
| Y06 | forced-zero の `verdict == not_usable` を除去 | **survived** |
| Y07 | judgeable candidate の `score_source == judge` を無効化 | detected |
| Y08 | abstention reason と cited loss reason の一致を除去 | **survived** |
| Y09 | observation range の下限照合だけを除去 | **survived** |
| Y10 | tool call の policy failure 化を無効化 | **survived** |
| Y11 | provider truncation の policy failure 化を無効化 | **survived** |
| Y12 | `process_exit` の bool 拒否を除去 | **survived** |
| Y13 | judge batch ID preimage から output artifact hashes を除去 | **survived** |
| Y14 | raw Git tree component の `/` 拒否を除去 | **survived** |

Y06 は最初の並行 fixture 競合によるノイズを除いて単独再実行し、unit 21/21 と attacks 61/61 の通過を確認した。表外 mutation の確定値は **13 件中 detected 1、undetected 12** である。

### 表に残る空白

現行表 (`spec_contracts.py:5-39`) は代表的な scalar 境界には効くが、仕様の全境界・enum・conjunction を転記したものではない。少なくとも次が欠ける。

- observation の cross-field range、sorted/unique、admitted source range の上下限
- task/inventory ID、judge candidate order、各 hash echo
- judge total の dimension-sum 等式、forced-zero の source/total/verdict conjunction
- abstention loss の reason/eligibility/question/opportunity binding
- tool/truncation/process metadata の型と policy conjunction
- stable-ID preimage の全構成要素
- authenticated obligation の ID 型、role enum、projection hash closure
- artifact set、`budget.json`、token observation の closed schema と terminal-state variants
- freeze の dynamic-code/undeclared-executable rejection

表は「仕様からの独立転記」という方式自体は良いが、29 項目を完全な分母として扱う仕組みがない。仕様変更との drift 検出もなく、表にない規範は自動導出されない。今回の 12 survivors はその実害である。

## F2 — 文書裁定は誠実、ただし実装が裁定後仕様へ追随していない

### 5 文書の整合性

対象 5 文書は `README.md`、`PROTOCOL.md`、`EVALUATOR_SPEC.md`、`POWER_AND_DECISION_BOUNDARIES.md`、`preregistration.json` とした。記述は次の点で一致する。

- 唯一の enforced equal-input predicate は各 arm の `admitted_source_bytes <= 65,536`。
- evaluator-enforced input-token ceiling と tokenizer preflight は存在しない。
- backend tokenizer/usage は observation-only で、eligibility、ordering、scoring、failure code、decision に使わない。
- requested max output 12,000 は backend への共通 request にすぎず、独立に tokenized/verified されない。
- equal token budgets、equal information、equal serialized request bytes、equal cost は主張しない。

根拠は `README.md:89-103`、`PROTOCOL.md:171-175,257-283`、`EVALUATOR_SPEC.md:386-408`、`POWER_AND_DECISION_BOUNDARIES.md:153-166`、`preregistration.json:50-62,318-323,339`。特に限界を明示しているため、F2 の「token equality を装わない」という文書修正は **PASS** である。

production import は stdlib と evaluator-local module のみであり、backend usage を eligibility や score に参照する分岐もない。現行 `_execution` は usage を artifact field へ写すだけである (`pipeline.py:192`)。この狭い意味で `stdlib_only` と observation-only trust boundary は破っていない。

### Blocker: byte-budget ruling の executable contract が未実装

文書同士は一致するが、文書とコードは一致しない。

- 仕様は reviewer call より前に closed `budget.json` を作り、`verify-run` が再計算することを要求する (`EVALUATOR_SPEC.md:410-421,962-978`)。
- 仕様は execution ごとに closed `token_observation` を直接含める (`EVALUATOR_SPEC.md:423-437,981-982`)。
- 片 arm が 65,537 bytes なら、両 reviewer/judge call なし、CLI 0、packet/budget/ledger/seal のみを持つ sealed pair-wide `model_ineligible` を要求する (`EVALUATOR_SPEC.md:439-455`)。
- production は `_packet` 内で ceiling 超過を `PipelineError("packet_source_ceiling", 2)` にする (`pipeline.py:114-120`)。これは artifact sink 作成前 (`pipeline.py:201-209`) なので、packet、budget、ledger、seal を残さない。
- normal run の required artifact set に `budget.json` がなく (`pipeline.py:228-231`)、`RUN` は常に normal reviewer/judge path と primary を構築する (`pipeline.py:209-216`)。
- `_execution` は generic `usage` を持つだけで、仕様の closed `token_observation` を持たない (`pipeline.py:192`)。

現在の M01 と spec-contract test は `_packet` の 65,536/65,537 acceptance だけを見るため、この full-pipeline terminal/audit contract の欠落を検出できない。裁定そのものは妥当でも、その裁定を測定証拠へ実装するテストがない以上、研究実行を許可できない。

## F3/F4 — 前回ケースの閉鎖は確認できた

### 再封印 artifact 7 件

A01/A02/A04/A05/A06/A07/A08 を、valid run の改変後に ledger と seal を再計算して再実行した。unknown repository/execution/pair-arm/slot fields、noncanonical request、forged opportunity、forged binding hash は **7/7 で `verify-run ok=false`** となった。nested record closure と semantic replay の修正は有効である (`pipeline.py:228-278`)。

### 前回の残り 9 件

`hostile_regressions.py:29-52` を直接再実行し、次を全件確認した。

- Git: leading-zero/+ size、non-ASCII OID、duplicate tree headerを typed reject
- freeze: qualified `builtins.eval` と `importlib.import_module` を typed reject
- authenticated input: object allow-list、integer obligation ID を typed reject
- CLI missing argv: stderr proseではなく canonical `m20.cli-error.v1`

F3 の 7 artifact とこの 9 件で、前回の silent accept 12 件および non-typed reject 4 件はすべて回帰確認済みである。

## 新規不正入力 37 件

前回の 45 inputs と重ならない 37 件を `/tmp` の probe から実 production decoder/helper へ投入した。

| 領域 | 件数 | typed reject | silent accept | untyped reject |
| --- | ---: | ---: | ---: | ---: |
| model output decoder | 14 | 14 | 0 | 0 |
| Git repository parser | 8 | 8 | 0 | 0 |
| freeze source gate | 5 | 0 | **5** | 0 |
| model transport metadata | 3 | 0 | **3** | 0 |
| authenticated obligation | 7 | 0 | **6** | **1** |
| **合計** | **37** | **22** | **14** | **1** |

model decoder では task/inventory echo、schema version、non-string union/summary/source ID、missing mechanism field、null conclusion/loss ID、3 種の judge hash echo、total-sum mismatch、missing dimension を試し、全件 closed `TypedError` だった。Git では `.`/`..`/backslash component、mode `100664`、tree-mode-to-blob type mismatch、tree header position、uppercase parent OID、nondigit size suffixを試し、全件 `PreflightError` だった。この二領域の修正は良好である。

黙受理・非 typed の内訳は次のとおり。

1. freeze は `from builtins import eval as e`、`getattr(builtins,"eval")`、`__builtins__["eval"]`、`subprocess.run(...)`、`os.system(...)` を全件受理した。現在の AST scan は importlib import と bare/attribute の限定名だけを照合する (`freeze.py:35-45`)。仕様は dynamic import/`exec`/`eval` と undeclared executable lookup の拒否を要求する (`EVALUATOR_SPEC.md:1118-1120`)。
2. `_result` は `timeout="false"`、`client_truncation=1`、`provider_truncation=[]` を受理した。`process_exit` と usage/tool item は検査するが、3 flag の bool 型を検査しない (`pipeline.py:184-187`)。truthiness により failure code が変わるため scorer input の型穴である。
3. `_obligation` は integer `reference_id`、integer `projection_id`、empty source `required_id`、unknown subject role、forged `projection.canonical_sha256`、empty `reference_id` を受理した (`pipeline.py:35-62`)。source/projection required ID を両方 integer にすると typed refusal ではなく `AttributeError` になった。

これらは単に human-facing error の問題ではない。freeze bypass は禁止された executable behavior を frozen bundle に含められ、transport flag の型は policy resultを変え、projection hash/ID closure は authenticated source trace の意味を弱める。

## その他のテスト証拠上の限界

generated full-run fixtures の 52 `vector_*` cases は、現在も case 名だけを増やし、全件同じ `("claim", "pass")` transport を使う (`freeze.py:8-15`)。各 vector の hostile input を full pipeline に注入していない。`generate-fixtures --check` が通ることは byte reproducibility を示すが、この semantic hollowness は検出しない。

また generated attack manifest の operation/oracle は ID を埋めた汎用文字列であり、具体的 mutation、expected changed result、観測結果を artifact 自体には残さない (`freeze.py:15`)。実行側 `attack_oracles.py` が実 mutation を行う点は改善材料だが、manifest 単独を網羅性の証拠にはできない。

## NO を解除するための最小条件

1. 12 surviving mutationsを regression/operator として追加し、unit/acceptance gate の少なくとも一方を確実に落とす。
2. spec-contract matrix に cross-field equality/order/echo/type/stable-ID/terminal/artifact/freeze security を追加し、各規範が表へ一度だけ対応する coverage ledger または drift check を持たせる。
3. §3.3.1 どおり `budget.json`、exact/+1 pair-wide terminal、zero-call/no-primary artifact variant、`verify-run` recomputation を full pipeline fixture で実装・検査する。
4. closed observation-only `token_observation` を実装し、backend 値が決定に流れないことを mutation test する。token ceiling は追加しない。
5. freeze scan を alias/getattr/builtins mapping と undeclared subprocess/os executable lookupまで閉じるか、より保守的な allow-list policyへ置き換える。
6. transport metadata と authenticated obligation の全 field を closed typed decodeし、projection canonical hashを再計算する。
7. generated vector full runs を各 vector の実 input/expected resultへ接続し、attack manifest を具体的な操作・oracle・expected changeで監査可能にする。

`preregistration.json` の freeze hash が `null` であることは判定理由に含めていない。上記を解消して再レビューが YES になった後、運用手順どおり non-null freeze を実行することが研究測定開始の前提である。

## 再現コマンド

```text
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s evaluator/tests -t . -p 'test_*.py'
PYTHONDONTWRITEBYTECODE=1 python3 -m evaluator verify-reference-vectors
PYTHONDONTWRITEBYTECODE=1 python3 -m evaluator run-attacks
PYTHONDONTWRITEBYTECODE=1 python3 -m evaluator generate-fixtures --check
```

コード、m20 文書、`crates/` は変更していない。`evaluator/` 配下へ作業ファイルも作成していない。mutation copy と probe script は `/tmp` のみを使用した。
