# D2 独立契約再レビュー

**BLOCKING: 4 / SHOULD-FIX: 2 / NOTE: 4**

**結論:** 前回7件のうち5件は閉じ、2件は部分閉鎖である。ただし、対称化によって主要指標を内容なしで達成できる新しい経路が生じた。現契約はまだ凍結できない。

対象は ADR 0038 と m20 の4ファイルである。E1/E2 の `reviewer_disagreements.md` は指定パスおよび `tmp/orchestration/` 配下に存在しなかった。対象、既存実装、前回レビューは読み取りだけに使い、対象ファイルと実装は変更していない。なお v2 識別子は現行 `crates/` / `schemas/` にまだ存在しないため、以下の「CLOSED」は実装完了ではなく、凍結前の契約として実装・検証可能な一意性が閉じたという意味である。

## 前回7件の閉鎖判定

| 前回 | 判定 | 再レビュー結論 |
| --- | --- | --- |
| D1-B1: abstention の arm 非対称 | **PARTIALLY-CLOSED** | A/B とも同じ output/scorer 形と loss-based abstention を使えるようになり、前回の「A は原理的に abstain 成功不能」は閉じた。しかし baseline に D 固有 property ID が入り、さらに対称化で全packetに常用可能な abstention basis が生じた。D2-B1/B2/B3。 |
| D1-B2: fan-out/deferred gate 脱落 | **PARTIALLY-CLOSED** | m20 の4ファイルでは seven gates、exact ID sets、p95 rank 285、`20|D|<=|A|`、exact/+1 fixture、失敗時停止が一致した。一方 ADR の Evaluation commitment は seven gates を列挙せず deferred gate を落とし、時間予算も m20 と矛盾する。D2-B4。 |
| D1-B3: Cargo arbitrary-code threat | **CLOSED** | `workspace.cargo_test@1` を実行せず、request は固定IDしか選べず、選択時は一意な typed `unsupported`、run は verifier process/evidence record を許さず、validator は `verifier_observed` の空集合を要求する。command/argv/path/cwd/env/mount/cache/limit は schema-invalid であり、no-spawn test matrix も必須である（ADR:675-746,788-820,1021-1028）。 |
| D1-B4: 独立性のない exact p 値 | **CLOSED** | 主要判定は frozen-corpus descriptive rectangle へ変更され、p-value/alpha/CI/significance は禁止された。repository別と3つの leave-one-repository-out cells は必須である。参考 tail は明示的に非推論・非判定であり、成功条件へ再流入していない（README:44-49; preregistration:8-34,978-1014; PROTOCOL:290-294; POWER:21-49,51-87,155-175）。 |
| D1-B5: profile/exclusion 未固定 | **CLOSED** | `rust.production.v1` の closed DTO、path/matcher/category/precedence/reason semantics、canonical bytes/hash、production diff、exclusion ID preimage が ADR で固定され、preregistration は ID/schema/hash と ADR authority だけを参照する。実測 hash も本文値 `4b6cca…dd96` と一致した（ADR:347-455; preregistration:53-57,1057-1059）。 |
| D1-B6: unresolved-call occurrence closure 欠落 | **CLOSED** | v2 obstruction は call kind/reason/span/source/snapshot/extractor と `related_capabilities=["direct_calls"]` を閉じ、run validator が global set と occurrence subset を extraction report から再構築する。macro expansion の潜在個数は invocation count と分離され `unknown` のまま残る（ADR:263-339,1005-1010）。 |
| D1-B7: context v2 discovery 未閉鎖 | **CLOSED** | `at least as strict` はなく、全bounds、seed、edge/direction/depth、BFS state/tie-break、anchor/path/test/window/loss/hash規則が exact DTO として固定された。実測 hash も本文値 `7c4ce…bf26` と一致した（ADR:494-673,1016-1020）。 |

## BLOCKING — 修正後に残った／新たに生じた問題

### D2-B1 — 対称化で primary endpoint が blanket abstention と形式的 claim によって空洞化した（新規）

