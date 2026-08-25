# Wave 1 修正 + Wave 2 独立コードレビュー

判定: **BLOCKING 6 / SHOULD-FIX 1 / NOTE 3**

**Wave 1+2 を受け入れて Wave 3（C7 runtime v2）へ進んでよいか: NO**

レビュー対象は R2 指示書、必須設計文書、ADR-0038、指定された Wave 1 修正および C2/C3 実装である。実装変更は行っていない。一時変異はすべて復元し、開始時に採取した対象 13 ファイルの SHA-256 と終了時の SHA-256 が全件一致し、`git diff --check` も成功した。`benchmarks/` には触れていない。

## BLOCKING

### B-R2-1: D の capability split 出力が現行 v2 schema 自身に拒否される

- `synthesize.rs:1684-1697` は D substantive obligation だけをルール名で特別扱いし、`required_capabilities = None`、`target_support_capabilities`、`enumeration_capabilities` を投影する。
- しかし `schemas/reviewgraphen.obligation.schema.json:414-439` は `required_capabilities` を必須とし、split fields を定義していない closed schema である。
- 実際の D bundle をこの schema に通す一時検査は `AdditionalProperties { unexpected: ["enumeration_capabilities"] }` で FAILED した。通常の 9 テストは schema validation を一度も行っていない。
- ADR §2 が要求する専用 split spec、両 field の必須・非 null・sorted/unique/disjoint validation、legacy rule の v2 split projection、rule-level gap の分類がない。内部では D も legacy `Obligation.required_capabilities` に押し戻され、D gap は再び ambiguous `required_capabilities` として serialize される。

したがって C2 の public contract は closed v2 contract として成立していない。

### B-R2-2: 二層 denominator が実装されていない

- `synthesize.rs:1156-1177` は D substantive obligations と D capability-gap obligation を一つの `obligation_ids` に集め、そのまま `UniverseDescriptor` を構築する。
- resolved-target obligation IDs と candidate-space-gap IDs の別集合、enumeration capability state/limitation/obstruction closure、resolved-target stage numerator 集合は C2 の型・出力に存在しない。
- そのため「accepted syntactic-unique target の coverage」と「未知の candidate space」を別々に閉じられず、gap ID が resolved-target denominator に混入する。

これは ADR §3 と Required verification 5 の中心的不変条件に反する。

### B-R2-3: malformed target は typed synthesis obstruction ではなく文字列入り汎用 validation error

- `synthesize.rs:1187-1204` は zero/multiple/missing accepted callee を `DomainError::Validation(format!("typed synthesis obstruction: ..."))` で返す。専用 error variant または retained obstruction recordではない。
- zero target は `ProgramSpace` decode で先に拒否され、`changed_public_callee_rule.rs:289-315` も synthesis obstruction を検査していない。
- multiple target test は error string に `typed synthesis obstruction` が含まれることしか検査しない。

AGENTS.md の「error string だけでなく typed error/Obstruction」と ADR §1 clause 4 を満たさない。

### B-R2-4: 8 clause 個別検査が成立しておらず、2つの trigger 破壊を見逃す

- clause 1 の `relation.kind == "calls"` を削除する変異で C2 の 9 テストは **9/9 pass** した。
- changed caller を containment witness の代替として認める変異でも **9/9 pass** した。
- `changed_caller_or_callee_attribute_does_not_substitute_for_changed_containment` というテスト名に反し、fixture の caller `function:checkout-submit` には `changed=true` が設定されていない。
- 非 function/private/missing resolution/non-unique resolution は検査するが、relation kind、実 changed-caller-only、各 target-support capability の partial/missing/unknown、exact qualification trace、同 endpoint の duplicate observation、capability field mutation matrixは個別に閉じていない。

実装の現行 trigger は containment-only で正しいが、Required verification 1/3/4 を満たす検出力がないため受入条件を満たさない。

### B-R2-5: C3 は target relation と caller/callee subjects の結合を検証しない

- `context.rs:4852-4857` の public `prepare_subject_windows_v2` は obligation ID と caller/callee ID を独立引数として受ける。
- obligation が D rule/property/one relation target であること、引数がその accepted relation の source/sole target であることを検証しない。
- `context.rs:4900-4986` は渡された ID をそのまま callee/caller expectation にする。非 accepted ID すら domain error ではなく `MissingSource` subject loss に変換する。
- 実際に内部テスト `v2_session_names_missing_source_and_location_subjects` は任意の legacy obligation と `function:not-accepted` を組み合わせて成功させている。

よって C7 が誤った endpoint を渡しても envelope は「actual D caller/callee が含まれるか typed loss で名指しされる」という subject guarantee を満たしたように見える。projection identity が target refs と任意 subject IDs の両方を hash しても、意味的結合の欠落は直らない。

### B-R2-6: 「frozen live oracle」が循環し、pre-sidecar/v1 byte compatibility を検証していない

