# Wave 1+2 最終閉鎖判定

判定: **BLOCKING 0 / SHOULD-FIX 0 / NOTE 3**

**Wave 1+2 を受け入れて Wave 3（C7 runtime v2）へ進んでよいか: YES**

R3 で残した `B-R2-3` と `S-R2-1` は CLOSED。R1 8件、R2 7件、R3 follow-up 2件の全17件に OPEN はない。以下の残る限界は受入阻害ではなく、現在の検証範囲を明示するものである。

## 直前2修正の確認

### B-R2-3 follow-up

追加された `invalid_relation_inputs_and_detached_endpoint_references_cannot_reach_synthesis` は名前と内容が一致している。zero target、duplicate relation、detached missing callee、detached missing caller を別々に検査する。missing callee では hard-coded relation ID と missing target ID を用い、`DomainError::DanglingReference` の `owner`、`owner_id`、`reference` を variant として直接 assert している。実装の定数や error string を oracle にしていない。

オーケストレーター確認済みの target-side `DanglingReference -> Validation` 変異による FAILED を採用した。さらに非重複の caller-side `DanglingReference -> Validation` 変異も同じ登録済み integration test が FAILED した。callee 修正が caller obstruction を弱めた形跡はない。

### S-R2-1 follow-up

`checked_v2_usize_add` は `left.checked_add(right)` の overflow を typed `DomainError::Incomplete` に変換し、operation、call-site limit、`usize::MAX` observed を保持する。C3 の candidate count、per-source count、replacement/total bytes、session index、partition、input bound、loss count がこの helper または引き続き checked な乗算・減算・変換を使う。新規 production path に saturating arithmetic は再導入されていない。

追加された `v2_checked_usize_add_reports_overflow_as_typed_incomplete` は `usize::MAX + 1` を直接与え、variant と3 field を literal で検査する。オーケストレーター確認済みの saturating 変異による FAILED を採用した。さらに helper が call-site limit を捨てる変異も同テストが FAILED した。helper 集約による error metadata の退行も検出される。

## 自身で行った非重複変異

| # | seam | 変異 | 結果 |
|---:|---|---|---|
| 1 | D candidate-space closure | `call_graph_complete: false` を `true` に変更 | `coverage_contract_keeps_candidate_space_gaps_out_of_resolved_target_denominator` FAILED (`true != false`) |
| 2 | D caller endpoint obstruction | caller-side `DanglingReference` を汎用 `Validation` に変更 | `invalid_relation_inputs_and_detached_endpoint_references_cannot_reach_synthesis` FAILED |
| 3 | C3 overflow metadata | `checked_v2_usize_add` が call-site `limit` を捨てて `usize::MAX` を返すよう変更 | `v2_checked_usize_add_reports_overflow_as_typed_incomplete` FAILED |
| 4 | legacy five-rule bytes | `relation.changed_call_contract@1` の weight `5.0` を `5.25` に変更 | `deterministic_reorder_and_fixed_fixture_bytes_are_stable` が固定 hash 不一致で FAILED |

4件すべて検出され、**自身の変異で未検出だった件数は 0**。4件目では semantic-count oracle は PASS したが、独立した canonical hash oracle が FAILED したため、テスト間の役割分担も確認できた。

全変異を都度復元した。終了時 SHA-256 は開始時と一致する。

- `synthesize.rs`: `ba9940f68d6e80f5a4052e619e8f05de602ab376f6a31b8ef669b1f34ee4b34f`
- `context.rs`: `12c2b8dfdc92ca5a55c0503c9e8562afeb64de24ea8b27e915c3e76d0fcbf45b`
- `changed_public_callee_rule.rs`: `8a25f9baa89b35b5f559749aa84d69ad301fc08f6ff2dfba31b7a9167f2023cf`

`git diff --check` も成功した。既存 dirty worktree、`benchmarks/`、git state は変更していない。

## 互換性・登録・回帰検査

