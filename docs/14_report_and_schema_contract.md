# 14. Report and Schema Contract

> Status: Draft v0.1  
> Current implemented schema: `reviewgraphen.review.report.v1`<br>
> Accepted D2 design schema: `reviewgraphen.review.report.v2` (ADR 0018)

`reviewgraphen.review.report.v1` remains the implemented M1 report and keeps
its unresolved execution projection. [ADR 0018](adr/0018-d2-execution-claim-report-and-index-v3.md)
defines the additive D2 report contract in
[`reviewgraphen.report.v2.schema.json`](../schemas/reviewgraphen.report.v2.schema.json).
V1 is not silently repointed or filled with guessed D2 metadata; a v2 report
must be regenerated from confirmed v2 event/CAS data.

The checked-in [v2 report example](../schemas/reviewgraphen.report.v2.example.json)
is local-only. It proves shape and report-local self-consistency, not the tail,
CAS bytes, index, denominator, plan/envelope closure, or lifecycle. No
v2 source-bound fixture is checked in until it can be generated from actual core
canonical output and rebuilt through the actual v3 index implementation; the
bundle validator never reconstructs or fabricates those sources from report
assertions.

M4 adds the closed, source-bound
[`reviewgraphen.review.report.v3`](../schemas/reviewgraphen.report.v3.schema.json)
contract. V3 is generated only from a confirmed event-v3 journal, its verified
index-v4 projection, and authority-root/CAS checks. Its coverage keeps selected,
visited, completed, evidence-supported, verified, fresh-verified, and accepted
as separate explicit ID sets; an accepted finding additionally requires the
current finding, active human `accept` decision, and complete passed evidence
trace. The checked-in v3 example is emitted by the public source-bound test
fixture after an actual event-v3 replay, fixed-witness verification, explicit
human acceptance, current-finding projection, completed lifecycle transition,
and index-v4 rebuild. Its integration test requires byte equality (apart from
the text file's terminal newline), confirmed-tail/CAS closure, deterministic
repeat and rebuild output, and exact construction-accounting bounds.

M5 adds the additive, source-bound
[`reviewgraphen.review.report.v4`](../schemas/reviewgraphen.report.v4.schema.json)
contract without changing the frozen v1-v3 contracts. V4 retains the complete
v3 report and coverage shape, binds generation to a completed double-submit
M5 bundle and a verified index-v5 snapshot, and adds the two nested input
registrations/descriptors plus context-cover, section, restriction, gluing
attempt, global-candidate, and gluing-obstruction arrays. Each projection view
declares one recoverable loss for every nonempty M5 array it omits; the checked
example and SHA-256 are emitted by the public journal/CAS/index/runtime/report
pipeline and checked byte-for-byte against a repeated generation.

## 1. Report-first principle

ReviewGraphenはCLI表示より先にstable report contractを定義します。

理由:

- agentが構造を直接読める。
- CLI、MCP、UI、CIが同じsourceを利用できる。
- fixtureとschemaで回帰検証できる。
- human reportをcanonical stateにしなくて済む。
- accepted fact、claim、evidence、decisionの境界を強制できる。

## 2. Report envelope

```json
{
  "schema": "reviewgraphen.review.report.v1",
  "report_type": "review",
  "report_version": 1,
  "metadata": {},
  "scenario": {},
  "result": {},
  "coverage": {},
  "projection": {}
}
```

### metadata

- report ID。
- run ID。
- generated timestamp。
- tool versions。
- profile/rule versions。
- model/provider descriptors。
- policy version。

### scenario

- repository/snapshot。
- ProgramSpace reference。
- ReviewSpace reference。
- EvidenceSpace reference。
- extraction completeness。
- change morphism。
- budget。

### result

- status。
- obligations。
- executions。
- claims。
- evidence bindings。
- verifications。
- decisions。
- findings。
- obstructions。
- completion candidates。
- gluing results。
- stale records。

### coverage

- universe descriptor。
- dimensions。
- raw/weighted counts。
- limitations。
- freshness。

### projection

- human review。
- AI view。
- audit trace。
- CI gate。

## 3. Status

report status:

```text
completed
partial
unsupported_input
failed
```

gate statusとは別です。

```text
pass
blocked
incomplete
```

`completed` reportが`blocked`または`incomplete`であることは正常です。

## 4. Snapshot reference

```json
{
  "repository_id": "CAPHTECH/example",
  "base_revision": "main",
  "target_revision": "abc123",
  "tree_hash": "sha256:...",
  "dirty": false,
  "profile_id": "code-review@1"
}
```

## 5. Universe descriptor

```json
{
  "id": "universe:...",
  "snapshot_id": "snapshot:...",
  "profile_id": "code-review@1",
  "rule_set_hash": "sha256:...",
  "extractor_set_hash": "sha256:...",
  "policy_version": "default@1",
  "obligation_ids": ["obligation:..."],
  "limitations": [
    {
      "kind": "capability_missing",
      "capability": "interprocedural_data_flow"
    }
  ]
}
```

## 6. Obligation record

必須:

- ID。
- target。
- property。
- context requirement。
- evidence requirement。
- risk descriptor。
- applicability。
- provenance。
- lifecycle。
- source IDs。

severityとconfidenceを混同しません。obligationにmodel confidenceは通常不要です。AI-generated candidateだけcandidate confidenceを持ちます。

## 7. Claim record

```json
{
  "id": "claim:...",
  "execution_id": "execution:...",
  "obligation_ids": ["obligation:..."],
  "polarity": "issue_present",
  "property_id": "payment.at_most_once",
  "target_refs": ["path:submit-to-charge"],
  "statement": "...",
  "source_ids": ["symbol:...", "relation:..."],
  "assumptions": ["..."],
  "confidence": 0.82,
  "disposition": "proposed",
  "review_status": "unreviewed"
}
```

## 8. Evidence and verification

Evidenceをclaim recordへ埋め込みすぎず、ID参照にします。

```json
{
  "evidence_bindings": [
    {
      "claim_id": "claim:...",
      "evidence_id": "evidence:...",
      "relation": "supports",
      "scope": {
        "property_id": "payment.at_most_once"
      }
    }
  ],
  "verifications": [
    {
      "claim_id": "claim:...",
      "result": "passed",
      "verifier_id": "duplicate-submit-test@1",
      "evidence_ids": ["evidence:..."],
      "limitations": []
    }
  ]
}
```

## 9. Finding projection

Findingはclaim/evidence/decisionへの参照を持つprojectionです。

```json
{
  "id": "finding:...",
  "claim_id": "claim:...",
  "severity": "critical",
  "title": "Duplicate submission can charge twice",
  "status": "accepted",
  "evidence_ids": ["evidence:..."],
  "verification_ids": ["verification:..."],
  "decision_id": "decision:...",
  "location_refs": ["symbol:..."],
  "remediation_candidate_ids": ["candidate:..."]
}
```

verificationなしのcandidateをFindingとして人間viewへ出す場合、`unverified candidate`を明示します。

## 10. Obstruction

```json
{
  "id": "obstruction:...",
  "kind": "gluing_conflict",
  "severity": "high",
  "title": "...",
  "source_ids": ["context:...", "section:..."],
  "counterexample_refs": [],
  "required_resolution": [
    "Resolve nullability contract on overlap"
  ],
  "blocks": [
    "gate:default",
    "global-claim:profile-flow-safe"
  ],
  "review_status": "unreviewed"
}
```

review processの障害とartifact findingを区別します。

## 11. Coverage schema

```json
{
  "universe_id": "universe:...",
  "extraction": {
    "files_parsed": {"numerator": 417, "denominator": 421},
    "call_resolution": {"value": 0.71, "qualification": "static direct calls"}
  },
  "stages": {
    "visited": {"raw": 420, "total": 500, "weighted": 0.91},
    "completed": {"raw": 398, "total": 500, "weighted": 0.88},
    "evidence_supported": {"raw": 250, "total": 500, "weighted": 0.79},
    "verified": {"raw": 180, "total": 500, "weighted": 0.72},
    "fresh_verified": {"raw": 174, "total": 500, "weighted": 0.69}
  }
}
```

percentageだけを保存せずnumerator/denominatorを持ちます。

## 12. ProjectionViewSet

### human_review

- summary。
- gate status。
- critical/high findings。
- unresolved obligations。
- evidence limitations。
- decisions required。
- suggested next actions。

### ai_view

- stable IDs。
- object states。
- source trace。
- dependencies。
- capability limitations。
- requested next operations。

### audit_trace

- all source IDs。
- tool/model versions。
- prompt/projection hashes。
- event range。
- accepted/rejected decisions。
- information loss。
- schema migrations。

### ci_gate

- status。
- policy。
- blockers。
- incomplete reasons。
- threshold evaluation。

## 13. Information loss

各projectionは少なくとも次を宣言します。

```json
{
  "information_loss": [
    {
      "kind": "omitted_low_risk_obligations",
      "source_count": 240,
      "projected_count": 0,
      "reason": "human summary",
      "recoverable_via": "ai_view"
    }
  ]
}
```

loss宣言は「詳細は省略した」という定型文だけでは不十分です。source count、kind、reason、recoverabilityを持たせます。

## 14. Schema versioning

- schema IDへmajor versionを含める。
- additive optional fieldは同majorで可能。
- enum追加はconsumerによってbreakingになり得るためcompatibility noteを持つ。
- field semantics変更はmajorを上げる。
- migration toolを用意する。
- old fixtureを保持する。
- report producer/consumer compatibility matrixを公開する。

## 15. Validation boundary

deserialization時に次を検証します。

- ID format。
- required source refs。
- confidence range。
- state transition。
- accepted claimにdecisionがあるか。
- verified resultにverifier/evidenceがあるか。
- coverage numerator <= denominator。
- stale recordがfresh coverageへ含まれていないか。
- projection source IDsとloss。
- schema/profile version。
- duplicate IDs。
- dangling references。

JSON Schemaだけで表現しにくいcross-record invariantはruntime validatorで確認します。

## 16. Normalization

- string trimming。
- set-like ID list dedup。
- stable ordering。
- canonical JSON for hashing。
- timestamp UTC。
- path normalization。
- enum lowercase snake_case。
- decimal scoreの範囲。
- source fragmentをreportへ直接大量埋め込みせずartifact ref化。

## 17. Research export

研究評価向けに、production reportからPIIやsource codeを除いたprojectionを持ちます。

含む:

- experiment condition。
- model/config。
- obligation counts。
- finding labels。
- metrics。
- token/cost。
- runtime。
- anonymized project ID。
- benchmark ground-truth mapping。

含まない:

- secret。
- raw proprietary source。
- personal identity。
- unrestricted model output。

## 18. Contract invariants

1. report statusとgate statusを分ける。
2. accepted Program factとAI claimを同じcollectionにしない。
3. Findingはclaim/evidence/decisionへ追跡できる。
4. verified recordはverifierを持つ。
5. coverageはuniverseとraw denominatorを持つ。
6. projectionはsource IDsとlossを持つ。
7. stale recordはfresh coverageへ入らない。
8. schema migrationをsilentに行わない。