- **根拠:** 全packetは最低1件の `not_admitted_context` loss を必須とする（PROTOCOL:180-187; preregistration:245-251）。abstention は非空 `detail` と inventory 内 loss ID を1件引用すれば schema-valid である（preregistration:282-287,385-433）。claim 側も `inconclusive` を許し、非空 `summary` と admitted source ID だけで schema-valid である（同:276-280,325-383）。`specific_disposition_valid` は boolean として出力されるが、「concrete」対「generic」を機械的に判定する frozen algorithm はなく、normative schema が保証するのは文字列 `minLength: 1` だけである（同:439-553）。judge-positive yield は secondary で、成功に最低件数を要求せず primary を救済も棄却もしない（同:930-950,1005-1014）。
- **帰結:** 両armは source semantics を評価せず、常に存在する loss を引用して同じ abstention を返すだけで `completed=1` にできる。あるいは任意の admitted source に `inconclusive` を結ぶだけでもよい。これは前回の非対称を直した結果、新しく両armに生じた抜け道である。仮に `b>=18,c<=7` を満たしても、示せるのは主に ID echo／JSON遵守の差であり、「事前登録した主要指標で明確な実益を示した」とは判定できない。`frozen-corpus descriptive` への統計的限定は正しいが、この endpoint の内容妥当性を補わない。
- **必要な閉鎖:** primary success に、事前登録済みで機械判定可能な有用性条件を追加すること。少なくとも無条件に利用可能な loss だけを引用する abstention と内容のない `inconclusive` を practical completion から分離し、arm-hidden judge または独立した source-grounded rubric の事前固定された閾値を成功の必要条件にする必要がある。abstention を有用と数えるなら、どの loss がそのpropertyの判定不能を支持するかを機械的に閉じ、全packet共通 loss の存在だけでは成功させないこと。

### D2-B2 — baseline に D 固有 property lens が漏れ、free-form contrast が成立していない（残存）

- **根拠:** A は「free-form」「D relation、endpoint annotation、obligation universe、subject-first metadata を受けない」と定義される（README:51-56; preregistration:146-158）。しかし共通 output schema は `property_id` を必須とし、定数は自己記述的な `rust.callee_contract_review@1` である（preregistration:253-287,302-324）。共通 `target_id` の preimage にも同じ property ID が入り（同:187-208）、両armは同じ instruction/schema を受ける（PROTOCOL:221-225）。したがって A のモデル入力は「callee contract review」という D のレビュー lens を受ける。
- **帰結:** rule ID、relation ID、endpoint IDs は A から除けたが、何を探すべきかという主要な ReviewGraphen semantic structure は残る。A は「production diff の free-form review」ではなく「D property を知らされた diff-only review」であり、preregistered question/README の contrast と一致しない。これは B の構造効果を過小評価するだけとは限らず、property cue と source projection の効果を分離不能にする測定汚染である。
- **必要な閉鎖:** (a) A を property-directed diff review と明記して estimand/arm名/解釈を全ファイルで変更するか、(b) reviewer-visible schema を arm-neutral task/disposition ID にし、D property ID は scorer側の hidden binding としてのみ閉じること。どちらでも A/B の reviewer-visible canonical packet fixture を凍結し、A に rule/property/relation/endpoint/subject metadata がないことをnegative fixtureで検証すること。

### D2-B3 — arm-neutral inventory は「closed」と宣言されるだけで normative schema と有効fixtureがない（新規）

- **根拠:** output と primary scorer には埋込み Draft 2020-12 schema があるが、source inventory には `closed: true`、required-field名、自然言語の `field_types` しかなく `normative_json_schema` がない（preregistration:217-252 対 288-553）。`role` のenum、hash形式、各nested objectの `additionalProperties:false`、rangeの整合、source/loss IDの全体一意性は実行可能な schema/semantic algorithm として固定されていない。6 fixtures の `inventory` は `source_inventory_id/source_ids/loss_ids` だけの scorer用略記であり、列挙された8 required fieldsを持つ inventory instance ではない（同:555-839）。PROTOCOL はそれらを「frozen fixtures」と呼びつつ、canonical bytes と expected scorer records を後の freeze-manifest input に回す（PROTOCOL:180-219）。
- **帰結:** evaluator実装は同じ schema ID の下で baseline `role` や loss object に D 固有ヒントを追加でき、逆に必要な closure を省いても、今ある6例はそれを検出しない。B1 の対称化が本当に arm-neutral かを機械的に証明できず、fixture bytes/hashも本文から一意に再構成できない。
- **必要な閉鎖:** inventory の完全な closed JSON Schema と別のsemantic validatorを今の preregistration に埋め込み、全enum・canonicalization・ID/hash preimage・cross-field closureを固定すること。A/B の各成功/失敗fixtureには schema-validな完全 inventory、packet-visible schema/instruction、canonical bytes/hashを含め、baselineへの D semantic field混入と foreign/extra role/loss/sourceを落とす mutation fixturesを追加すること。

