# D9 評価ハーネス最終品質レビュー

## 判定

**NO**

オーケストレーター向け指標: **NO / b / 0 / 8**

- `b`: 公称 `3067 / 3067 killed`, score `100%` は有効な mutation score ではない。
- 自己追加した意味的等価変更の survive 数: **0 / 6**。
- 残る主要限界: **8件**。
- freeze null を理由にした NO ではない。freeze の運用実行は依然として受入前提だが、本判定はそれより前に再現する scoring/oracle 欠陥による。

## 要約

提供 artifact `/tmp/m20-h6-mutation-sweep-final.json` の SHA-256 は指定値 `8e50123d1294963ae4a3ab5600155e7e77be781ae49573a27faf17feb07479ce` と一致し、記録上は 3067 mutants、killed 3067、survived 0、material survived 0、score 100.00% である。6本の対象 production module の現行 source hash も artifact と一致した。

しかし sweep と同じ worker 環境で**未変更 production**を実行しても unit suite が失敗した。`mutation_sweep.py` が mutant ごとに `M20_SWEEP_WORKER=-<hash>` を設定し、`support.py` がそれを fixture root の絶対パスへ付加する一方、凍結済み generated fixtures は suffix のない絶対パスを含む。そのため `test_freeze.py` の `check_generated` が mutation の有無と無関係に失敗する。自己追加した意味的等価変更6件が1件も survive しなかったのも、この共通偽陽性で説明できる。

したがって、今回の 100% は「テストが全 mutant の意味差を検出した」証拠ではない。artifact から正しい mutation score は復元できず、oracle、resume provenance、timeout 分類を修正した clean sweep が必要である。

## 1. artifact の監査

### 記録値

| 項目 | 値 |
|---|---:|
| total | 3067 |
| killed | 3067 |
| survived | 0 |
| material survived | 0 |
| 公称 score | 100.00% |

artifact の結果三つ組 `(unit, vectors, attacks)` の主要分布は次のとおりだった。

| 結果 | 件数 |
|---|---:|
| `(1, 0, 0)` | 1092 |
| `(1, 0, 2)` | 677 |
| `(124, 0, 0)` | 666 |

少なくとも vectors または attacks が非ゼロの行は 1309 件、両方ゼロは 1758 件である。1309/3067 = 42.68% は独立した二つの oracle が失敗を観測した行の下限にすぎず、補正 score ではない。unit oracle が共通汚染されているため、残り1758件を killed と survived に分類できない。

### 未変更 baseline が kill される

sweep と同じ worker suffix を与え、production を一切変えず unit discovery を実行した結果は次のとおりだった。

- 34 tests 実行、1 failure、1 skip、約15.8秒。
- failure は `test_complete_generated_full_runs_match_production` のみ。
- `reference_vectors.generated.json` と `attacks.generated.json` は一致。
- `fixtures.generated.json` と `inventory.generated.json` は不一致。
- 差は `/tmp/m20-evaluator-pipeline-fixture-v1/...` と `/tmp/m20-evaluator-pipeline-fixture-v1-<worker>/...` の absolute path。

原因の連鎖は以下である。

1. `evaluator/mutation_sweep.py` の `_run_one` が `M20_SWEEP_WORKER=-<hash>` を設定する。
2. `evaluator/tests/support.py` の `FIXTURE_ROOT` がこの値を root path に連結する。
3. generated full-run fixture には repository/stage/obligation の absolute path が canonical bytes として入る。
4. `evaluator/tests/test_freeze.py` が再生成結果と凍結ファイルの byte equality を要求する。
5. よって unit exit は mutant の意味とは無関係に 1 になる。

mutation runner には、まず未変更 baseline の全 oracle が exit 0 であることを確認し、満たさなければ sweep 全体を `error` で停止する preflight が必要である。

## 2. 自己追加した意味的等価変更

