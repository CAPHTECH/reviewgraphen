# 06. ProgramSpace Ingestion

> Status: Draft v0.1  
> Principle: AST is the entry point, not the complete review model

## 1. 目的

ProgramSpace ingestionは、repositoryのすべてを「理解した」と宣言する工程ではありません。後続のReviewObligation生成に必要な**accepted structural factsと明示的なunknown**を作る工程です。

LLMにrepository探索を任せる前に、可能な限り決定論的な解析器で対象世界を列挙します。

## 2. 入力境界

一つのingestion runは固定されたsnapshotを対象にします。

```yaml
snapshot:
  repository_id: CAPHTECH/example
  base_revision: main
  target_revision: abc123
  worktree_state: clean
  submodules: pinned
  profile: code-review@1
```

次をsnapshot descriptorに含めます。

- repository identity。
- base/head commit。
- untracked/dirty state。
- submodule revision。
- generated/vendor exclusion policy。
- language adapter versions。
- analyzer versions。
- build configuration。
- feature flags。
- target platform。
- environment-dependent limitations。

## 3. 解析層

ProgramSpaceは段階的に構築します。高い層が取れない場合でも低い層を利用でき、coverage denominatorへ限界を反映します。

### Tier 0: Repository and change facts

- files。
- hashes。
- language。
- change type。
- additions/deletions。
- rename/copy。
- ownership。
- build manifest。
- test files。
- diff hunks。

### Tier 1: Syntax and symbols

- AST。
- module / namespace。
- class / type / function / method。
- declaration / reference。
- visibility。
- annotation / attribute。
- source range。
- containment。

### Tier 2: Semantic relations

- resolved import。
- direct call。
- type use。
- implementation / override。
- read / write。
- async / await。
- exception / result propagation。
- test-to-target relation。
- route-to-handler relation。
- serialization relation。
- ownership and boundary relation。

### Tier 3: Control and data flow

- CFG。
- def-use。
- source/sink。
- inter-procedural path。
- path condition。
- error flow。
- resource lifecycle。
- transaction boundary。
- state transition。

### Tier 4: Runtime evidence

- executed call path。
- coverage。
- trace。
- logs。
- performance profile。
- race detector result。
- integration environment observation。

## 4. ASTだけでは足りない理由

ASTは次を正確に表します。

- syntax。
- declaration。
- lexical nesting。
- explicit expression。
- direct construct。

一方、重大欠陥は次に現れます。

- indirect call。
- dynamic dispatch。
- dependency injection。
- state transitionの組み合わせ。
- requestからDBまでのpath。
- multiple event re-entry。
- configurationとcodeの相互作用。
- requirementとtestの不一致。
- local assumptionsの衝突。

したがって、AST nodeの網羅はProgramSpace completenessの一側面に過ぎません。

## 5. Accepted factとInference

### Accepted fact候補

- compiler/parserが抽出したdeclaration。
- LSPがresolveしたsymbol reference。
- gitが報告したchange。
- test runnerのexit result。
- toolが生成したCFG edge。
- repositoryで宣言されたownership。
- versioned policy file。

acceptedはsource adapterの観測として利用可能という意味です。

### Inference

- feature membershipの推定。
- likely call edge。
- business role。
- probable invariant。
- risk category。
- missing test hypothesis。
- semantic equivalence。

InferenceはProgramSpaceへ入れる場合も`unreviewed`またはcandidate provenanceを持ち、accepted relationと同じqueryで暗黙に混ざらないようにします。

## 6. Provenance

すべてのfactは次を持ちます。

```yaml
provenance:
  source_kind: compiler | lsp | parser | git | test | policy | ai
  source_id: ...
  tool:
    name: rust-analyzer
    version: ...
  extraction_method: symbol-reference-v1
  confidence: 1.0
  review_status: accepted
  snapshot_id: ...
```

confidence 1.0でも意味的に完全とは限りません。confidenceはそのfact抽出の確からしさであり、repository modelの完全性ではありません。

## 7. Completeness Declaration

extractorは「取れたfact」だけでなく「取れなかった領域」を返します。

