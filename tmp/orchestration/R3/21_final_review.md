# Wave 1+2 最終独立レビュー

判定: **BLOCKING 1 / SHOULD-FIX 1 / NOTE 4**

**Wave 1+2 を受け入れて Wave 3（C7 runtime v2）へ進んでよいか: NO**

未閉鎖は `B-R2-3` と `S-R2-1`。実装の現値はどちらも ADR に沿っているが、要求された回帰検出力が missing-accepted-callee 分岐と C3 固有の checked arithmetic に存在しない。とくに前者は、汎用 `Validation` へ戻した変異を core の unit 435、integration 63、doc 65 の全テストが見逃したため、typed obstruction contract の閉鎖とは判定できない。

## BLOCKING

### B-R2-3 — missing accepted callee の typed obstruction 回帰が未検出

`exact_accepted_callee_id` の現実装は次を正しく返す。

- target 数 0/複数: `DomainError::Incomplete`
- sole target が accepted artifact に存在しない: `DomainError::DanglingReference`

zero/multiple は文字列ではなく variant と各 field を `matches!` しており、`Incomplete` を汎用 `Validation` に戻す変異で 0-target と 2-target の 2 テストが FAILED した。

しかし missing accepted callee については、integration test が detached relation の **source** を non-accepted にするだけで、sole **target** を non-accepted ID にするケースがない。target 側の `DanglingReference` だけを `DomainError::Validation("D calls relation missing accepted callee")` に戻したところ、core unit **435**、integration **63**、doc **65** がすべて PASS した。テスト名 `invalid_zero_target_and_duplicate_relation_inputs_cannot_reach_synthesis` の内容も missing callee を検査していない。

閉鎖には、accepted caller を保った detached relation の sole target を non-accepted ID に置換し、`owner`、`owner_id`、`reference` を含む `DomainError::DanglingReference` variant を直接 assert する登録済みテストが必要である。

## SHOULD-FIX

### S-R2-1 — C3 固有の checked arithmetic 回帰が未検出

R2 で指摘した実装箇所は修正済みである。`starts.len()` は `u32::try_from`、adjacency は `checked_add`、partition/input/loss counter は `checked_add` / `checked_mul` となり、ADR §5.1 の現行コード上の要求を満たす。

一方、C3 の `losses.len().checked_add(resolved_losses.len())` を `saturating_add` に戻す変異は、core unit **435** と `context_subject_windows` **9** がすべて PASS した。通常入力は先行 cap で小さいため、現行 fixture だけではこの退行を到達させられない。既存の汎用 `checked_resource_add` を `saturating_add` に戻す別変異は `resource_formula_refuses_each_checked_arithmetic_overflow_phase` が FAILED したが、これは旧 resource formula の helper であり C3 の新規演算を検査していない。

C3 の counter/addition を overflow 入力を直接与えられる checked helper に集約し、typed `Incomplete` を assert することを推奨する。

## 自身で行った一時変異

| # | 対象 | 変異 | 実行結果 | 判定 |
|---:|---|---|---|---|
| 1 | B-R2-2 denominator | D universe の `obligation_ids` を resolved IDs ではなく全 obligation IDs に変更 | `coverage_contract_keeps_candidate_space_gaps_out_of_resolved_target_denominator` FAILED。left は substantive+gap の2件、right は resolved 1件 | 検出 |
| 2 | B-R2-2 resolved/gap 分離 | `resolved_target_obligation_ids` の D-rule filter を削除して gap ID を resolved 側へ混入 | 同テストが resolved count `2 != 1` で FAILED | 検出 |
| 3 | B-R2-3 arity | zero/multiple の `Incomplete` を汎用 `Validation` に変更 | 0-target と 2-target の typed assertion がともに FAILED | 検出 |
| 4 | B-R2-3 missing callee | target 側 `DanglingReference` のみ汎用 `Validation` に変更 | core 435 + integration 63 + doc 65 が全 PASS | **未検出** |
| 5 | S-R2-1 C3 | `loss_count` の `checked_add` を `saturating_add` に変更 | core unit 435 + C3 integration 9 が全 PASS | **未検出** |
| 6 | S-R2-1 arithmetic control | `checked_resource_add` を `saturating_add` に変更 | overflow unit test が FAILED | 検出。ただし C3 固有ではない |
| 7 | stack regression | `UniverseDescriptor` に inline `[[u8; 32]; 16]`（512 bytes）を追加 | `coverage_extension_keeps_event_runtime_carriers_compact` が `UniverseDescriptor <= 256` で FAILED | 検出 |

**自身の有効な変異 7 件、未検出 2 件。**

全変異を個別に復元した。終了時の SHA-256 は開始時と一致した。

- `synthesize.rs`: `ba9940f68d6e80f5a4052e619e8f05de602ab376f6a31b8ef669b1f34ee4b34f`
- `context.rs`: `c3ecd7b4254d5fd267add0c8062ac258c145c103b0fade35b2a7b658def79b2b`
- `changed_public_callee_rule.rs`: `e914cc73be1f14618022ec98125e0d1842622a0266b72f257cce83a131d0ba3f`

