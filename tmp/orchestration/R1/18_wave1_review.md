# ReviewGraphen Rust 実装 Wave 1 独立コードレビュー

- Reviewer: R1 (独立レビュー。実装者ではない)
- 対象: C1 ingest / C5 verifier / C6 profile
- 正典: `docs/adr/0038-changed-public-callee-relation-slice.md`
- 判定日: 2026-08-23
- 実装変更: なし。一時変異と検出 probe は全て復元済み。

## 結論

| Severity | 件数 |
| --- | ---: |
| BLOCKING | 5 |
| SHOULD-FIX | 3 |
| NOTE | 6 |

**Wave 1 を受け入れて Wave 2 へ進んでよいか: NO**

対象テスト 14 / 29 / 35、既存 v1 canonical fixture の静的 oracle 2 件、scoped rustfmt、`git diff --check`、`scripts/validate_bundle.py` は pass した。しかし、C1 の exact span、v1/v2 version boundary、closed-record validation、C5 security test の検出力、C6 Stage 0 denominator/planning preservation に BLOCKING がある。

## BLOCKING findings

### B1 — C1: v2 obstruction の end column が inclusive ではなく 1 文字先になる

- `crates/reviewgraphen-ingest/src/rust.rs:1175-1184` は `Span::end()` の zero-based exclusive column に `+1` して `end_column` としている。
- 1 行 fixture `pub fn caller() { nowhere(); }` に独立な exact assertion を一時追加したところ、正しい inclusive 範囲は `start_column=19, end_column=27` だが、実装は `end_column=28` を返した。
- `crates/reviewgraphen-ingest/tests/changed_public_callee_facts.rs:132-134` は `end_column >= start_column` しか確認せず、このずれを検出しない。
- span は obstruction ID preimage に入るため、source trace だけでなく stable ID も誤った座標へ固定される。

ADR 0038 §3.2 の「exact normalized source path and inclusive start/end line and column」と Required verification 5 に不適合。

### B2 — C1: v2 obstruction を unversioned に v1 canonical state へ混入している

- `crates/reviewgraphen-ingest/src/rust.rs:199-215` は全 Rust extraction に mandatory な v2 global obstruction を追加する。
- `crates/reviewgraphen-ingest/src/lib.rs:1506-1694` は occurrence/global v2 draft を legacy と共通の `limitation_by_id` と `obstruction_by_id` に挿入する。
- `crates/reviewgraphen-ingest/src/lib.rs:1731-1736` はそれらを core `Extraction.limitations` に入れ、`crates/reviewgraphen-ingest/src/lib.rs:1801-1812` は追加 record を `reviewgraphen.extraction_report.v1` として出力する。新フィールドを `serde(skip)` にしても、追加 record 自体は v1 bytes に現れる。
- Rust adapter descriptor は引き続き version `"1"` (`crates/reviewgraphen-ingest/src/rust.rs:232-238`) である。

したがって、同じ snapshot/profile/rule/extractor-version tuple で legacy extraction limitations が増え、`direct_calls` を要求する既存ルールの qualification IDs、universe limitation IDs、contract/universe canonical bytes が変わり得る。既存 fixture ファイル自体の oracle は pass するが、live v1 ingest→synthesize compatibility は検証されていない。

ADR 0038 §2.1 の既存 5 ルール/v1 bytes 不変、§3.2 の v1 obstruction serialization 不変、§8.5 の v1 read/replay compatibility に不適合。v2 record は versioned な別 projection/contract に隔離するか、明示的な extractor major/version boundary が必要。

### B3 — C1: closed v2 occurrence record の ID/description semantic validation が存在しない

- `IngestionObstructionV2` の全フィールドは public で、`Serialize` のみ (`crates/reviewgraphen-ingest/src/lib.rs:313-344`)。
- `call_occurrence_v2()` は optional field が揃えば record をそのまま投影し、ID、schema、description、field closure を再検証しない (`crates/reviewgraphen-ingest/src/lib.rs:289-311`)。
- occurrence ID preimage (`crates/reviewgraphen-ingest/src/lib.rs:1552-1602`) は kind/severity/sources/paths/capability/call kind/reason/span/extractor を含むが、record の `schema` を含まない。
- description は ingest 内で deterministic に render されるものの、投影 record に対して byte-for-byte 再構築・検証する API/decoder/schema mutation test がない。