repository 内の `evaluator/` は変更せず、`/tmp` の production module コピーに次の6変更を個別に適用し、sweep と同じ三つの oracle と worker 環境で確認した。

| ID | production への等価変更 | unit | vectors | attacks | sweep 判定 |
|---|---|---:|---:|---:|---|
| E01 | canonical 処理の局所変数 `decoded` を rename | 1 | 0 | 0 | killed |
| E02 | 相互依存しない token assignment 2文を交換 | 1 | 0 | 0 | killed |
| E03 | token count の局所変数を rename | 1 | 0 | 0 | killed |
| E04 | 相互依存しない source text/digest assignment を交換 | 1 | 0 | 0 | killed |
| E05 | decoder の局所変数 `raw_hash` を rename | 1 | 0 | 0 | killed |
| E06 | 到達不能な `if False: raise ...` を追加 | 1 | 0 | 0 | killed |

**survive は 0 / 6。** 各 unit failure はすべて上記 `check_generated` の path 差で、vectors/attacks は通過した。意味的等価 mutant を校正用に入れれば、正常な sweep では survive または `equivalent/invalid-for-score` へ triage されるべきである。今回は全件が同じ無関係な oracle で kill されたため、ユーザー指定の判定基準では sweep/oracle 欠陥を疑う条件に該当し、実際に欠陥を再現した。

## 3. resume provenance の欠陥

`mutation_sweep.py` の resume は mutant の source hash と mutated hash が一致すれば既存 killed row を再利用するが、次を結果 provenance に束縛していない。

- mutation runner 自身の hash/version。
- unit/vector/attack tests と generated fixtures の hash。
- timeout、並列度、Python/runtime、host/resource 条件。
- baseline preflight の結果。

artifact 内には unit exit 0、attacks exit 2 の2 mutant があるが、現行コードで同じ mutant を `_run_one` に再投入すると両方とも unit exit 1、attacks exit 2 になった。現行 production hash が一致していても artifact が現行 runner/oracle の単一実行結果とは限らず、resume による混成結果である。受入用 sweep では全 provenance を run identity に含め、違えば resume を拒否すべきである。timeout/fallback/error 行も再利用対象にしてはならない。

## 4. timeout kill の評価

「timeout kill 666件（21.7%）」は artifact 全体の timeout 件数ではない。666 は結果が**ちょうど** `(unit=124, vectors=0, attacks=0)` の件数であり、いずれかの oracle が 124 になった mutant は **737 / 3067 = 24.03%** だった。

artifact の unit-timeout mutant 4件を、並列負荷を外して timeout 90秒で再実行した。

| sample | mutation | artifact | 単独再実行 | triage |
|---|---|---|---|---|
| T1 | `textnorm.py` condition negation | unit 124 | 90秒でも124 | token loop の実質的 hang。kill 候補 |
| T2 | `pipeline.py` condition clause deletion | unit 124 | exit 1、約15.6秒 | timeout は負荷依存。失敗は共通 fixture oracle のみ |
| T3 | `pipeline.py` comparison replacement | unit 124 | exit 1、約15.2秒 | timeout は負荷依存。fixture failure と意味的 test failure が併存 |
| T4 | `repository.py` `if_condition_false` | unit 124 | exit 1、約15.3秒 | timeout は負荷依存。失敗は共通 fixture oracle のみ |

標本4件中3件は、60秒・並列実行時の timeout を単独再実行で再現しなかった。従って timeout 737件を一律 semantic kill と数えるのは不適切である。少なくとも `killed_by_assertion`、`killed_by_expected_exception`、`timeout_reproduced_isolated`、`timeout_flaky/resource` を分離し、timeout は長い単独再試行と原因 triage 後にのみ分母へ含める必要がある。artifact には jobs、各試験の経過時間、host/runtime/resource fingerprint も不足している。

## 5. fallback path