- `changed_public_callee_facts.rs:413-437` は現行 `ingest()` の出力から `frozen_live_oracle = sha256(legacy_bytes)` をその場で計算し、同じ現行出力系列と比較する。独立 literal ではない。
- `changed_public_callee_facts.rs:439-446` の「five legacy rules」も同じ実行で得た current `MvpRulePack::synthesize` 同士の比較で、pre-sidecar の literal bytes/hash ではない。
- したがって legacy API と v2 wrapper が同じ方向に回帰した場合、または既存5ルールが同じ current implementation から変化した場合に検出できない。ADR Required verification 15 が明示的に要求する pre-sidecar frozen live-oracle hashes を満たさない。

一方、checked-in core fixture 自体は変更されておらず、既存 golden test は pass している。この finding は「ordinary live ingestion + five-rule synthesis」の独立 oracle が循環している点である。

## SHOULD-FIX

### S-R2-1: C3 の新規経路に unchecked/saturating counter arithmetic が残る

- `context.rs:4043` は `starts.len() as u32` で line count を縮小変換する。
- `context.rs:4095-4096` は adjacency 判定で `saturating_add(1)`、`context.rs:4613` と `4749-4753` も saturating arithmetic を使う。
- 固定 session bounds 下で多くは実到達しにくいが、public low-level resolver も含め ADR §5.1 の「every counter and addition must be checked」を型上保証していない。

## NOTE

### N-R2-1: C3 golden policy は独立に正しい

ADR 本文の JSON code block のみを抽出し、実装定数もテスト定数も使わず SHA-256 を計算した。exact 1,885 bytes から得た値は次であった。

`sha256:7c4ceca165588cd38b28cc6882bb68a1ff4040dbb19ee34dfd792216d70bbf26`

`context_subject_windows.rs:5-8` は bytes と hash を実装とは別の literal で保持しており、循環していない。

### N-R2-2: テスト登録漏れはない

core は `autotests = false` だが、`review_profile`、`context_subject_windows`、`changed_public_callee_rule` は `Cargo.toml:24-34` に明示登録されている。legacy `m1.rs` は `src/lib.rs:30-36` から lib test module として登録されている。実行時にも core 424、C2 9、C3 9、profile 40 が列挙された。

### N-R2-3: B4 detector は返却値ではなく実副作用を観測する

オーケストレーターの process-spawn mutant 結果を前提として確認し、こちらでも verifier seam 35 tests を実行した。PATH shim の marker file と独立 positive control があり、単なる `process_started=false` の返却値比較ではない。

## R1 指摘の閉鎖判定

| R1 ID | 判定 | 根拠 |
|---|---|---|
| B1 | CLOSED | オーケストレーターの独立変異確認を採用。exact inclusive span literal、多行、non-path case がある。 |
| B2 | CLOSED | v2 sidecar type/API が legacy result と分離され、v2 IDs が v1 report/ProgramSpace limitationへ入らない。循環 live oracle は新たに B-R2-6 とした。 |
| B3 | CLOSED | closed v2 records、semantic decoder、schema/kind/description/ID-preimage validation が入り、schema discriminator も ID bindings に含まれる。 |
| B4 | CLOSED | marker-file process canary と positive control が実 process side effect を検出する。 |
| B5 | CLOSED | Stage0 cluster は exact `A_c`/`D_c` map、各 weight、closed deferral reason、global A/D を保持・serialize する。 |
| S1 | CLOSED | request/snapshot/universe の namespace を resolver と record validator の双方で拒否するテストがある。 |
| S2 | CLOSED | `deferred_fraction_gate` は `checked_mul(20)` と typed overflow を使用する。 |
| S3 | PARTIALLY-CLOSED | v2 ingest mutation casesは増えたが、live oracle が current output 由来で、Required verification 15 の literal pre-sidecar oracle/cross-collection 全行列は未閉包。 |

**R1 OPEN: 0、PARTIALLY-CLOSED: 1。**

## C2 ADR 適合判定

| 項目 | 判定 | 根拠 |
|---|---|---|
| §1 exact trigger implementation | PARTIAL | 現行実装の clauses 1-8 と containment-only predicate は読めるが、malformed target が typed obstruction でなく、個別 test が2変異を見逃す。 |
| §1 identity/provenance | PASS | relation ID が sole target identity。caller/callee/change artifacts/contains witnesses は generator/source provenanceで、change witness は target identity に入らない。weight 4.0、evidence modes、no dependency も一致。 |
| §2 capability split | FAIL | D output が現行 schema-invalid。専用 split construction/semantic validatorがなく、D gap と legacy v2 projectionも ambiguous fieldを残す。 |
| §3 two-layer denominator | FAIL | substantive IDs と gap IDs が単一 universe denominatorに混在し、per-rule candidate-space closureがない。 |
| §4 profile/Stage0 | PASS | profile golden/matcher precedence/exclusion identity、exact Stage0 sets、p95、checked 5% comparisonが実装・テストされる。 |
| Required verification 1 | FAIL | relation-kind と real changed-caller mutationが未検出。その他の個別 matrix も不足。 |
| Required verification 2 | PARTIAL | reordered program の universe/contract bytes は一致するが、exclusion caseを含む全指定 ID/body の独立 oracleではない。 |
| Required verification 3 | PARTIAL | `ast=partial` と `direct_calls=partial` のみ。3 support capabilities × partial/missing/unknown と exact trace を閉じていない。 |
| Required verification 4 | FAIL | capability field schema/mutation matrixがなく、そもそも D output が schema-invalid。core checked-in legacy fixture test は pass。 |
| Required verification 5 | FAIL | v2 ingest occurrence factsはあるが、resolved-target/candidate-space run sets と exact rebuild validator は C2 にない。 |
| Required verification 6 | PASS | profile/Stage0 の golden、mutation、exact/+1/zero tests は通る。 |
| 既存 `relation.changed_call_contract@1` semantics | PASS | rule/property/weight/contract branchは未変更で、D は別 ID/property。断定的 `breaking_call` 等もない。 |
| checked-in v1 canonical fixtures | PASS | 対象 fixture/schema に `git diff` はなく、full core golden/replay tests が pass。ただし live pre-sidecar oracle は B-R2-6。 |