ADR 0038 §3.2 の「ID preimage must bind the complete record except description」「description ... must validate byte-for-byte」と Required verification 5 に不適合。

### B4 — C5: security test が実プロセス起動を観測せず、危険な変異を検出できない

- hostile fixture helper は fixture 文字列が非空であることだけを確認し、文字列を seam へ渡さない (`crates/reviewgraphen-verifier/tests/deferred_workspace_seam.rs:60-85`)。
- `malicious_cargo_inputs_share_the_same_no_io_typed_result` も各文字列を `_untrusted_executable_input` として無視し、hard-coded getter の `process_started=false` / `executable_resolved=false` を確認するだけ (`.../deferred_workspace_seam.rs:384-419`)。
- 一時変異として `resolve_deferred_workspace_verifier` 冒頭に `std::process::Command::new("true").status()` を挿入した。29 テストは **全 pass** した。

現行 production seam 自体には process API がなく、型は execution-control data を保持しない点は良い。しかし Required verification 8 は「never resolves or spawns a process」「malicious fixtures ... same no-I/O result」を実際に証明するテストを要求する。現在のテストは返却 record と実際の副作用を循環的に同一視しており、security regression detector になっていない。

### B5 — C6: Stage 0 / overflow planning contract が必要な denominator 情報を表現・保持できない

- `Stage0Cluster` は applicable/deferred ID set だけで、deferred reason と obligation weight を持たない (`crates/reviewgraphen-core/src/profile.rs:721-730`)。
- `Stage0Gates` は global union と per-cluster count のみを返し、各 cluster の exact `A_c` / `D_c` set を捨てる (`.../profile.rs:741-760,804-831`)。
- fixed 300 cluster domain について、個数と ID 重複だけを検証し、exact frozen cluster set や cluster ID namespace を検証しない (`.../profile.rs:766-803`)。
- `planning_retains_deferred_ids_in_the_applicable_denominator` は ID subset しか確認せず、weight `4.0` と typed reason `budget_exhausted | prerequisite_deferred` を検証できない (`crates/reviewgraphen-core/tests/review_profile.rs:500-510`)。

この型では「overflow 候補が ID と weight を保って deferred」「C、every A_c/D_c、A、D を serialize」を満たす canonical artifact を構築できない。ADR 0038 §4.3 と Required verification 6 に不適合。

## SHOULD-FIX findings

### S1 — C5: request/snapshot/universe ID namespace を検証していない

`resolve_deferred_workspace_verifier` と `DeferredWorkspaceUnsupportedRecord::validate` は StableId grammar は前提にするが、`request_id.kind()=="request"`、`snapshot_id.kind()=="snapshot"`、`universe_id.kind()=="universe"` を検証しない (`crates/reviewgraphen-verifier/src/lib.rs:253-299,377-411`)。異種 ID でも canonical unsupported record を生成できるため、source/identity closure を fail-closed にすべき。

### S2 — C6: exact integer comparison に saturating arithmetic を使っている

ADR は `20 * |D| <= |A|` の exact integer comparison を要求するが、`crates/reviewgraphen-core/src/profile.rs:820-821` は `saturating_mul(20)` を使う。現実的な allocation では overflow 到達は困難でも、contract は exact/checked arithmetic を要求する。overflow は pass/fail へ丸めず typed invalid/incomplete にすべき。

### S3 — C1: Required verification 5 の mutation matrix が不足している

新規 14 テストは method/cross-crate/macro/ambiguous/glob/shadowed/zero/multiple/public/partial/global limitation/determinism を確認する一方、少なくとも次を独立に検証していない。

- exact start/end line/column（B1 を見逃した）
- `DirectNonPath` と `DirectEmptyPath`
- schema / ID-bound field / deterministic description の mutation rejection
- v2 records を加えても live v1 canonical bytes と既存 5-rule output が不変であること
- input vector reorder 後の obstruction IDs/canonical bytes