- clean restoration 後の `cargo test -p reviewgraphen-core --quiet` は unit **436**、D rule **14**、context **9**、profile **40**、doc **65** がすべて PASS。
- core は `autotests = false` だが3 integration target は `Cargo.toml` に明示登録され、今回も14/9/40件が実行された。新しい helper test は lib unit count を435から436へ増やして実行されている。
- legacy five-rule の fixed semantic fixture と fixed canonical hash test は PASS。weight 変異を canonical hash が検出したため、単なる current-output 同士の比較ではない。ingest の pre-sidecar literal oracle はオーケストレーターの B-R2-6 変異結果を採用する。
- 型サイズ回帰テストは D integration 14件の一つとして PASS。R3 の inline 512-byte 増量変異で FAILED 済みであり、引き続き生きている。repository に `RUST_MIN_STACK`、thread `stack_size`、対象 `#[ignore]` はない。
- ADR 本文の single-line JSON から独立再計算した hash は profile `sha256:4b6cca93794ab03b1576e17d2e395ec43f731d316685247a89363ae2e840dd96`、context `sha256:7c4ceca165588cd38b28cc6882bb68a1ff4040dbb19ee34dfd792216d70bbf26`。test 側の独立 literals と一致する。
- ingest、verifier、fmt、clippy の全 PASS は指示書記載のオーケストレーター確認を採用した。

## 全17件の最終閉鎖表

| review | ID | 最終判定 | 閉鎖根拠 |
|---|---|---|---|
| R1 | B1 | CLOSED | v2 obstruction の inclusive end-column、多行、non-path cases が exact literal で検査される。 |
| R1 | B2 | CLOSED | v2 ingestion sidecar/API と legacy canonical state を分離し、新 v2 ingestion contract で authority 境界を固定。 |
| R1 | B3 | CLOSED | closed v2 records、semantic decoder、kind/description/ID-preimage validation が成立。 |
| R1 | B4 | CLOSED | 実 process-spawn 変異で複数 FAILED。返却値でなく副作用を検出。 |
| R1 | B5 | CLOSED | Stage 0 が exact `A_c`/`D_c`、global A/D、weights、closed reasons を保持・serialize。 |
| R1 | S1 | CLOSED | request/snapshot/universe namespace を resolver と record validator の双方で拒否。 |
| R1 | S2 | CLOSED | deferred fraction は checked exact integer comparison と typed overflow を使用。 |
| R1 | S3 | CLOSED | v2 mutation matrix と独立 pre-sidecar live/canonical literals が成立。 |
| R2 | B-R2-1 | CLOSED | D capability split の closed schema、semantic validation、field mutation matrixが成立。 |
| R2 | B-R2-2 | CLOSED | resolved-target denominator と candidate-space gap を別集合・別状態として保持。R3 の2変異と今回の completeness 変異を検出。 |
| R2 | B-R2-3 | CLOSED | zero/multiple/missing callee と missing caller が typed variant。target/caller 双方の generic-error 変異を検出。 |
| R2 | B-R2-4 | CLOSED | 8-clause trigger。オーケストレーターの clause 1、5+6 変異を含む matrix が成立。 |
| R2 | B-R2-5 | CLOSED | target relation と caller/callee subjects を結合。validator 無効化で core lib 10件 FAILED。 |
| R2 | B-R2-6 | CLOSED | pre-sidecar legacy/five-rule hashes は独立 literals。extractor-version 変異を検出。 |
| R2 | S-R2-1 | CLOSED | C3 additions/counters は checked helper へ集約され、overflow と metadata の双方を変異検出。 |
| R3 | B-R2-3 follow-up | CLOSED | missing accepted callee の exact `DanglingReference` assertion を登録・実行し、R3 の未検出穴を閉鎖。 |
| R3 | S-R2-1 follow-up | CLOSED | C3 overflow helper unit test を登録・実行し、R3 の未検出穴を閉鎖。 |

**OPEN: なし。**

## 残る限界（受入阻害ではない）

1. mutation sampling は代表 seam に対するもので、全 branch・全 checked call-site の exhaustive mutation score ではない。ただし今回の2修正については success/failure variant と metadata を直接検査し、過去の重点変異も併用している。
2. 型サイズ上限は carrier layout の膨張を止める回帰検査であり、全 toolchain・全 platform・将来の深い runtime call graph に対する形式的な最大スタック証明ではない。現 toolchain の既定スタックで core 全体が通ることと合わせて受け入れる。
3. legacy byte oracle は checked-in canonical fixture と固定 live-ingest sample を強く固定するが、任意の全 repository 入力に対する普遍的な observational-equivalence proof ではない。versioned fixture と今後の migration discipline で管理する範囲である。

以上の限界を明示した上で、Wave 1+2 の契約境界、回帰検出力、legacy compatibility は Wave 3 着手に十分である。
