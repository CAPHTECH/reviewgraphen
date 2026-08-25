# 非登録 pilot 原因分析

## 結論

観測された `structured 0/9` は、ReviewGraphen の structured context が free-form diff より劣るという有効な方向性観測ではない。主因は pilot harness の契約非対称である。

- reviewer の outer prompt は、packet 内の deterministic-abstain instruction/schema を「この pilot の response contract ではない」と明示し、別の `m20.nonregistered-pilot.structured-output.v1` を要求した（[structured prompt, lines 1–8](/home/rizumita/workspace/reviewgraphen/tmp/orchestration/PILOT/http-runs-final/low-32k/reviewgraphen/structured/prompt.txt:1)）。
- judge は outer prompt を受け取らず、`candidates.json` の `review_material` と `review_output` だけを読むよう指示された（[judge instruction](/home/rizumita/workspace/reviewgraphen/tmp/orchestration/PILOT/judges/reviewgraphen/instructions.md:1)）。`review_material` は packet 本体なので、judge からは内側の “Return the fixed provider-free abstention using the supplied closed schema.” が唯一の task contract に見えた。
- harness は実際に quickstart の `*.provider-free-reviewer-packet.v1.json` を読み（[run_pilot.py, lines 159–163](/home/rizumita/workspace/reviewgraphen/tmp/orchestration/PILOT/run_pilot.py:159)）、judge candidate には outer prompt ではなく raw structured input だけを格納した（[run_pilot.py, lines 505–535](/home/rizumita/workspace/reviewgraphen/tmp/orchestration/PILOT/run_pilot.py:505)）。

したがって semantic finding を返すほど reviewer の outer 契約には適合するが、judge が推定した fixed-abstention 契約には違反する。逆に fixed abstention は judge instruction の “abstention or malformed output is false” によって primary proxy を必ず失敗する。この組合せでは structured arm の usable 成功経路が閉じている。

これは frozen m20 packet そのものの契約ではない。m20 は `arm-neutral.source-grounded-packet@2` を evaluator 内で構成し（[pipeline.py, lines 126–132](/home/rizumita/workspace/reviewgraphen/benchmarks/m20-changed-public-callee-utility-v1/evaluator/pipeline.py:126)）、instruction は “Inspect the admitted Rust source and return one source-grounded disposition...” である（[common_instruction.txt](/home/rizumita/workspace/reviewgraphen/benchmarks/m20-changed-public-callee-utility-v1/evaluator/data/common_instruction.txt:1)）。judge にも full packet、validated output、binding view を同じ candidate 内で渡す（[EVALUATOR_SPEC, lines 865–883](/home/rizumita/workspace/reviewgraphen/benchmarks/m20-changed-public-callee-utility-v1/EVALUATOR_SPEC.md:865)）。この pilot はその経路の faithful dry-run ではない。

## 観測の保全

結果を良く見せる再分類はしない。既存表の分母は structured 9、free-form 9 のまま、観測値も 0/9 と 7/9 のまま残す。ただし 0/9 には `invalid_directional_inference_due_to_contract_asymmetry` を付し、primary metric、Stage 1/2A、または packet 優位性の証拠へ昇格しない。もともと3ペアは Stage 1/2A から除外済みである（[PILOT.md, lines 1–5](/home/rizumita/workspace/reviewgraphen/tmp/orchestration/PILOT/PILOT.md:1)）。

32k の transport/parser は原因ではない。low-32k と xhigh-32k の12実行は全て `json_extraction_succeeded=true` で、たとえば reviewgraphen structured は stop 終了、17,266 output tokens、抽出成功である（[execution.json](/home/rizumita/workspace/reviewgraphen/tmp/orchestration/PILOT/http-runs-final/low-32k/reviewgraphen/structured/execution.json:1)）。6 structured 実行は5 completed + 1 explicit abstain で、semantic findings を返した5件の全15 unique source-ID references は各 packet の admitted set に含まれた。32k では ID 取り違えを観測していない。