## C3 ADR 適合判定

| 項目 | 判定 | 根拠 |
|---|---|---|
| exact policy DTO/hash | PASS | ADR 本文からの独立計算とテスト側独立 literals が一致。mutated/duplicate fieldも拒否。 |
| ordered resolver protocol | PASS | v2 session は pending request、ordinal/digest、逐次 submit、length/hash/CAS/line-count validationを行い、raw bytesを built value に残さない。 |
| v1 resolver path separation | PARTIAL | v2 session/finish は別型で `prepare_context` を呼ばないが、discovery/candidate helpers と `ContextSourceRequest` を共有する。C7 dispatch がまだないため end-to-end の「v1 path 不使用」はこの wave だけでは証明されない。 |
| subject guarantee | FAIL | caller/callee と target relation の意味的 binding がなく、任意/nonaccepted endpointで成功できる（B-R2-5）。 |
| window ordering/merge/caps | PARTIAL | role priority、same-file adjacent/disjoint、overlap loss、line/byte/per-file/total capsの代表例はある。Required verification 7 の cross-file、全 BFS token/depth/tie-break、anchor denominator、path/test selection、全 exact/+1、全 identity/order/loss mutationは v2 pathで閉じていない。 |
| v1 replay byte identity | PARTIAL | v1 implementation hunksは変更されず、fixed v1 policy hashと full legacy context tests は pass。ただし C3 tests に pre-C3 v1 envelope bytes の独立 literal replay oracleはない。 |
| Required verification 7 | FAIL | 上記 matrix と endpoint-binding test が不足する。 |

## 循環・登録・detector・過剰停止

| 失敗パターン | 判定 |
|---|---|
| テストと実装の循環 | **あり**。B-R2-6 の live oracle。C3 golden は循環なし。 |
| 返却値と副作用の同一視 | **なし**。B4 canary は marker file と positive control を使用。 |
| テスト未登録 | **なし**。core の3 integration testsと legacy lib testsを実行確認。 |
| 仕様欠落時の過剰停止 | **確認せず**。独立な Wave 1/C2/C3 検査は継続できた。 |

## 実行結果

- `cargo test -p reviewgraphen-core`: 424 lib + C2 9 + C3 9 + profile 40 + doc 65、すべて pass。
- `REVIEWGRAPHEN_TRUSTED_CARGO=$(scripts/resolve-trusted-cargo.sh) cargo test -p reviewgraphen-ingest --test changed_public_callee_facts -- --test-threads=1`: 19 pass。
- `cargo test -p reviewgraphen-verifier --test deferred_workspace_seam -- --test-threads=1`: 35 pass。
- D bundle を現行 v2 schema へ通した一時検査: expected FAILED（B-R2-1）。
- checked-in v1 obligation/report fixtures と `examples/double-submit-payment` に対する `git diff --exit-code`: success。

## 一時変異と検出結果

| # | 変異 | 実行した suite | 結果 |
|---:|---|---|---|
| 1 | callee 自身の `attributes.changed` を containment の代替にする | C2 9 tests | **検出**。1 failed。 |
| 2 | `direct_calls` を D target-support capabilities に混入する | C2 9 tests | **検出**。2 failed。 |
| 3 | C3 window priority を caller-before-callee に反転する | C3 9 tests | **検出**。1 failed。 |
| 4 | trigger clause 1 の `kind == calls` を削除する | C2 9 tests | **未検出**。9/9 pass。 |
| 5 | changed caller を containment witness の代替にする | C2 9 tests | **未検出**。9/9 pass。 |

**実装変異 5、未検出 2。** 一時 schema assertion は診断 probe であり、実装変異数には含めていない。全変異と probe を `apply_patch` で復元した。開始時/終了時 SHA-256 は以下の全対象で一致した: core Cargo.toml/synthesize.rs/C2 test/context.rs/C3 test/lib.rs/profile.rs/profile test、ingest lib.rs/rust.rs/facts test、verifier lib.rs/seam test。