```yaml
completeness:
  files:
    discovered: 421
    parsed: 417
    excluded: 2
    failed: 2
  symbols:
    resolution: partial
  calls:
    direct_static: complete_for_parsed_files
    dynamic_dispatch: partial
    reflection: unsupported
  data_flow:
    intra_procedural: available
    inter_procedural: unavailable
  runtime:
    trace_coverage: 0.37
```

この宣言はobligation generationとcoverage gateに影響します。

## 8. Exclusion

除外は黙って行いません。

```yaml
exclusion:
  pattern: vendor/**
  reason: vendored_dependency
  policy_id: policy:code-review:vendor
  affects:
    - node_coverage
    - path_coverage
  review_status: accepted
```

generated code、migration、config、docsは欠陥源になり得るため、単純に無視しません。profileで対象propertyとriskに応じて扱います。

## 9. Language-neutral schema

ProgramSpaceのcore vocabularyは言語固有構文を直接持ちません。

```text
program.file
program.module
program.symbol
program.type
program.callable
program.state
program.test
program.config
program.resource
```

language-specific detailはextension payloadへ置きます。

```json
{
  "kind": "program.callable",
  "language": "rust",
  "language_payload": {
    "async": true,
    "unsafe": false,
    "trait_impl": "PaymentGateway"
  }
}
```

ruleはgeneric propertyとlanguage capabilityの双方を宣言します。

## 10. Rust reference adapter

MVPのreference adapterはHigherGraphenのdogfoodingを優先し、Rustを対象とします。

候補input:

- `git diff --name-status` 相当のprovider-neutral adapter。
- `cargo metadata`。
- `syn`によるAST。
- rust-analyzerまたはSCIP由来のsymbol relation。
- `cargo test --message-format json`。
- coverageが利用可能な場合のregion mapping。
- Clippy / compiler diagnostics。

MVPでprecise inter-procedural data flowを自作しません。Tier 3は外部analyzer adapterまたはbounded fixtureから始めます。

## 11. Adapter capability descriptor

```yaml
adapter:
  id: rust-syn@1
  languages: [rust]
  produces:
    - file
    - symbol
    - containment
    - attributes
  does_not_produce:
    - resolved_dynamic_calls
    - interprocedural_data_flow
  deterministic: true
  network_required: false
```

ruleは必要capabilityを宣言します。

```yaml
rule:
  id: async.concurrent_reentry
  requires:
    any_of:
      - state_transition
      - event_to_handler
    optional:
      - runtime_trace
```

capability不足の場合、ruleを黙ってskipせず、not-generated reasonまたはcoverage limitationを残します。

## 12. ProgramSpace lift

ExtractionBundleからHigherGraphen structureへ変換します。

```text
file fact          -> 0-cell
symbol fact        -> 0-cell
call relation      -> 1-cell or incidence
state transition   -> 1-cell
path               -> Complex
feature context    -> Context
snapshot           -> Space metadata
fact provenance    -> Provenance
```

lift adapterはstable ID、deduplication、validationを担当します。

## 13. Input normalization

- path separatorを正規化する。
- symlinkをworkspace policyに従って解決する。
- line ending差をcontent hashから分離するか明示する。
- renameをdelete/addへ潰さない。
- generated file markerを保持する。
- source rangesを1-based/0-basedで混在させない。
- Unicode identifierをnormalized formだけで同一視しない。
- build profile差をsnapshotに含める。

## 14. Failure handling

| Failure | Handling |
| --- | --- |
| 一部file parse失敗 | partial ProgramSpaceを作り、extraction obstructionを記録。 |
| symbol resolver unavailable | syntax factsは受理し、semantic completenessを低下。 |
| test command失敗 | test evidenceとしてfailureを記録し、ingestion自体と分離。 |
| unsupported language | file factsを保持し、profile capability obstructionを作る。 |
| tool version mismatch | snapshot/toolchain mismatchとしてreport。 |
| malicious path | workspace boundaryで拒否。 |

## 15. Ingestion invariants

1. accepted factはsource adapterとsnapshotへ追跡できる。
2. unresolved referenceを存在しないrelationとして扱わない。
3. unsupportedとabsentを区別する。
4. excluded regionをcoverage denominatorから暗黙に消さない。
5. inferenceをaccepted extraction factへ昇格しない。
6. same source/tool versionからstable fact IDsを生成する。
7. ProgramSpace作成後にsource fileが変わった場合、snapshot mismatchを検出する。