なお pilot outer schema は `source_ids` だけを要求し、window ID field を持たない（[structured prompt, line 7](/home/rizumita/workspace/reviewgraphen/tmp/orchestration/PILOT/http-runs-final/low-32k/reviewgraphen/structured/prompt.txt:7)）。したがって「window ID に束縛できなかった」をこの実験から診断することはできない。

## structured 9件の機序分類

分類記号は依頼の (a) schema/束縛/abstention、(b) packet の判断材料不足、(c) packet 構造・ID の読み違え、(d) judge 基準または judge input の free-form 側への非対称、(e) その他、である。複数該当を許し、主因を先に書く。

| pair / condition | 分類 | 実測した機序 |
| --- | --- | --- |
| reviewgraphen / low-12k | **(a)+(e)** | outer schema の enum 候補と `exact admitted source_id` を未充填のまま返した。judge は “unfilled template” と判定。12,000 tokens 到達と inline reasoning を伴うため、低 pin での完了失敗が直接原因。(c) の ID 取り違えではなく placeholder の未具体化。 |
| fsl / low-12k | **(a)+(e)** | parsed output が空で、judge は “returned no disposition or claims”。12,000 tokens で JSON 抽出も失敗。 |
| casegraphen / low-12k | **(a)+(e)** | reviewgraphen と同じ schema template をそのまま返し、judge は “Literal values such as completed\|abstain...” と判定。 |
| reviewgraphen / low-32k | **(d)→(a)、副次 (b)** | outer schema、exact admitted IDs、3 findings は満たしたが、judge は “violates the packet’s fixed-abstention instruction ... by returning a defect review in a different schema”。同時に `discover`、set semantics、excluded type が欠け、3 TP と3 FP。 |
| fsl / low-32k | **(d)→(a)** | fail-open sibling-field finding は judge も TP としたが、“semantic review instead of the fixed provider-free abstention” の一点で unusable。error-status finding のみ (b)/過剰推論。 |
| casegraphen / low-32k | **(b)、背景に (d)** | exact excerpts は引用したが、assertion trust boundary と overflow reachability が packet に無く、judge は “critical premises are acknowledged as missing”。outer/inner 契約非対称も同じ candidate construction に存在するが、この個別 rationale は evidence 不足を決定理由にした。 |
| reviewgraphen / xhigh-32k | **(d)→(a)、副次 (b)** | judge は明示的に “did not provide the required provider-free abstention”。unknown bound の局所観測は TP、`matches`/`discover` の前提は欠落。 |
| fsl / xhigh-32k | **(d)→(a)** | 2つの局所 classifier 観測を TP としながら、“otherwise structured review” が fixed abstention でないため false。引用した2 source IDs は admitted。 |
| casegraphen / xhigh-32k | **(a)+(b)+(d)** | packet の欠落を理由に outer schema で coherent abstention。judge instruction が abstention を自動 false とし、“limitations and blocked questions are coherent” でも unusable。 |

(c) は32kの6件では0件である。low-12k の2 template は ID を誤対応したのではなく ID を一度も具体化していないため (e) とした。

## judge が usable に要求したもの

judge の明示基準は “usable, specific, source-grounded, internally coherent, and gives an auditable disposition” であり、abstention/malformed は false（[judge instruction](/home/rizumita/workspace/reviewgraphen/tmp/orchestration/PILOT/judges/fsl/instructions.md:1)）。実際の適用は arm 間で次のように異なった。

### structured