## NOTE

1. **C6 golden hash は循環していない。** `crates/reviewgraphen-core/tests/review_profile.rs:11-12` に独立 literal `sha256:4b6cca...dd96` がある。実装定数を参照していない。
2. ADR §4 の fenced DTO 本文から実装/example を参照せず独立抽出した 1,582 UTF-8 bytes の SHA-256 は `4b6cca93794ab03b1576e17d2e395ec43f731d316685247a89363ae2e840dd96`。ADR declared hash および newline なし example bytes と一致した。
3. `public` は `matches!(visibility, Visibility::Public(_))` のまま (`crates/reviewgraphen-ingest/src/rust.rs:963-970`)。`pub(crate)` / `pub(super)` / `pub(in crate)` は false。visibility broadening 変異はテストが検出した。
4. `direct_calls` は partial のまま (`crates/reviewgraphen-ingest/src/rust.rs:164`)。located occurrence と global limitation の `related_capabilities` は exact `direct_calls`、global macro latent count は `unknown`。断定的な `breaking_call` / `contract_violation` / `unsafe_bug` 命名は対象差分にない。
5. 既存 `relation.changed_call_contract@1` / payment semantics の source contract と checked-in canonical fixture は変更されておらず、静的 fixture oracle 2 件は pass。ただし B2 の live ingest compatibility をカバーしない。
6. C5 production seam の現行コードはプロセスを起動せず、command/executable/argv/path/cwd/environment/mount/cache/credential/toolchain/resource-limit/identity/unknown field を typed schema error にする。B4 はその重要な性質を regression test が証明できていない問題。

## 単位別 ADR 適合判定

| Unit | ADR / Required verification | 判定 | 根拠 |
| --- | --- | --- | --- |
| C1 ingest | §3.2 / item 5 | **FAIL** | partial/global obstruction/public visibility は適合。exact span、v1 isolation、closed record validation が不適合 (B1-B3)。 |
| C5 verifier | §6 / item 8 | **FAIL** | production seam は structurally process-free だが、no-process security test が副作用を観測せず process-spawn mutant を見逃す (B4)。 |
| C6 profile | §4 / item 6 | **FAIL** | canonical profile/hash/path/matcher/exclusion ID は概ね適合。Stage 0 per-cluster denominator と deferred weight/reason preservation が欠落 (B5)。 |

## AGENTS.md コードレビュー確認順

| # | 観点 | 判定 | 確認結果 |
| ---: | --- | --- | --- |
| 1 | 境界違反 | PASS | C1 は Program facts/limitations、C5 は unsupported non-authority record、C6 は profile/exclusion/planning inputs。claim/evidence/accepted promotion 経路は追加されていない。 |
| 2 | ID 安定性と versioning | **FAIL** | exact span が ID に誤って入り (B1)、v2 facts が adapter v1 canonical state を変え (B2)、occurrence schema が ID に未結合 (B3)。 |
| 3 | accepted / inferred / verified 状態遷移 | PASS | C5 unsupported を observation/evidence/verification/pass に昇格する API はない。confidence promotion もない。 |
| 4 | source trace | **FAIL** | C1 source IDs/path は保持するが exact inclusive span が誤り、closed record の再検証もない (B1/B3)。 |
| 5 | staleness propagation | N/A (scoped) | Wave 1 の 3 単位は staleness transition を追加しない。stale evidence を sign-off に使う変更もない。 |
| 6 | coverage denominator | **FAIL** | direct_calls partial/global unknown は保持する一方、v1 limitation universe 汚染 (B2) と Stage 0 A_c/D_c・weight/reason 消失 (B5) がある。 |
| 7 | security boundary | **FAIL** | production seam の型は狭いが、必須の実プロセス非起動 test が mutation を検出できない (B4)。 |
| 8 | performance / incrementality | SHOULD-FIX | B2 は unresolved occurrence を legacy+v2 の両方へ積み増す。C6 は exact arithmetic でなく saturating arithmetic (S2)。 |
| 9 | projection 表現 | **FAIL** | C1 v2 closed projection の semantic validator 不在 (B3)、C6 Stage 0 projection に per-cluster sets/weight/reason がない (B5)。 |