mutant selection が `None`、または hash mismatch の場合、runner は三つの exit をすべて125にして `outcome: killed` を返す。これは scoring defect である。選択・適用・復元・hash 検証の失敗は mutant がテストに検出されたことを意味せず、`error` として sweep 自体を非ゼロ終了させ、score の分子・分母から除外しなければならない。

今回 artifact の fallback row が0件であることは確認済みでも、この分類ロジックが正しいことにはならない。受入条件は「今回0件」に加え、「fallback を注入した自己テストが non-scoring error を返すこと」である。

## 6. operator と module の網羅性

### 実際の対象

3067件の内訳は次の6 module だけである。

| module | mutants |
|---|---:|
| pipeline | 1573 |
| repository | 547 |
| model_boundary | 496 |
| source_payload | 199 |
| textnorm | 138 |
| canonical | 114 |

runner の `MODULES` には9 module が列挙されるが、generator が `category != scoring-relevant` を skip するため、`artifacts.py`、`freeze.py`、`cli.py` は0件である。6 module の「score construction logic」に限定した sweep と記述するなら整合するが、artifact integrity、release gate、terminal state/exit code を含む「評価ハーネス全体の網羅的 sweep」とは主張できない。schema、generated data/template、test generator 自体も対象外である。

### operator scope

実装済み13 operator は comparison、clause deletion、condition negation、integer ±1/zero、if true/false、and/or、raise deletion、membership inversion、bool flip、set relaxation である。条件境界に対する広がりは D8 の隣接 mutation 表より大きくなったが、少なくとも次が欠ける。

- 通常文の削除（`raise` 以外）。
- return の削除・返値置換。
- assignment RHS、call の削除・置換。
- 算術 operator の置換。
- string/bytes/`None` constant の置換。
- dict/list field/member の削除。
- variable/attribute reference の置換。
- exception handler、`break`、`continue` の変異。
- schema、generated artifact、template の構造変異。

よって「網羅的 AST mutation sweep」は、現状の6 module・13 operator という明示された universe に対しても、一般的な AST mutation の網羅性を満たさない。

## 7. D8 F1–F4 の再確認

### F1: 隣接 mutation の一般化

以前の Y01/Y02/Y03/Y05/Y06 を現行コードへ再適用し、`test_h6_regressions` が5/5を検出することを確認した。D8 の invalid 37件もすべて typed rejection になった。追加で実行した39件では typed 38、silent 1、untyped 0で、silent 1件は D8 の37件から明示的に除外されていた曖昧な `tool_calls` list-container ケースである。

従って D8 で求めた個別回帰修正は効いている。ただし D9 sweep は共通 oracle 汚染により、その一般化を 100% score で証明できていない。

### F2: token ceiling / equal token budget の裁定

`README.md`、`PROTOCOL.md`、`EVALUATOR_SPEC.md`、`POWER_AND_DECISION_BOUNDARIES.md`、`preregistration.json` の記述は次で一致している。

- evaluator-enforced input-token ceiling/tokenizer preflight は行わない。
- 65,536 は admitted-source UTF-8 byte ceiling である。
- equal token budget、equal information、equal serialized request bytes、equal cost は主張しない。
- backend token usage は `observation_only` で、eligibility、ordering、scoring、failure code、decision に使わない。

実装も `_token_observation` を closed observation record として保存し、`verify_run` は `token_observation_recomputable=false` を返す。token count を ceiling と比較する経路は見つからなかった。F2 の裁定と限界記述は概ね整合している。

ただし generated `budget_plus_one_65537` fixture は仕様の「片腕だけ 65,537 bytes」と異なり、現状は両腕が 65,537 bytes になる。production の pair-wide 判定は `all(...)` で片腕超過も拒否する形だが、full-pipeline fixture が asymmetric one-arm overflow を直接証明していない。

### F3: model-ineligible と budget artifact