1. **judge が packet から推定した response schema への適合。** reviewgraphen xhigh は valid JSON かつ source-supported observation を含むのに、“violates the packet’s explicit closed response schema and fixed-abstention instruction” とされた（[reviewgraphen judgment, candidate `879ea4…`](/home/rizumita/workspace/reviewgraphen/tmp/orchestration/PILOT/judges/reviewgraphen/codex-result/judgment.json:1)）。fsl low-32k も中心所見を “locally supported” と認めながら、“malformed relative to the explicit task” とされた（[fsl judgment, candidate `5cc0e8…`](/home/rizumita/workspace/reviewgraphen/tmp/orchestration/PILOT/judges/fsl/codex-result/judgment.json:1)）。
2. **欠落した前提を defect として補わないこと。** casegraphen low-32k は “critical premises are acknowledged as missing” で失敗した（[casegraphen judgment, candidate `1bd5c2…`](/home/rizumita/workspace/reviewgraphen/tmp/orchestration/PILOT/judges/casegraphen/codex-result/judgment.json:1)）。
3. **しかし abstention 自体も usable にはならない。** casegraphen xhigh は “limitations and blocked questions are coherent” でも false。内側 contract と metric の組合せが成功不能である直接証拠である。

### free-form

free-form は、全主張が正しいことではなく、少なくとも一つの具体的・追跡可能な中心所見と auditable disposition を要求された。

- reviewgraphen low-32k: “clear disposition, exact code references, and a concrete test-quality observation”。production-risk FP を含んでも、`.all(...)` の vacuous-test 指摘で usable（[reviewgraphen judgment, candidate `87d16c…`](/home/rizumita/workspace/reviewgraphen/tmp/orchestration/PILOT/judges/reviewgraphen/codex-result/judgment.json:1)）。
- fsl low: “one materially supported defect family” があり、他2件が speculative でも “supported finding remains usable and auditable”（[fsl judgment, candidate `49de2c…`](/home/rizumita/workspace/reviewgraphen/tmp/orchestration/PILOT/judges/fsl/codex-result/judgment.json:1)）。
- casegraphen xhigh: “specific, auditable findings; two mechanisms are directly demonstrated by the diff” とし、overflow の過剰主張があっても usable（[casegraphen judgment, candidate `218afb…`](/home/rizumita/workspace/reviewgraphen/tmp/orchestration/PILOT/judges/casegraphen/codex-result/judgment.json:1)）。

よって (d) は単なる「散文が読みやすい」という嗜好ではない。free-form は mixed TP/FP でも中心 TP があれば通し、structured は同じく中心 TP があっても judge が誤認した schema contract で落とした、task-definition の非対称である。

## free-form usable 7件の packet 内再現可能性

判定単位は、7個の usable candidate disposition のそれぞれについて、judge が usable の根拠にした少なくとも一つの中心機序を、base/target の変化や packet 外 test/helper を補わず admitted payload だけから書けるか、と固定した。現在の free-form 出力から packet 情報を推測していない。packet の source inventory/ranges と free-form judge TP を機械的に照合した。

**結果は 3/7。該当するのは fsl の low、low-32k、xhigh の3件だけである。** fsl packet は `outcome_class` の sibling-field predicates を含む `rust/fslc/src/outcome.rs:59–223` と `exit_status` の `257–292` を admitted にしている。judge が3条件すべてで採用した fail-open finding は、この範囲だけで記述できる（[PILOT.md, lines 102–116](/home/rizumita/workspace/reviewgraphen/tmp/orchestration/PILOT/PILOT.md:102)）。structured reviewer 自身も low-32k/xhigh で同じ finding を生成し、judge は TP としたため、情報充足の直接的な反実仮想になっている。

残る4件は packet だけでは再現できない。

- reviewgraphen 2件: packet は target の `context.rs:1852–2060` 等を含むが、judge が usable の根拠にした old→new の missing-owner change と `context.rs:5090+` の更新 test/`.all(...)` は含まない。current `continue` は読めても「変更」と vacuous-test finding は書けない（free-form evidence は [PILOT.md, lines 88–96](/home/rizumita/workspace/reviewgraphen/tmp/orchestration/PILOT/PILOT.md:88)）。
- casegraphen 2件: packet ranges は主に `resource_protocol.rs:381–509` の grant evaluator と wrappers で、free-form の中心である `ResourceOccupancyIndex::remove`、その先行 mutation、identity-set/counter construction を含む `95–223` がない。したがって non-transactional removal と set-versus-counter mechanism は packet 外（free-form evidence は [PILOT.md, lines 128–136](/home/rizumita/workspace/reviewgraphen/tmp/orchestration/PILOT/PILOT.md:128)）。