`git diff --check` も成功した。既存の dirty worktree と `benchmarks/` は変更していない。

## スタックオーバーフロー回帰

修正方針は妥当である。大きい D-only coverage body は `Option<Box<DTwoLayerCoverage>>` として間接保持され、イベント回復経路へ inline に複製されない。repository 内（`target/`、`benchmarks/`、`tmp/` を除外）に `RUST_MIN_STACK`、thread `stack_size`、対象テストの `#[ignore]` はない。

型サイズ上限テストは登録済み integration test として実行され、現値で PASS する。512-byte inline field の追加で実際に FAILED したため、単なる名目上の assertion ではない。スタック量を環境変数で隠す対処ではなく、carrier の layout 増大を直接止めている。

## DTO hash と legacy byte compatibility

ADR-0038 本文の single-line JSON だけを抽出し、実装定数を使わず SHA-256 を再計算した。

| DTO | ADR bytes | 独立計算結果 | test 側 oracle |
|---|---:|---|---|
| `rust.production.v1` | 1,582 | `sha256:4b6cca93794ab03b1576e17d2e395ec43f731d316685247a89363ae2e840dd96` | `review_profile.rs` の独立 literal と一致 |
| `context.subject_windows@2` | 1,885 | `sha256:7c4ceca165588cd38b28cc6882bb68a1ff4040dbb19ee34dfd792216d70bbf26` | `context_subject_windows.rs` の独立 bytes/hash literals と一致 |

clean restoration 後の `cargo test -p reviewgraphen-core --tests` は unit **435**、明示登録された integration **14 + 9 + 40 = 63** がすべて PASS した。`reference_obligation_semantics_are_a_nine_item_oracle_of_five_concrete_and_four_gaps` と `deterministic_reorder_and_fixed_fixture_bytes_are_stable` も個別に PASS し、既存5ルールの semantic set と v1 canonical fixture bytes は不変である。B-R2-6 の独立 frozen live-oracle mutation 結果は指示書の前提どおり採用した。

## R1 + R2 最終閉鎖判定

| ID | 最終判定 | 根拠 |
|---|---|---|
| R1-B1 | CLOSED | inclusive end-column literal、多行/non-path cases。R2 で閉鎖確認済み。 |
| R1-B2 | CLOSED | v2 ingestion sidecar/API と legacy canonical state が分離。新 v2 ingestion contract により ADR 欠落も裁定済み。 |
| R1-B3 | CLOSED | closed v2 records と semantic/ID-preimage validation が存在。 |
| R1-B4 | CLOSED | オーケストレーターの実 process-spawn 変異で複数 FAILED。返却値でなく副作用を観測。 |
| R1-B5 | CLOSED | Stage 0 が exact `A_c`/`D_c`、global A/D、weights、closed reasons を保持。 |
| R1-S1 | CLOSED | request/snapshot/universe namespace を resolver/record validator の双方で拒否。 |
| R1-S2 | CLOSED | exact checked integer comparison と typed overflow。 |
| R1-S3 | CLOSED | v2 mutation matrix に加え、B-R2-6 の独立 literal live oracle が成立。 |
| B-R2-1 | CLOSED | D split fields を受理する closed v2 schema と field mutation matrixが登録済み。現行 D contract が schema-valid。 |
| B-R2-2 | CLOSED | resolved denominator、candidate-space gap、enumeration state/limitations、stage sets が別フィールド。2つの独立変異を検出。 |
| B-R2-3 | **OPEN** | 現実装は typed だが、missing accepted callee の target-side variant 回帰を全テストが見逃す。 |
| B-R2-4 | CLOSED | 指示書の clause 1 および 5+6 独立変異結果を採用。残り clause matrix も登録済み。 |
| B-R2-5 | CLOSED | `validate_subject_binding_v2` の全面無効化で core lib 10件 FAILED。 |
| B-R2-6 | CLOSED | extractor-version frozen literal oracle 変異が FAILED。legacy/v2 current-output 循環を解消。 |
| S-R2-1 | **OPEN** | 現行演算は checked だが、C3 固有の saturating 回帰が未検出。 |

## NOTE

1. B-R2-2 の検査は「gap ID を expected set に冪等 extend」する変異ではなく、production constructor の universe denominator と resolved set を別々に破壊したため、二層分離の両側を実際に観測している。
2. core は `autotests = false` だが、3 integration target は `Cargo.toml` に明示登録され、実行時にも 14/9/40 件が列挙された。
3. profile/context golden は ADR 本文由来の独立 literal であり、実装定数だけを比較する循環ではない。
4. formatter、clippy、ingest、verifier の全 pass は指示書に記載されたオーケストレーター確認を採用した。今回の一時変異は core の指定 seam のみに限定した。
