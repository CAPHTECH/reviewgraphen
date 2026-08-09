# 15. Storage and Repository Layout

> Status: Draft v0.1  
> Decision: local-first event log + content-addressed artifacts + derived index

## 1. Canonical state

ReviewGraphenのcanonical semanticsはHigherGraphen上のtyped structuresとdomain eventsです。

Markdown、human report、AI summary、dashboard、YAML manifestはprojectionです。これらを真のレビュー状態にしません。

永続表現としては、append-only event logを正本記録にし、SQLite等を再構築可能な派生indexとします。

## 2. Workspace layout

```text
.reviewgraphen/
  config.toml
  config.local.toml          # gitignore

  profiles/
    code-review.toml

  policies/
    default.toml

  store/
    events/
      01K...jsonl
    snapshots/
      <snapshot-id>.json
    checkpoints/
      <event-seq>.json

  artifacts/
    sha256/
      ab/
        <hash>

  indexes/
    reviewgraphen.sqlite

  runs/
    <sha256(canonical-run-id)-full-hex>/
      logs.jsonl
      recovery/
        intents/
        completions/

  reports/
    <run-id>/
      report.json
      human-review.md
      audit-trace.json
```

## 3. Event log

各 run の `logs.jsonl` は一 event 一行の JSONL。V2 は空ファイルを
永続状態として認めず、sequence 1 の `RunGenesisManifest` を原子的に書いた
`initialize_v2` が唯一の作成経路である。reader/recover は既存の各 component を
`O_NOFOLLOW` で開くだけで、欠落を作成してはならない。

ファイルシステム上の run directory は `run_id` をそのまま使わず、canonical
StableId text の SHA-256 full hex を component とする。これにより StableId が許す
`/` を含む payload も traversal にならない。run identity は path 名ではなく、V2
manifest と全 event の `run_id` により検証する。初期 log は `O_TMPFILE` に書き
`sync_all` 後、create-only `linkat(AT_EMPTY_PATH)` と run-dir `fsync` で公開する。

```json
{
  "schema": "reviewgraphen.review_event.v1",
  "id": "event:...",
  "run_id": "run:...",
  "genesis_hash": "sha256:...",
  "sequence": 1024,
  "actor": "reviewgraphen-core@1",
  "logical_time": 1024,
  "payload": {"type": "claim_proposed", "data": {}},
  "payload_hash": "sha256:...",
  "previous_event_hash": "sha256:...",
  "event_hash": "sha256:..."
}
```

M1では`genesis_hash`はsnapshot、universe、seeded ProgramSpace evidenceを含むpristine aggregateのcanonical hashです。空streamでも`run_id`とgenesisは必須です。最初の`previous_event_hash`は`run_id`と`genesis_hash`を入力にした`reviewgraphen.event_chain_genesis.v1` sentinelであり、以後は直前eventの`event_hash`です。replay/resumeはsequenceだけでなくこのchainを検証するため、同一sequenceのfork/spliceを受け入れません。

hash chainは改ざん防止の補助であり、署名やtrusted timestampを自動的に意味しません。`logical_time`はM1で決定的replayに使うsequence同値の論理時刻で、wall clockや認証時刻の代替ではありません。authority-bearing evidence、evidence binding、verification、human decisionをJSONだけで再importすることはできず、hostが同一run/genesis/bodyへ発行したnonserializable admissionを必要とします。各admissionはmint時のchain tail hashと期待next sequenceにもbindされるため、同じrun/genesis/bodyでもforked streamや別positionへ再利用できません。decisionはこれに加えてacceptance closureとuniverseへbindされます。

## 4. Event streams

大規模repoでは一つの巨大logよりstreamを分けます。

- repository stream。
- snapshot stream。
- review run stream。
- decision stream。
- policy stream。

cross-stream referenceはstable IDで行い、global sequenceまたはtimestampだけに依存しません。

## 5. Checkpoint

event replayを高速化するためcheckpointを作れます。

checkpointは次を持ちます。

- covered event range。
- event tail hash。
- model/schema version。
- serialized Program/Review/Evidence state。
- validation result。

checkpointはevent logから再生成可能でなければなりません。

## 6. Artifact store

大きいpayloadをreport/eventへ直接埋めません。

対象:

- source excerpts。
- raw model response。
- test logs。
- analyzer output。
- trace。
- binary coverage。
- context envelope。
- rendered report。
- prompt。

content-addressed ID:

```text
artifact:sha256:<hash>
```

metadata:

- media type。
- size。
- encryption state。
- source。
- retention policy。
- sensitive classification。