exact ceiling と +1、zero-call、sealed `model_ineligible`、null arm result、budget artifact 再監査を `test_budget` と `test_h6_regressions` で確認した。両 suite 合計11 tests は成功した。上記 asymmetric fixture gap は残る。

### F4: token observation と invalid input

`token_observation` の closed fields、authority/source、非負整数、unavailable reason の検証と、execution/judge execution の audit 組込みを確認した。D8 の cross-field/role/hash/authenticated-obligation 系の主要 mutation も回帰試験で検出された。

## 8. 通常 baseline の状態

worker suffix を付けない通常環境では次が成功した。

- unit discovery: 34 tests、OK、1 skip、約25.3秒。
- reference vectors: 52/52 pass。
- attack suite: 61/61 killed、survived 0。
- `generate-fixtures --check`: OK。
- `test_h6_regressions` + `test_budget`: 11 tests、OK。

これは通常の回帰 suite が健全に動くことを示す。一方、mutation score の妥当性には「mutation runner と同一環境の未変更 baseline」が必要であり、そこが失敗している。

また `freeze.py` が52 vector IDすべてに同一の reviewer/judge transport `("claim", "pass")` を与えて full-run fixture を生成するため、52ケースの hostile input semantics を production full-run に注入した証拠にはなっていない。byte reproducibility と semantic end-to-end coverage は区別すべきである。

## 9. 残る限界 8件

1. **Baseline-kill contamination:** worker-specific fixture path により未変更 production 自体が unit oracle に kill される。
2. **Resume provenance 不足:** runner/oracle/environment を束縛せず、現行再実行と異なる stale row を再利用できる。
3. **Timeout 分類不良:** 実数は737件で、標本4件中3件が単独再現せず、負荷 timeout を semantic kill に混入している。
4. **Fallback 誤分類:** mutation 未適用・hash mismatch の125を `killed` として score に算入する。
5. **Operator 不足:** 一般的な AST mutation operator 群を網羅していない。
6. **Module/data scope 不足:** 実 sweep は6 scoring module のみで、artifact/freeze/CLI/schema/generated data を含まない。
7. **Fixture の意味的空洞:** 52 vector の full-run transport が同一で、+1 fixture も片腕超過を直接試験していない。
8. **主張可能範囲:** 修正後の mutation score でも、選択済み operator に対する test sensitivity しか示さず、仕様の正しさ、研究妥当性、judge proxy の妥当性、open-development bias、bytes と tokens/information/cost の同等性は証明しない。

## 10. YES へ必要な条件

1. worker-specific temporary root を canonical fixture 内容から除外するか、path を正規化し、sweep と同じ環境の未変更 baseline で unit/vectors/attacks がすべて exit 0 になること。
2. baseline failure、mutant selection/apply/restore/hash failure を non-scoring `error` とし、sweep を失敗終了させること。
3. run identity に runner、全 oracle、fixtures、timeout、jobs、Python/runtime/host fingerprint を含め、clean run または provenance-safe resume を行うこと。
4. timeout を単独・延長再試行して `reproduced semantic hang` と `resource/flaky` に triage し、後者を kill へ数えないこと。
5. equivalent calibration mutants が少なくとも想定どおり survive/equivalent になり、material mutant の kill と区別されること。
6. module/operator universe と除外理由を機械可読に固定し、「ハーネス全体」ではなく実際の範囲へ claim を限定すること。網羅的 AST sweep を主張するなら不足 operator と非scoring integrity module/data を追加すること。
7. 52 vector semantics を production full-run に実注入する fixture と、片腕だけ 65,537-byte の pair-wide ineligibility fixture を追加すること。
8. その条件で全 survivor を equivalent / duplicate / unreachable / test gap / specification gap / real defect に triage し、根拠付き ledger を凍結すること。

以上より、F1–F4 の個別修正自体は概ね確認できるものの、新受入基準「網羅的 AST mutation sweep + 生存 mutant の triage」は満たさない。公称100%を受理せず、**NO** と判定する。