### D2-B4 — ADR と凍結対象m20で Stage 0 と時間authorizationが一致しない（残存＋新規不整合）

- **根拠:** m20の4ファイルは seven gates と 21 model-hours / 24 wall-hoursで一致する（README:72-85,109-112; preregistration:864-910,965-1014; PROTOCOL:309-319; POWER:177-192）。一方 ADR の Evaluation commitment は Stage 0 を「prevalence, subject retention/loss, context reduction, deterministic rebuild, denominator visibility, fan-out」とだけ列挙し、`deferred fraction` と benchmark 名の `enumeration honesty` を明示しない（ADR:888-900）。同節はさらに `24 cumulative model-hours`、Stage 1 `within six model-hours`、Stage 2A `within 24 cumulative model-hours` と規定するが、m20 の model ceiling は5.25 h/21 hで、6 h/24 hは reserve込み wall-clock envelope である。ADR section 4.3 が fan-out/deferredを両方必須にしていても（ADR:457-492）、後の Evaluation commitment と単一の exact Stage 0 list/time authorityになっていない。
- **帰結:** PROTOCOL は ADR 0038 自体をfreeze inputとし、曖昧時だけ preregistration machine contractを優先する（PROTOCOL:3-7,30-48）。これは「曖昧」ではなく、モデル実行のauthorization単位と必須gate列挙の矛盾である。準拠実装／運用者が ADR の24 model-hoursを採るか、m20の21 model-hoursを採るか一意でない。
- **必要な閉鎖:** ADR section 10 を seven gate IDs・exact thresholdの参照・全条件 conjunction に揃え、deferred gateを明記すること。時間は `cumulative_model_ceiling=18,900/75,600 s (5.25/21 h)` と `wall-clock envelope=21,600/86,400 s (6/24 h)` に分離し、全5ファイルで同じ用語にすること。

## SHOULD-FIX

### D2-S1 — control label の seal 順序に実現不能な「order未公開」が残る

PROTOCOL:143-155 と POWER:139-148 は label を order reveal 前に行うと書く一方、repository/commit IDs と sampling seed は公開され、本文自身が「public rankの知識」を認める。labels は membershipを動かさず、全eligible commitを二者labelするため前回S3の実害は閉じているが、秘密でないorderを秘密のように書かず、「arm outcomes前、全eligibleをlabelし、rank認識の有無にかかわらずmembershipへ影響しない」に統一すべきである。

### D2-S2 — subject-retention gate の失敗集合と丸めが exact-ID contractになっていない

`A_c` は profile exclusion後の applicable obligations なのに、subject retention の remainder を「typed excluded or unknown」とする（PROTOCOL:84-106; preregistration:869-887）。excluded candidate は `A` に入らないため remainderにはなれない。また95%の numerator ID set、両subjectを含む判定、整数比較（例 `20|S|>=19|A|`）が固定されていない。fan-out/deferredと同じく exact setとexact/+1 fixtureを追加し、subject loss/unknownとprofile exclusionを混同しないこと。

## NOTE

### D2-N1 — deferred verifier の帰結は正しく明示された

descriptor選択は request拒否ではなく fixed typed `unsupported`; verification disabledならrecord不在である。`verifier_available=false`、`verifier_observed=[]`、primary creditなし、Evidence/Verification/Decision/Finding生成なしが一貫する。実workspace検証は明示的 non-goal/deferredで「不要」とはされていない（ADR:675-746,960-987,1058-1070）。m20 secondary endpointsから verification yield は削除され、preregistration:84-85 も不実行・primary非寄与を明記する。したがって無意味な verification-yield 指標は残っていない。

### D2-N2 — `independent` grep の残存語は統計的主張ではない

5対象の全残存を確認した。統計文脈ではすべて「not independent」「no independent sample」または仮想的 reference assumption である。それ以外は独立した mutation、空cache、provider-free observer、labelerの作業など通常語であり、analysis unitの独立性を主張しない。reference tail は式・数値とも残るが、alpha比較・advance/success条件には入らない。削除必須とは判定しない。

### D2-N3 — D1の4 SHOULD-FIXのうち S1/S3/S4 は閉じ、S2だけADRに残る