## 循環・自己参照テスト

| 対象 | 判定 | 説明 |
| --- | --- | --- |
| C6 golden hash | **循環なし** | テスト側独立 literal。ADR 本文からの独立 SHA-256 と一致。 |
| C5 hostile no-I/O | **実質的な自己参照あり** | fixture を seam に渡さず、record 自身の hard-coded false getter を確認するため、実 process spawn を観測しない。 |
| 全体 | **循環/tautological test あり** | C5 の 1 test family。C6 golden にはない。 |

## 一時変異と検出結果

| Unit | 一時変異 / probe | 期待 | 結果 |
| --- | --- | --- | --- |
| C1 | function `public` を restricted visibility でも true に broaden | `pub(crate)` 等の fixture が失敗 | **検出**。`crate::crate_visible` が true となり test failure。 |
| C5 | resolver 冒頭で harmless external process `true` を起動 | no-process test が失敗 | **未検出**。29/29 pass。 |
| C6 | implementation profile hash の末尾を `...dd96`→`...dd97` | 独立 golden が失敗 | **検出**。canonical profile test failure。 |
| C1 probe | exact inclusive end column を 27 と独立 assertion | current output が一致 | **不一致を検出**。actual 28 (B1)。 |

- 仕掛けた implementation 変異: 3
- 検出された変異: 2
- **未検出変異: 1**

## 復元確認

一時変異を加えたファイルの終了時 SHA-256 は開始時と一致した。

| File | SHA-256 (開始時 = 終了時) |
| --- | --- |
| `crates/reviewgraphen-ingest/src/rust.rs` | `4f25fed0a600ee215de6b72438947c7ed2f3624037335c15b924957617c498b3` |
| `crates/reviewgraphen-ingest/tests/changed_public_callee_facts.rs` | `86cb2532876d9e090477eaf449c25b57bd172d886cd9512e962336bb0c6d6dd6` |
| `crates/reviewgraphen-verifier/src/lib.rs` | `69d286f15f88a91976a4963f6e7da08de71934a800e10f4b068d2399ccd2f9d3` |
| `crates/reviewgraphen-core/src/profile.rs` | `3692cb1526bfdf18db3154dcd7174faf29242087e864b2600cf86abb09accf37` |

`git diff` から `Command::new("true")`、visibility broadening、temporary span probe が消えていることも確認した。`crates/reviewgraphen-core/src/lib.rs` は開始/終了とも `29540337120a4e02f2a90fe7fb18af8675e654ec55343b41cd84829caaf54635` で不変。対象外 `context.rs` は別単位の並行編集でレビュー中に hash が変わったが、本レビューは一度も変更・復元していない。

## 実行した検証

- `cargo test -p reviewgraphen-ingest --test changed_public_callee_facts -- --test-threads=1`: 14 pass
- `cargo test -p reviewgraphen-verifier --test deferred_workspace_seam`: 29 pass
- `cargo test -p reviewgraphen-core --test review_profile`: 35 pass
- legacy `reviewed_obligation_example_is_the_complete_canonical_fixture_oracle`: pass
- legacy `migration_record_matches_the_checked_in_canonical_fixture_byte_for_byte`: pass
- ADR profile JSON 独立 SHA-256: pass (`1,582 bytes`, `4b6cca...dd96`)
- target-scoped `rustfmt --check`: pass
- target-scoped `git diff --check`: pass
- `python3 scripts/validate_bundle.py`: PASS

`cargo fmt --all -- --check` の workspace-wide failure は事前情報どおり C3 編集中の `context.rs` / `src/lib.rs` に限定されるため、Wave 1 対象の判定には用いていない。

## 最終判定

**Wave 1 を受け入れて Wave 2 へ進んでよいか: NO**

少なくとも B1-B5 を解消し、C1 の live v1 byte-compatibility oracle、C5 の実 process-spawn 検知、C6 の exact per-cluster/weight/reason serialization を含む独立 mutation tests が pass してから再レビューすべき。