これは product @1 packet の source selection がこの3ペアで均一に十分でなかった証拠ではあるが、m20 @2 packet の効果推定ではない。

## 問題の切り分けと反証方法

### 評価/harness — 確定した主因

確定事実は outer/inner response contract が衝突し、judge input から outer contract が脱落したことである。反証試験は同じ raw output を、(1) reviewer に実際に提示した outer contract と source material を一体化した blind candidate、または (2) frozen evaluator が作る exact @2 packet/binding/mechanical-state/judge batch で再採点すること。修正後も supported fsl structured candidates が source-grounding 次元で落ち、schema mismatch を理由にしないなら、契約非対称だけでは0/9を説明できない。現状は judge 自身が fsl structured の TP を認めつつ schema mismatch だけで false にしており、主因を支持する。

### packet 内容/出力契約 — product @1 では実在、m20 には未検証

3/7 と repo 差は source selection の不足を示す。反証は、同じ source-byte ceiling のまま frozen evaluator が選ぶ @2 の admitted ranges に、free-form 中心機序の必要 range が入るかを、モデル/judge を呼ばず source-ID/range closure で測ること。入っているなら product @1 固有の問題、入らなければ subject-window selection の設計問題である。出力 contract については exact source range observation と typed disposition を持つ frozen `reviewer_output.v1` を使い、pilot 独自 schema を使わない。

### モデル能力 — 一般的な structure 不読は反証済み、低予算完了性は未解決

32k では全12 JSON 抽出成功、structured semantic outputs の foreign ID は0、fsl では packet-only で free-form と同じ中心 finding を得た。したがって「Qwen が packet 構造や ID を読めない」が0/9の説明ではない。一方 low-12k は6件中5件が token ceiling に達し、3件 malformed（[PILOT.md, line 138](/home/rizumita/workspace/reviewgraphen/tmp/orchestration/PILOT/PILOT.md:138)）なので、登録 pin 下の completion risk は否定しない。反証は exact frozen @2 request/schema を12kで事前登録どおり走らせ、mechanical schema/closure pass を測ること。32k へ事後変更して救済してはならない。

### 3ペア偏り

fsl は packet 内で中心機序が閉じ、reviewgraphen/casegraphen は diff の決定的 range が packet 外だった。3件では repository × change-shape の偏りと arm 効果を分離できない。反証は fixed hash-ranked Stage 1 sample と repository/leave-one-out cells をそのまま使うことであり、pilot を見て sample、分母、閾値を変更しない。

## Stage 1 前の処置

1. `structured 0/9` を directional pilot result として使用しない。raw observation、9/9 denominator、judge quotes は保存する。
2. pilot harness を再利用する場合は product quickstart @1 packet を semantic reviewer input に転用しない。frozen evaluator の @2 packet、`reviewer_output.v1` decode、mechanical score、full candidate/binding view、utility rubric を end-to-end で呼ぶ。bespoke judge に outer contract を後付けするだけより、実 Stage 1 経路を直接 dry-run する。
3. source sufficiency はモデル結果ではなく、必要 range が admitted closure に入ったかを別の診断表で記録する。loss/abstention を消さず、packet 外の旧側・test・helper を暗黙に補わない。
4. primary metric `usable_grounded_disposition_completed`、分母、3ペアの除外、12k pin、Stage 1 sample/threshold は変更しない。

**再 seal は不要。** 推奨修正は tmp の非登録 pilot harness を捨て、既に seal 済みの evaluator @2 経路をそのまま使うことであり、frozen bundle bytes/contract を変えない。将来、@2 の source selection、reviewer schema、judge rubric、output pin のいずれかを変更するなら evaluator contract 変更なので atomic re-seal が必要だが、この invalid pilot を根拠にその変更は採らない。