- S1: tokenizer/version/hash、packet/component bytes、preflight/provider usage、truncationが両arm必須になった（PROTOCOL:239-254; preregistration:951-963）。
- S2: m20の4ファイルは21 model-hours + 3 reserve = 24 wall-hoursへ修正済み。ただし ADR の旧表現が残り D2-B4。
- S3: control labelはsample membership/orderを変えず、二者・全eligible・disagreement/unable処理・adequacy stopを固定した（PROTOCOL:133-160; preregistration:912-929）。
- S4: verifier実行自体をdeferしたため、execution/dedup/reuse ambiguityはこのsliceから消えた（ADR:737-746）。

### D2-N4 — canonical materialの機械確認

`preregistration.json` はJSONとしてparse可能であり、埋込み reviewer-output / primary-score schema に対して6つの output/expected-score は妥当だった。ADRから改行を除いて再計算した profile/context DTO SHA-256 は、それぞれ `4b6cca93794ab03b1576e17d2e395ec43f731d316685247a89363ae2e840dd96` と `7c4ceca165588cd38b28cc6882bb68a1ff4040dbb19ee34dfd792216d70bbf26` で本文と一致した。これは D2-B3 の「完全inventory fixtureがない」問題を相殺しない。

## frozen-corpus descriptive gate は「明確な実益」を示せるか

**現契約のままでは示せない。** 非独立commitにp値を付けない修正、repository cells、leave-one-out、非常に大きいrectangleは妥当である。しかし統計的慎重さとは別に、primary endpoint が常設lossを引用した blanket abstention または内容未検査の `inconclusive` を成功とする。したがってrectangle通過から「有用なレビュー判断が増えた」への意味的な橋がない。D2-B1を閉じれば、限定された frozen corpus 上の明確な実益を示しうる descriptive designにはできる。

## ユーザー定義の実用状態9項目

ここでの判定は「この契約を矛盾なく実装したと仮定して、項目を契約として満たすか」であり、未実装の現行コードを実装済みとは数えていない。

| # | 実用性ゲート | 契約実装時 | 根拠・限界 |
| ---: | --- | --- | --- |
| 1 | local Rust repo + base/head → deterministic ingest → versioned universe | **達成** | request v2 がimmutable repository/base/target、profile/rule/extractor/context identitiesをbindし、deterministic rebuildを必須化する。 |
| 2 | fixture固有でない Relation / Path / Invariant obligation | **達成** | real Rust accepted `calls` relationから D substantive obligationを生成し、fixture名・repository identity分岐を禁止する。 |
| 3 | obligationごとの bounded projection と source/loss/hash | **達成** | subject-first multi-window、candidate denominator、included/excluded/unknown/loss、projection hashがexact contract化される。 |
| 4 | structured claim または abstention; raw prose非canonical | **達成** | closed structured outcomeとraw/parsed hashを要求する。ただし empirical utility scoring は D2-B1 で未成立。 |
| 5 | claim/evidence/verification/human decision分離; unsupported/stale/inconclusive保持 | **達成** | non-authority ceiling、typed unsupported/incomplete reasons、自動Evidence/Decision生成禁止が固定される。 |
| 6 | allow-list + workspace-scoped verification; reviewer arbitrary shell禁止 | **未達** | reviewer shell/toolは閉じるが、workspace verificationは常に `unsupported`。successor sandbox ADRが必要である。 |
| 7 | 短いPR report + auditable JSONを一つのCLI workflowで生成 | **達成** | run-v2 canonical audit JSONからclosed human-report manifest/Markdownを生成し、CLI/runtime major dispatchを必須化する。実装時は単一workflow testが必要。 |
| 8 | provider不要quickstart + real-model adapter | **達成** | generic `deterministic.abstain@1` と Codex/Claude process adapterを同じv2 observer unionへ置く。 |
| 9 | clone後に文書どおり再現可能 | **未達** | ADRは clone後quickstart、checked-in request、artifact生成commandを必須化していない。m20 corpus rootsとbackendはこのmachine固有であり、benchmark自体も第三者再現契約ではない。 |

**未達のまま残る実用性ゲート: 2 / 9（#6, #9）**

## 最終判定

**この契約を凍結して実装に進んでよいか: NO**

BLOCKING 4件が残る。特に D2-B1 は、前回のarm非対称を形式上直したことで新たに生じた測定上の退行であり、現在の descriptive rectangle が明確な実益を表せない直接原因である。