## 7. Derived index

SQLite初期schema候補:

```text
objects
relations
obligations
executions
claims
evidence
bindings
verifications
decisions
obstructions
coverage
staleness
events
artifact_metadata
```

indexはquery最適化のためのprojectionです。databaseだけを書き換えてcanonical stateを変更しません。

### M1 report execution references

M1 aggregateはclaimが参照する`execution_id`だけを保持し、reviewer、provider/model、
context envelope、raw outputをcanonical stateとして保持しない。したがってreport adapterは
schema必須fieldに`unknown`/`unresolved` sentinelを使い、executionを`abstained`として
`abstention_reason`を出す。各placeholderにはprojection-loss limitationと、該当claim/
obligationを`blocks`へ列挙する`execution_metadata_unavailable` obstructionを添える。
これはexecutionが観測済みであるという主張でもM2 execution engineでもない。

## 8. Configuration

`config.toml` はworkflow設定であり、review結果の正本ではありません。

```toml
schema = "reviewgraphen.config.v1"
profile = "code-review@1"

[storage]
mode = "local"
event_dir = ".reviewgraphen/store/events"
artifact_dir = ".reviewgraphen/artifacts"
index = ".reviewgraphen/indexes/reviewgraphen.sqlite"

[security]
network = "provider-only"
repository_write = false
arbitrary_shell = false
redact_secrets = true
```

configuration変更はrun manifestへhashを保存します。

## 9. Profile and policy storage

profile/rule/policyはGit管理可能なtext fileにします。ただし、そのfileが実行時に解釈されたversion/hashをrunへ固定します。

- current file pathだけを参照しない。
- run後の編集で過去結果の意味を変えない。
- compiled/normalized profile artifactを保存する。
- policy exceptionはeventとして記録する。

## 10. Repository source isolation

source repositoryとReviewGraphen storeを論理分離します。

- repository codeはread-only mountを推奨。
- `.reviewgraphen`のみwrite可能。
- external working directoryへ出力しない。
- symlink escapeを拒否。
- artifact storeへsecret classificationを適用。
- source upload policyをprovider adapterごとに強制。

## 11. Locking and concurrency

必要lock:

- repository snapshot lock。
- event stream append lock。
- index rebuild lock。
- artifact write atomicity。
- run state transition lock。

parallel reviewerは同一event streamへ直接競合writeせず、execution resultをtemporary artifactへ書き、orchestratorがstable orderでcommitします。

## 12. Retention

分類別policy:

| Artifact | Default |
| --- | --- |
| event metadata | retain |
| report | retain |
| raw model output | configurable |
| source excerpt | configurable / sensitive |
| test log | bounded |
| runtime trace | bounded / sensitive |
| secret detection artifact | encrypted / minimal |
| provider prompt | policy-dependent |

削除してもeventから存在とhashが分かるtombstoneを残します。

## 13. Export and import

```bash
reviewgraphen store export \
  --run <run-id> \
  --redact source,identity \
  --output review-bundle.tar.zst
```

bundle:

- manifest。
- schemas。
- events。
- selected artifacts。
- checksums。
- redaction declaration。
- projection loss。

import時にhash、schema、ID collision、trust levelを検証します。

## 14. Encryption

MVPで独自暗号を実装しません。

- filesystem/OS encryptionを前提。
- hosted版ではmanaged KMS。
- artifact-level encryptionが必要なら標準AEAD library。
- keyをrepositoryへ保存しない。
- encrypted artifactもmetadata leakageを考慮。

## 15. Migration

store schema migration:

1. event schemaはappend-onlyで旧versionを保持。
2. new runtimeがold eventをupcastする。
3. derived indexは再構築可能。
4. irreversible migration前にexport。
5. migration reportをeventとして記録。
6. semantic changeはschema morphismとして扱う。

## 16. Disaster recovery

- event log backup。
- artifact checksum。
- index rebuild command。
- checkpoint validation。
- partial write recovery。
- run resume。
- corrupted artifact obstruction。

```bash
reviewgraphen store verify
reviewgraphen store rebuild-index
reviewgraphen run resume <run-id>
```

## 17. Storage invariants

1. report/projectionをcanonical stateにしない。
2. indexはevent logから再構築可能。
3. artifactはcontent hashで検証する。
4. runはprofile/policy/config hashesを固定する。
5. source repositoryへのwriteをdefault禁止。
6. event appendはatomic。
7. deletionはaudit tombstoneを残す。
8. migrationは意味変更を記録する。
