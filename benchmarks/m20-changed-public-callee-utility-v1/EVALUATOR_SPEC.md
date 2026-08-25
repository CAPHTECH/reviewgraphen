# m20 executable evaluator specification

Status: single-pipeline implementation specification, version
`m20-evaluator.pipeline.v1`.

This document is normative only until the evaluator bundle described in
section 12 is implemented and frozen. After freeze, the hash-bound evaluator,
schemas, reference vectors, and generated fixtures are normative for packet
construction and scoring. Study question, corpus, unit, stages, rectangles,
authority ceiling, and interpretation limits remain normative in the four
pre-existing m20 documents.

The implementation MUST live under
`benchmarks/m20-changed-public-callee-utility-v1/evaluator/`, MUST use Python 3,
and MUST NOT modify `crates/` or ADR 0038. The evaluator author MUST be distinct
from the Candidate D slice implementer. No hand-written scored fixture is
permitted.

## 1. Single-pipeline surface and one trust boundary

### 1.1 Production topology

There is one production entry point:

`RUN(frozen_launch, model_transport, new_output_root) -> RunResult`.

It executes one paired commit unit end to end in one process:

```text
verify freeze/runtime and authenticated launch selectors
  -> read pinned base/head repository objects
  -> validate the atomic Stage-0 occurrence closure and v3 frozen obligation
  -> derive both source plans
  -> EXTRACT every source and derive losses/opportunity
  -> construct both packets and hidden bindings
  -> invoke reviewer for slot 0, parse raw bytes, score in memory
  -> invoke reviewer for slot 1, parse raw bytes, score in memory
  -> construct sealed opaque permutation and one two-candidate judge batch
  -> invoke judge, parse raw bytes, reconcile and compute both primary cells
  -> finalize artifact ledger and run seal
```

Packet, inventory, payload, loss, opportunity, hidden binding, mechanical
record, candidate, batch, reverse map, judge result, and primary record are
module-private immutable values. They are never accepted by a production CLI
or reconstructed from files written by this run. The process may serialize a
value for a model request or audit artifact, but continues with the original
in-memory value. Serialization is one-way on the production path.

The authenticated `m20.pipeline_launch.v1` is a frozen study selector, not a
scoring DTO. It contains exactly `schema`, `experiment_id`, `unit_id`,
`repository_root`, `base_commit_oid`, `head_commit_oid`,
`frozen_obligation_path`, `frozen_obligation_sha256`, `stage_manifest_path`,
`stage_manifest_sha256`, `context_policy_id`, and `context_policy_sha256`.
The last two fields are exactly `context.subject_windows@3` and
`sha256:932bfa18c5d286c63196366d6d2dc1aaf402f50baa1f1ab5f075b1007be55dd8`.
It contains no source bytes/status, packet, loss,
eligibility, binding, candidate, batch, score, seed, prompt, or hash for an
intermediate value. The stage manifest is sealed before launch and binds the
profile/context identities, public seeds, backend adapters, repository allow
list, commit pair, unit, and obligation hash. A mismatch is a pre-launch
refusal, not an arm result.

The pipeline derives reviewer slot order from the frozen arm-order seed:
`low_bit(SHA256(UTF8(seed) || 0x00 || UTF8(unit_id)))`; 0 maps slot 0 to A and
1 maps slot 0 to B. The two-element map is private state and a sealed audit
record; no launch field selects order. Reviewer invocation is serial.

The reviewer transport accepts only model `Qwen3.8-27B-MLX-4bit` and MUST omit
the `reasoning_effort` field so the mlx-dspark server default `low` applies. An
explicit effort override, an 8-bit or `ornith-*` model, or `xhigh` is a
prelaunch refusal. `MODEL_PIN_RATIONALE.md` is the sole evidence narrative for
this choice and is protocol-frozen separately from runtime results. Both arms
request the frozen common `max_output_tokens=12000` and use a 900-second hard
timeout. This remains a backend request setting, not an evaluator-recomputed
token ceiling.

### 1.2 Exact trust boundary

Exactly one class of information crosses from a non-authority into scoring:
model invocation results. It occurs three times per unit: two reviewer results
and one judge result. Each result contains raw response bytes plus backend
process/timeout/truncation/usage metadata. Only the raw bytes are parsed into a
semantic value; no backend- or caller-supplied parsed object, parsed hash,
packet hash, loss, candidate, or score is accepted.

The following do not cross that boundary:

- evaluator code/data verified by the G6 freeze contract;
- the protocol-authenticated launch/stage manifests and frozen obligation;
- repository objects addressed by the pinned commits and verified Git object
  IDs;
- deterministic clock-free internal values derived from those authorities;
- artifacts previously written by the same process.

Malformed repository objects, missing blobs, filesystem errors, and invalid
frozen spans are typed preflight failures or extraction obstructions. They do
not gain authority by being called trusted inputs. The reviewer and judge have
no tools, repository mount, output-artifact path, or channel for feeding an
intermediate artifact back to the pipeline.

### 1.3 Modules and size budget

Modules marked pure perform no file, clock, process, environment, random, or
network access. The three independently CONFORMING modules MUST be carried
forward without algorithm changes: `canonical.py`, `source_payload.py`, and
`textnorm.py`.

| Module | Responsibility |
| --- | --- |
| `canonical.py` | unchanged restricted canonical JSON, hashes, and StableIds |
| `source_payload.py` | unchanged `EXTRACT` and payload/source identity helpers |
| `textnorm.py` | unchanged `textnorm.v1` |
| `repository.py` | read-only, fixed-argv Git object adapter; verifies tree/blob IDs and returns normalized relative paths and bytes |
| `model_boundary.py` | total depth-bounded decode of reviewer/judge raw bytes into closed immutable output types |
| `stage0_contract.py` | pure atomic constructor/validator for occurrence summaries and `context.subject_windows@3` commitments |
| `pipeline.py` | private state types and the sole end-to-end constructor/scorer; owns loss, packet, permutation, judge, and primary logic |
| `artifacts.py` | one-way atomic artifact sink plus separate read-only `verify_run`; production never calls the reader |
| `freeze.py` | G6 portable bundle/execution/provenance contract plus complete acceptance gate |
| `cli.py` | only `run`, offline verification/vector/attack commands, and freeze dispatch |

`__main__.py` only dispatches. All other imports are side-effect free. Python
dependencies are standard-library only. `repository.py` may invoke only a
resolved `git` executable with `shell=false`, a workspace-scoped cwd, an empty
Git config environment, and the fixed read-only subcommands
`rev-parse --show-object-format` and `cat-file --batch`. It reads raw commit,
tree, and blob contents, recursively parses tree objects itself, and recomputes
`HASH(type + SPACE + decimal_length + NUL + content)` under the declared SHA-1
or SHA-256 format for every consumed object before use. Any malformed batch
framing, type, mode, OID, tree edge, duplicate path, or hash mismatch is a
typed preflight failure. Git version/executable hashes are run provenance,
never scoring authority.

The normative tree has 10 responsibility modules, 5 closed
schemas instead of 16, and no public intermediate-command schemas:

```text
evaluator/
  __init__.py  __main__.py  canonical.py  source_payload.py  textnorm.py
  repository.py  model_boundary.py  stage0_contract.py  pipeline.py
  artifacts.py  freeze.py
  cli.py
  data/
    common_instruction.txt  question_registry.v1.json
    utility_rubric.v1.json  failure_codes.v1.json
    fixture_templates.v1.json
  schemas/
    pipeline_launch.v1.json  reviewer_output.v1.json
    judge_batch_output.v1.json  run_manifest.v1.json
    freeze_manifest.v1.json
  reference_vectors/
    text.v1.json  source.v1.json  loss.v1.json  judge.v1.json
    occurrence.v1.json  context.v3.json
  generated/
    reference_vectors.generated.json  fixtures.generated.json
    attacks.generated.json  inventory.generated.json
  tests/
    test_reference_vectors.py  test_stage0_contract.py  test_pipeline.py
    test_model_boundary.py
    test_artifact_audit.py  test_attacks.py  test_freeze.py  test_cli.py
```

Code size and test count are observations, never acceptance metrics.
Acceptance depends on the named behavior-changing attacks in section 11 and
the literal vector outcomes, not a displayed aggregate pass number.

### 1.4 Atomic Stage-0 re-freeze contract

This version implements ADR 0038 sections 3.4 and 5.4 in one freeze. No
occurrence-only intermediate manifest is valid. `stage0_contract.py` is the
only constructor for both amended records. It receives the full internal typed
occurrence set or the full rebuilt context sets, independently derives the
public values, and byte-compares the proposed public value. A check that only
compares caller-supplied counts and digests is forbidden.

For occurrence enumeration, the old public array
`located_call_occurrences` is rejected. The constructor emits exactly one
`reviewgraphen.ingestion_obstruction_summary.v1` per accepted Rust file with
one or more occurrences, plus report-level `observed_occurrence_count` and
`occurrence_id_set_sha256`. Each summary has exactly the ADR 3.4 fields,
`detail_retention=spans_and_owner_sources_omitted_rebuildable`, one to eight
nonempty strictly ordered kind/reason buckets, a checked count, and the digest
of the sorted exact internal occurrence-ID set. File counts equal both the
bucket sum and set cardinality. Report count/digest close over every file; the
global limitation remains a singular source-backed unknown-latent record and
never contributes to the observed count.

Per-rule coverage contains exactly
`enumeration_obstruction_summary_ids`, `enumeration_limitation_ids`, their
sorted union `enumeration_obstruction_ids`,
`observed_unresolved_call_occurrence_count`, and
`occurrence_id_set_sha256`. It is rebuilt from the report. Canonical Stage-0
closure may retain only deterministic report bytes, summary-row count, and
observed occurrence count. Wall time, process CPU time, peak bytes, worker
count/utilization, and scheduling order are noncanonical operational
diagnostics outside the Stage-0 output root and every artifact/hash preimage.
The measured 183.2 seconds per cluster (about 15.3 serial hours for 300
clusters) and the rejected 96,329,400-row projection are pre-seal operational
observations, not changed gates or denominators.

The v2 context tuple remains an immutable compatibility literal:

`context.subject_windows@2` / `sha256:7c4ceca165588cd38b28cc6882bb68a1ff4040dbb19ee34dfd792216d70bbf26`.

It is never decoded as active v3. The active tuple is
`context.subject_windows@3` /
`sha256:932bfa18c5d286c63196366d6d2dc1aaf402f50baa1f1ab5f075b1007be55dd8`.
`CONTEXT_V3_BYTES` is the exact no-BOM/no-LF policy DTO in ADR 0038 section
5.4; canonicalization and SHA-256 must reproduce that literal hash.

V3 rebuilds four separately named exact sets and serializes each only as
`{cardinality:"known",observed_count,sorted_id_set_sha256}`: all accepted
file IDs, reached file IDs, the union of valid subject-file IDs and reached
file IDs, and exact range-bearing support-anchor IDs. The materialized union,
not the accepted-file denominator, is bounded at 4,096 before source-row
construction. A support-anchor ID is `D("context-support-anchor",
{anchor_contract:"context.support_anchor@1",end_line,owner_artifact_id,
snapshot_id,source_artifact_id,start_line})`.

There are exactly two ordered subject outcomes, callee then caller. Each is
either admitted with endpoint/file/range/window identity or a named high-
severity loss with recovery/source trace. Admitted windows retain sorted
anchor IDs. The remaining known anchors are partitioned into at most 15
nonempty reason summaries in policy precedence order; admitted and summarized
sets are pairwise disjoint and their union is the exact anchor denominator.
`latent_cardinality` is either closed `known_zero` or `unknown` with nonempty
capability states and qualification IDs. Unknown has no numeric value.

The v3 projection hash binds the exact policy tuple, snapshot/request,
obligation/relation/endpoints, all four commitments, latent union, two subject
outcomes, materialized sources/windows, loss summaries, and remaining
unknown/loss/source identities. Treatment sources must close exactly to the
admitted-window `source_required_ids`. Launch, stage manifest, frozen
obligation, run manifest, and hidden judge binding all repeat the active tuple
and projection hash; omission, v2/v3 mixing, or cross-decode is a prelaunch
refusal. The baseline source algorithm and `review-task` ID preimage are
unchanged.

## 2. Restricted canonical JSON and error ordering

All evaluator JSON is I-JSON restricted further as follows: object keys are
ASCII; duplicate keys are rejected during parsing; values are objects, arrays,
strings, booleans, null, or integers in `[-(2^53-1), 2^53-1]`; floats are
forbidden. Strings containing an unpaired surrogate or U+0000 are rejected.

`JCS(v)` means UTF-8 JSON with object keys sorted by their ASCII byte sequence,
no insignificant whitespace, lowercase `true`/`false`/`null`, unescaped UTF-8
except required JSON escaping, and array order preserved. Because all keys are
ASCII and floats are forbidden, this is an unambiguous RFC 8785 subset.

`H(v) = "sha256:" + lowercase_hex(SHA256(JCS(v)))`.
`D(kind,v) = kind + ":" + H(v)`. Thus a task ID is
`review-task:sha256:<64hex>`.

Validation errors are closed records `{code, json_pointer, detail_id}` sorted
by `(json_pointer UTF-8 bytes, code UTF-8 bytes, detail_id UTF-8 bytes)`.
Human exception text is never scorer input.

### 2.1 Raw model-output decoder

Restricted canonical JSON above remains byte-for-byte the only JSON parser.
`model_boundary.py` receives raw bytes directly from `model_transport`; it does
not accept a Python object or a separate parsed hash. Before parsing it rejects
more than 1,048,576 bytes and, using an iterative string/escape-aware scan,
rejects JSON nesting deeper than 32. Every parser exception, recursion error,
Unicode error, and schema error becomes a closed validation error; none escapes
the pipeline.

The decode operation is exactly:

```text
DECODE_MODEL(raw_bytes, expected_kind, expected_ids):
  raw_sha256 = SHA256(raw_bytes)
  enforce byte and nesting limits
  value = restricted_parse_json_bytes(raw_bytes)
  if expected_kind == reviewer:
      decode every nested field as section 4 ReviewerOutput
      require task_id and source_inventory_id equal expected_ids
  else:
      decode every nested field as section 9 JudgeBatchOutput
      require batch_id, record order, and candidate IDs equal expected_ids
  reject every unknown field and every bool where an integer is required
  parsed_sha256 = H(value)
  return immutable typed value plus raw_sha256 and parsed_sha256
```

Reviewer output and judge output are the only semantic DTO decoders on the
production path. All former pair-build, mechanical, judge-build/reconcile, and
primary request DTOs are deleted. Offline `verify-run` has its own hostile-
artifact decoder, but production never calls it.

### 2.2 Frozen failure-code order

`failure_codes.v1.json` contains exactly the following order; output arrays are
the applicable collected codes in this order, without duplicates:

1. `timeout`
2. `process_exit`
3. `empty_response`
4. `malformed_json`
5. `schema_invalid`
6. `task_id_mismatch`
7. `packet_hash_mismatch`
8. `inventory_hash_mismatch`
9. `payload_missing`
10. `source_blob_base64_invalid`
11. `payload_hash_mismatch`
12. `payload_length_mismatch`
13. `source_payload_closure_invalid`
14. `reviewer_lens_leak`
15. `hidden_binding_mismatch`
16. `pair_opportunity_mismatch`
17. `foreign_source_id`
18. `observation_range_invalid`
19. `changed_source_observation_missing`
20. `foreign_loss_id`
21. `routine_loss_only`
22. `mixed_loss_class`
23. `asymmetric_abstention_opportunity`
24. `loss_question_mismatch`
25. `inconclusive_disposition`
26. `invalid_text_codepoint`
27. `non_substantive_text`
28. `policy_violation`
29. `tool_violation`
30. `client_truncation`
31. `provider_truncation`
32. `missing_raw_hash`
33. `missing_parsed_hash`
34. `judge_batch_unavailable`
35. `judge_batch_invalid`
36. `judge_candidate_mismatch`
37. `utility_threshold_failed`
38. `primary_conjunction_invalid`

DTO decoding errors before a score record exists use the separate ordered
validation-error type from section 2.1. Codes for packet/hash/binding closure
are internal-invariant or offline-audit failures; they are never caller fields
that production tries to authenticate. Downstream checks whose prerequisites
did not decode are inapplicable, not inferred failures.

## 3. Reviewer-visible packet and exact source body

### 3.1 Packet DTO

Only `pipeline.py` constructs `arm-neutral.source-grounded-packet@2`; there is
no function accepting a packet. Its audit/model serialization is a closed
object with exactly:

- `schema`, constant `arm-neutral.source-grounded-packet@2`;
- `task_id`, matching `^review-task:sha256:[0-9a-f]{64}$`;
- `instruction`, byte-equal to the frozen common instruction template;
- `response_schema`, the complete closed
  `arm-neutral.source-grounded-disposition@1` schema object;
- `source_inventory`, section 3.2;
- `payloads`, section 3.3.

A and B have byte-identical `task_id`, `instruction`, and `response_schema`.
Only `source_inventory` and `payloads` may differ. Packet hash is `H(packet)`.
`admitted_source_bytes` is the integer sum of `source.bytes` over source
records, even when two records deduplicate to one payload; it is bounded by
65,536. `payload_table_utf8_bytes` is separately the sum of `UTF8(text)` byte
lengths over unique payload records. JSON escaping and structural overhead are
recorded as serialized packet bytes but are not part of the admitted-source
budget. There is no evaluator-enforced input-token ceiling or tokenizer
preflight.

`common_instruction.txt` contains exactly this UTF-8 line plus one LF:

> Inspect the admitted Rust source and return one source-grounded disposition using the supplied closed schema. Cite exact admitted locations. If the task cannot be decided, cite only a declared loss marked eligible for primary abstention and state the blocked question and evidence needed.

The packet JSON string is the file content with that single terminal LF
removed; no other trimming is permitted.

The common instruction, response schema, question registry, utility rubric,
failure order, and seeds are loaded from the verified frozen files/manifests at
pipeline start and are the values used at runtime. No duplicate Python literal
may substitute for a frozen data value.

### 3.2 Source inventory DTO

`repository.py` first validates that `head_commit_oid` has
`base_commit_oid` as its registered first parent, obtains tree entries using
the fixed adapter, rejects non-UTF-8, absolute, empty, `.`/`..`, NUL, or
non-profile paths, and verifies every object ID from its canonical Git object
preimage. Baseline spans are derived from the base/head production-tree diff
under the frozen profile by this total rule:

```text
PLAN_BASELINE(base_tree, head_tree):
  paths = UTF-8-byte-sorted union of profile-included Rust paths
  for each path whose blob ID differs:
    strict-UTF-8 decode both existing blobs and split with EXTRACT line rules
    run Python 3.13.5 difflib.SequenceMatcher(None, base_lines, head_lines,
                                             autojunk=False).get_opcodes()
    for every non-equal opcode, create a base span for each nonempty base range
      and a head span for each nonempty head range
    expand each span by exactly 3 existing lines on both sides
    merge overlapping or immediately adjacent spans on the same side/path
  emit spans sorted by (path UTF-8 bytes, base-before-head, start, end)
```

A non-UTF-8 changed production blob becomes an extraction obstruction rather
than being silently dropped. ReviewGraphen spans are the unique source windows
in the hash-verified frozen obligation. The obligation may name IDs, roles,
sides, tree-verified blob IDs, paths, and inclusive spans; it may not contain
bytes or an availability flag. Neither plan is truncated or padded. If either
complete packet exceeds 65,536 admitted-source bytes, the paired unit is
model-ineligible before either reviewer call.

Each planned span becomes an internal `SourceKey =
(role,snapshot_side,path,start_line,end_line,blob_oid)`. Planned keys and
required IDs MUST be unique. A duplicate/conflicting obligation is a pre-launch
failure; a duplicated baseline discovery is an internal invariant failure.
For each key the pipeline fetches the addressed blob and calls the unchanged
section 3.4 `EXTRACT` exactly once. `Available(payload)` and
`Obstruction(kind,evidence)` are the only constructors. No status record is
read from the launch, obligation, disk artifact, or caller.

The inventory is closed and contains `schema`, `source_inventory_id`, sorted
`admitted_sources`, sorted `declared_losses`, and `canonical_sha256`.

An admitted source is closed and contains:

- `source_id`;
- generic `role` in `changed | context | support`;
- generic `snapshot_side` in `base | head`;
- profile-normalized relative `path`;
- closed inclusive `range {start_line,end_line}` with positive integers;
- `payload_id`;
- positive `bytes`, equal to the `UTF8(text)` byte length;
- payload-byte `sha256`.

No reviewer-visible key or role may name rule, property, obligation, relation,
caller, callee, endpoint, subject, or the D selection mechanism.

`source_id = D("source", {role,snapshot_side,path,range,payload_id,bytes,sha256})`.
Sources sort by `source_id` and are unique by both `SourceKey` and `source_id`.
The path is exactly the normalized relative path returned by the pinned tree;
there is no display-path or caller-path input.

A loss is closed and contains `loss_id`, `reason`, `omitted_scope`, opaque
`recovery_reference`, `primary_abstention_eligible`, and nullable opaque
`undecidable_question_id`. Its derivation is defined only in section 6.
Losses sort by `loss_id`, are unique, and are disjoint from source IDs.

Let `inventory_body` be exactly `{schema:"m20.source-inventory.v3",
admitted_sources,declared_losses}`. Then
`source_inventory_id = D("source-inventory", inventory_body)` and
`canonical_sha256 = H(inventory_body)`. Neither ID/hash is included in its own
preimage.

### 3.3 Payload DTO and closure

A payload is closed and contains exactly:

- `payload_id = D("source-payload", {sha256,byte_length})`;
- `encoding`, constant `utf-8`;
- `media_type`, constant `text/x-rust; charset=utf-8`;
- `byte_length`, a positive integer;
- `sha256`, the SHA-256 of `UTF8(text)`;
- `text`, the strict-UTF-8 decoding of the exact excerpt bytes.

Payloads sort by `payload_id`. Equal `(sha256,byte_length)` payloads deduplicate
to one record. Every source references exactly one payload; every payload is
referenced; `UTF8(text)`, `byte_length`, source `bytes`, and both hashes must
agree. JSON escaping is transport syntax only: after parsing, the reviewer sees
the decoded Rust text directly and needs no base64 operation. A
content/hash/length/UTF-8/orphan/foreign mismatch is an internal constructor
failure. `ArtifactVerifier` applies the same closure independently to detect
post-run corruption; production never imports a purported payload table.

#### 3.3.1 Input-budget ruling and audit records

Option A is normative: equal input budget is enforced only as the same exact
`admitted_source_bytes <= 65_536` predicate for both arms. The sum is computed
from the two internal source maps before arm order or any model call. Packet
metadata, JSON escaping, instruction, and response-schema bytes are measured
and disclosed separately but do not consume that source-byte allowance.
Packets are never padded. Equal admitted-source bytes are not equal tokens,
equal information, equal serialized request bytes, or equal cost.

The former 16,384 input-token ceiling is deleted. No tokenizer, vocabulary,
chat template, special-token rule, or local token count is required or bundled.
Vendoring a tokenizer was rejected because it would enlarge and change the
G6-frozen bundle while still not proving equality with the backend's actual
model tokenizer. Deleting all token telemetry was rejected because
provider-reported usage remains useful descriptive audit evidence. Backend
tokenizer/usage data is therefore observation-only and has no effect on
eligibility, ordering, scoring, failure codes, or study decisions.
No fourth mechanism is adopted because the byte rule already supplies one
portable, independently recomputable enforcement predicate without a new
dependency or authority. The common maximum-output request is fixed at 12,000
for both arms. It remains a
backend request that is not independently tokenized or verified and is not
part of an equal-token-budget claim.

Before either reviewer call, write closed `budget.json` with exactly
`{schema,unit_id,ceiling_kind,ceiling_bytes,arms,pair_model_eligible,
budget_audit_id}`. `schema` is `m20.input_budget_audit.v1`; `unit_id` is
byte-equal to the authenticated launch field; `ceiling_kind` is
`admitted_source_utf8_bytes`; and `ceiling_bytes` is the integer 65536. `arms`
is an array of exactly two records in integer `slot` order 0, 1, each exactly
`{slot,packet_sha256,admitted_source_bytes,within_ceiling}`.
`packet_sha256=H(packet)`, `admitted_source_bytes` is the section 3.1 sum, and
`within_ceiling` is exactly `admitted_source_bytes <= ceiling_bytes`.
`pair_model_eligible` is the conjunction of those two booleans and
`budget_audit_id = D("input-budget-audit", object_without_id)`. `verify-run`
independently recomputes every field from the packet source records.

Each launched model execution record contains closed
`token_observation = {authority,source,tokenizer,input_tokens,output_tokens,
cache_tokens,counting_scope,unavailable_reason}`. `authority` is always
`observation_only`; `source` is `backend_report | unavailable`; tokenizer is
nullable closed `{name,revision,vocabulary_sha256,config_sha256}`; token counts
are nullable nonnegative integers; `counting_scope` is nullable backend prose;
and `unavailable_reason` is null exactly when source is `backend_report`. If
source is `unavailable`, tokenizer, every count, and counting scope are null and
the reason is exactly `backend_usage_absent | backend_usage_invalid`. If source
is `backend_report`, at least one tokenizer/count/scope field is non-null.
`token_observation` is a direct field of `execution.json`; its canonical bytes
are therefore included in that artifact's ledger hash preimage. `verify-run`
only validates its closed shape and explicitly emits
`token_observation_recomputable=false`. It never recomputes or compares it to a
ceiling.

If both byte predicates pass, normal reviewer execution begins. If either
fails, no reviewer or judge call occurs. `RUN` returns the closed result
`{schema:"m20.pipeline_result.v1",unit_id,pipeline_terminal_state:
"model_ineligible",model_ineligible_reason:
"admitted_source_byte_ceiling_exceeded",arm_results:{A:null,B:null},
model_call_count:0,run_seal_id}` where `unit_id` equals the launch and
`run_seal_id` equals the same field in `seal.json`. The required artifacts are the two packets,
`budget.json`, ledger, and seal defined in section 11. CLI exit is 0 because
this is a valid non-model terminal classification, not an arm failure. It never
enters `n`, `b`, `c`, or `n00` and cannot be sampled or replaced after
outcomes.

The exact fixture gives both arms source maps whose counted source bytes are
65,536 and requires `pair_model_eligible=true`. The `+1` fixture changes one
arm to 65,537 bytes without changing the other and requires the sealed
model-ineligible result above, zero transport invocations, and no primary
records. The fixture runs through the real pipeline and `verify-run`.

This closes H5-G1 because no authoritative tokenizer/counting procedure exists;
H5-G2 because deterministic byte budget and non-recomputable token observation
have separate closed audit records; and H5-G3 because byte overflow has one
typed sealed terminal meaning. A backend context-length rejection after an
eligible request is sent is an ordinary post-launch failure for that arm,
scored 0 without retry. A provider-reported token count, including any value
above 16,384, never changes that result.

### 3.4 Exact excerpt algorithm

Inputs are immutable file bytes, their pinned blob hash, normalized path, and
one inclusive `(start_line,end_line)` request. The total algorithm is:

```text
EXTRACT(file_bytes, start_line, end_line):
  if start_line < 1 or end_line < start_line:
      return obstruction(span_invalid)
  decode file_bytes as strict UTF-8; on failure return obstruction(non_utf8_source)
  line 1 begins at byte 0
  each byte 0x0A ends its line and is included in that line
  a final non-empty suffix without 0x0A is the final line
  an empty file has zero lines
  if start_line or end_line exceeds line_count:
      return obstruction(span_out_of_bounds)
  start = first byte of start_line
  end = byte after 0x0A ending end_line, or EOF when end_line lacks 0x0A
  excerpt = file_bytes[start:end]
  assert strict UTF-8 decode(excerpt) succeeds
  return payload(excerpt) and the original inclusive line range
```

CR before LF is retained. No newline normalization, trimming, redaction,
syntax parsing, or re-encoding occurs. A forbidden D literal appearing in these
raw repository payload text or the exact profile-normalized repository path is
preserved. The same literal in any other evaluator-owned key, value,
instruction, role, annotation, or schema is rejected. This payload/path-only
exception prevents lens leakage without altering source truth or repository
identity. A path receives the exception only when it byte-equals the
profile-normalized path reconstructed from the pinned tree/blob manifest; a
caller-supplied display path does not qualify.

### 3.5 Reviewer-lens validator

After construction and source/path closure, recursively traverse the visible
packet. Do not semantic-scan `payloads[*].text`; its `UTF8(text)` bytes were
already content-validated and are the reviewer-visible source body. Do not
semantic-scan a source `path` after exact tree closure. For every other key,
reject these exact names:
`rule_id`, `property_id`, `obligation_id`, `relation_id`, `endpoint_id`,
`caller_id`, `callee_id`, and `subject_id`. For every other string scalar,
compute NFKC then default casefold and reject if it contains any NFKC+casefolded
member of this frozen list:

- `relation.changed_public_callee@1`
- `rust.callee_contract_review@1`
- `callee contract`
- `changed public callee`
- `subject-first`
- `selected by candidate d`
- `candidate d obligation`

Additionally reject an evaluator-owned role/type scalar exactly equal after
NFKC+casefold to `caller`, `callee`, `endpoint`, or `subject`. Matching is
Unicode scalar substring matching with no word-boundary exception. The
validator returns only `reviewer_lens_leak`; its trace records the JSON pointer
but never copies the sensitive value into reviewer artifacts.

## 4. Visible response and hidden binding boundary

The response schema `arm-neutral.source-grounded-disposition@1` is a closed
object with exactly `schema` (the same constant), `task_id`,
`source_inventory_id`, and `disposition`. It contains no arm or D metadata.

`disposition` is exactly one of:

- claim form: closed `{kind:"claim",claims,abstention:null}`. `claims` has 1..3
  closed records, each exactly `{conclusion,summary,observations,mechanism}`.
  Conclusion is `issue_present | issue_absent | inconclusive`. Summary has
  1..1024 Unicode scalars. Observations has 1..8 records. Mechanism is closed
  `{trigger,observed_behavior,consequence}`, each string 1..512 scalars.
- abstention form: closed `{kind:"abstention",claims:[],abstention}`. The
  abstention record is exactly `{reason,basis_loss_ids,observations,
  blocked_question,needed_evidence}`. Reason is one of the three
  `task_blocking_*` reasons in section 5; basis loss IDs has 1..3 strings;
  observations has 1..8 records; both prose fields have 1..512 scalars.

An observation is closed `{source_id,start_line,end_line}` with positive
integers. Observation arrays are strictly sorted by
`(source_id UTF-8,start_line,end_line)` and duplicate-free. Basis loss IDs are
strictly UTF-8-sorted and duplicate-free. Any unsorted array is schema-valid
JSON but typed-contract invalid; the decoder rejects it before scoring.

The reviewer does not supply claim or parsed-record IDs. After validation the
evaluator derives `D("review-disposition", {disposition_hash:H(disposition),
source_inventory_id,task_id})` and retains it with raw/parsed hashes outside
reviewer input.

Each arm's canonical hidden binding is constructed from the authenticated
obligation and internal sources and retains its `hidden_arm_id` for audit.
There is no binding decoder and no mechanical request. `pipeline.py` derives a
closed `JudgeBindingView` from that in-memory binding and omits
`hidden_arm_id` and every field whose key or value identifies A, B, baseline,
ReviewGraphen, packet order, or control label. It retains the hidden task
question and exact source/loss support needed to score relevance.

The view contains exactly `schema`, `task_id`, `rule_id`, `property_id`, sorted
`obligation_ids`, sorted `relation_ids`, sorted endpoint pairs
`{caller_endpoint_id,callee_endpoint_id}`, sorted subject/window records
`{subject_id,window_id,role}`, sorted hidden loss-support records, and
`binding_view_sha256`. The hash is computed immediately as `H` of the same
object without its hash field and is never accepted separately. It contains no
comparison construction kind, packet order, control
state, repository label, `hidden_arm_id`, or exact scalar values `A`, `B`,
`baseline`, or `ReviewGraphen` in evaluator-authored metadata. Repository source payloads are
not fields of this view; they arrive separately through the candidate packet.

## 5. Closed undecidable-question registry

The registry has exactly three entries and is data in a frozen JSON file:

| Registry key | Canonical status predicate | Loss reason | Omitted scope |
| --- | --- | --- | --- |
| `required_source_body` | at least one task-required source has status `unavailable` | `task_blocking_source_unavailable` | `source` |
| `required_reference_target` | at least one task-required reference has status `unresolved` | `task_blocking_reference_unresolved` | `reference` |
| `required_projection_integrity` | the task-required projection has status `integrity_failure` | `task_blocking_projection_integrity` | `projection` |

The words `available | unavailable`, `resolved | unresolved`, and
`valid | integrity_failure` name internal sum-type outcomes, not serialized
status inputs. Source outcome is produced only by repository lookup plus
`EXTRACT`. Reference outcome is produced only by resolving the frozen required
target against the pinned trees. Projection outcome is produced only by
recomputing the frozen projection source-set/hash. Missing, duplicate, extra,
or unknown requirements make the authenticated obligation invalid before
launch. Task-required IDs come only from that obligation.

For registry key `q`, the opaque visible question ID is
`D("undecidable-question", {registry_version:"m20-question-registry.v1",
task_id,registry_key:q})`. The semantic registry key never appears in a
reviewer packet.

## 6. Total loss and paired-opportunity algorithm

Each arm first receives exactly one routine loss, derived from the bounded
scope manifest. It has reason `routine_scope_omission`, scope
`bounded_context`, `primary_abstention_eligible=false`, and null question ID.
Its recovery reference is `D("recovery", {task_id,hidden_arm_id,
bounded_scope_manifest_id})`; its loss ID is derived from reason, scope,
recovery reference, and null question ID, never from the eligibility boolean.
It can never support primary completion.

Task-blocking losses are constructed once from internal outcomes; there is no
serialized eligibility or status authority:

```text
DERIVE_ARM_SUPPORT(task_binding, extract_results, reference_results,
                   projection_result, arm_manifest):
  assert every result was constructed for one unique task-required ID
  result = empty map keyed by (registry_key, reason)
  for registry entry in registry file order:
      support_ids = sorted exact internal obstruction/result IDs satisfying
                    its predicate
      if support_ids is non-empty:
          qid = D("undecidable-question", {registry_version,task_id,registry_key})
          recovery = D("recovery", {task_id,registry_key,support_ids})
          identity = {reason,omitted_scope,recovery_reference:recovery,
                      undecidable_question_id:qid,support_ids}
          loss_id = D("loss", identity)
          visible = {loss_id,reason,omitted_scope,
                     recovery_reference:recovery,
                     primary_abstention_eligible:false,
                     undecidable_question_id:qid}
          hidden_support = {loss_id,task_id,registry_key,qid,support_ids}
          result[(registry_key,reason)] = (visible,hidden_support)
  return result

PAIR_OPPORTUNITY(raw_A, raw_B):
  sig_A = sorted keys(raw_A)
  sig_B = sorted keys(raw_B)
  comparable = (sig_A == sig_B)
  if comparable:
      set primary_abstention_eligible=true on every raw_A/raw_B task loss
  else:
      leave every task loss false in both arms
  emit {comparable, signature_A:sig_A, signature_B:sig_B,
        eligible_question_ids_A, eligible_question_ids_B}
```

There is exactly one aggregate loss per `(registry_key,reason)` per arm.
Therefore exact signature equality guarantees equal cardinality, reason, and
question opportunity even when supporting obstruction IDs differ. If the
signatures differ, any abstention is primary zero in both arms with
`asymmetric_abstention_opportunity`; definite claims remain scorable. The
eligible flags exist only on the two final internal loss values. Pair
eligibility is intentionally absent from the loss-ID preimage; changing the
derived boolean cannot create a new identity. Supporting result/obstruction
IDs remain only in the hidden support record and loss-ID preimage, not in the
reviewer-visible loss object.

An abstention passes mechanical loss support only when pair opportunity is
comparable, every cited loss is a reconstructed eligible task loss in that
arm, every cited reason matches, and every cited question ID closes to the same
task. Routine-only, mixed routine/task, foreign, unknown, or unmatched loss
sets fail.

## 7. Code-point-exact substantive-text algorithm

This section defines `textnorm.v1`. The implementation and vectors, not a
second prose approximation elsewhere, are frozen.

### 7.1 Removal lexeme multiset

Construct raw lexemes from:

1. packet `task_id`, `source_inventory_id`, every source/loss/payload/question/
   recovery ID, every source path, and every source/payload hash;
2. these exact schema enum literals:
   `changed`, `context`, `support`, `routine_scope_omission`,
   `task_blocking_source_unavailable`,
   `task_blocking_reference_unresolved`,
   `task_blocking_projection_integrity`, `bounded_context`, `source`,
   `reference`, `projection`, `claim`, `abstention`, `issue_present`,
   `issue_absent`, and `inconclusive`.

For each raw lexeme, apply Unicode NFKC then Unicode default casefold. Remove
duplicates. Sort by descending Unicode scalar count, then ascending scalar
sequence. Empty normalized lexemes are forbidden.

### 7.2 Normalization pseudocode

```text
NORMALIZE(input, packet, output):
  reject input containing U+0000 or any surrogate code point U+D800..U+DFFF
  utf8_length = length(strict UTF-8 encoding of input)
  s = CASEFOLD(NFKC(input))
  lexemes = BUILD_LEXEMES(packet, output)
  out = empty code-point list
  i = 0
  while i < scalar_count(s):
      matches = every lexeme L for which s scalars at i begin with L
      if matches non-empty:
          choose first L in the frozen lexeme sort
          append exactly one U+0020 SPACE to out
          i = i + scalar_count(L)
      else:
          cp = s[i]
          category = Unicode General_Category(cp) from frozen runtime
          if category begins "P" or category begins "Z" or
             cp is TAB, LF, CR, FF, or VT:
              append exactly one U+0020 SPACE
          else:
              append cp
          i = i + 1
  collapse every non-empty run of U+0020 to one U+0020
  remove leading/trailing U+0020
  scan left-to-right for maximal tokens matching ASCII [a-z][a-z0-9]*
  distinct_tokens = tokens deduplicated then ASCII-sorted
  substantive = (24 <= utf8_length <= field_limit) AND
                (count(distinct_tokens) >= 3)
  return {utf8_length, normalized_text, tokens, distinct_tokens, substantive}
```

Replacement occurs before punctuation conversion. Matching is allowed inside
larger strings. Longest lexeme wins; equal lengths use scalar order. A removed
lexeme always becomes one space, so deletion can never concatenate neighboring
tokens. `field_limit` is 1024 for claim summary and 512 for every mechanism,
blocked-question, and needed-evidence field. Schema length counts Unicode
scalars; this algorithm independently counts UTF-8 bytes.

## 8. Mechanical and primary scoring

Mechanical evaluation consumes only an internal `ArmState` and the result of
`DECODE_MODEL` on raw reviewer bytes. Checks run in this order; all applicable
codes are sorted by frozen `failure_codes.v1.json`:

1. backend process, timeout, empty bytes, raw JSON, and closed output schema;
2. internally computed packet/inventory/payload/binding identities and the
   decoded task/inventory echoes;
3. observations: source exists, range is positive and wholly admitted, and at
   least one observation uses generic role `changed`;
4. claim: conclusion is `issue_present` or `issue_absent`; summary and all
   three mechanism fields pass unchanged `textnorm.v1`;
5. abstention: internal pair opportunity and cited internal losses pass section
   6; blocked question and needed evidence pass `textnorm.v1`;
6. policy, no-tools, client/provider truncation, and evaluator-computed raw and
   parsed hashes are retained by the artifact sink.

`inconclusive` is schema-valid for capture but mechanical zero. A foreign
source/loss, payload mismatch, or asymmetric abstention opportunity cannot be
overridden by judge output.

The internal mechanical result is serialized for audit as
`m20.mechanical_score.v2`. It contains schema, task/unit/hidden-arm IDs,
packet/raw/parsed/binding/pair-opportunity hashes, the five booleans
`process_and_schema_valid`, `closure_valid`, `mechanical_usefulness_valid`,
`policy_valid`, and `hashes_retained`, ordered failure codes, and sorted
normalization-trace IDs. Its own score hash excludes no field.

`m20.primary_score.v3` is closed and contains schema, task/unit/hidden-arm IDs,
candidate and judge-batch IDs, mechanical-score hash, packet/raw/nullable-
parsed/binding hashes, those same five booleans, `utility_judge_valid`,
`completed`, and ordered failure codes. `completed` is true iff all six
booleans are true and failure codes are empty. The reconciled candidate score
is the only source of `utility_judge_valid`; confidence is never an input.
Both primary records are constructed together from the same internal pair and
judge result. There is no single-arm or caller-supplied primary operation. A
judge decode/availability failure sets both utility booleans false; a reviewer
failure may mechanically force only its own candidate to zero.

## 9. One arm-hidden judge batch for two candidates

### 9.1 Sealed permutation

Before any reviewer outcome, `judge_permutation_seed` is loaded from the
authenticated stage manifest. After both reviewer results and bindings exist
in memory, order the two internal candidates by hidden arm ID. Compute
`bit = low_bit(SHA256(UTF8(seed) || 0x00 || UTF8(task_id)))`; reverse when bit
is 1. The pipeline creates an immutable reverse map and writes its sealed audit
artifact
`{batch_id,[{candidate_id,hidden_arm_id}]}`. This artifact is never judge input.

### 9.2 Candidate and batch input

For each permuted internal candidate compute exact `packet_sha256`,
`output_artifact_sha256`, `binding_view_sha256`, and
`mechanical_score_sha256`, then:

`candidate_id = D("judge-candidate", {task_id,packet_sha256,
output_artifact_sha256,binding_view_sha256,mechanical_score_sha256,
mechanical_state})`.

The model-visible closed `m20.utility_judge_batch_input.v1` contains `schema`, `task_id`,
`rubric_id`, `batch_id`, and exactly two `candidates` in permuted order. Each
candidate contains its ID, four hashes, full packet, `JudgeBindingView`, and a
closed `mechanical_state` union. `judgeable` contains the full validated
reviewer output. `mechanical_forced_zero` contains null parsed output and exact
sorted mechanical failure codes; its output-artifact hash still binds the raw
response bytes (or the internally computed empty-response hash). Neither form contains
an arm/control label or `hidden_arm_id`. Thus one malformed reviewer result
does not erase the other candidate's opportunity to be judged.

`batch_id = D("judge-batch", {task_id,rubric_id,candidate_ids_in_order,
packet_hashes_in_order,output_artifact_hashes_in_order,
binding_view_hashes_in_order,mechanical_score_hashes_in_order})`.

These hashes are derived from content immediately before serialization; there
is no API accepting a candidate plus an asserted hash. The same immutable
candidate values remain in memory for output reconciliation.

### 9.3 Batch output and reconciliation

The only judge trust-boundary object is the closed
`m20.utility_judge_batch_output.v1`. It contains `schema`, `batch_id`,
and exactly two score records in input order. Each echoes `candidate_id`, all
four candidate hashes, `score_source`, four integers (not booleans) in 0..2, their
exact integer sum, and verdict `usable | not_usable`. A judgeable candidate
requires `score_source=judge`. A mechanically forced-zero candidate requires
`score_source=mechanical_forced_zero`, four zero dimensions, total zero, and
`not_usable`; any other value invalidates the batch.

One and only one record must exist for each internal candidate. Any missing,
extra, duplicate, reordered, malformed, foreign-ID, hash-mismatched, or partially valid
record invalidates the entire batch and sets `utility_judge_valid=false` for
both candidates. Timeout/unavailable has the same result and no retry.

A candidate passes iff every dimension is at least 1, total is at least 6, and
verdict is `usable`. The pipeline uses its in-memory reverse map to restore the
two results to arms; it never reads the sealed map artifact. The judge cannot
emit or select an arm. One commit consumes
one at-most-90-second batch, preserving exactly 10/40 batches and 900/3,600
judge-model seconds cumulatively.

`utility_rubric.v1.json` contains exactly these ordered dimensions and anchors:

| Dimension | 0 | 1 | 2 |
| --- | --- | --- | --- |
| `source_specificity` | no exact source support | exact support but thin | multiple or especially diagnostic exact supports |
| `hidden_task_relevance` | does not address the bound question | addresses it | directly resolves or sharply isolates it |
| `mechanism_or_blocker_specificity` | generic or identifier echo | specific causal mechanism or blocker | specific and discriminating mechanism or blocker |
| `audit_actionability` | no reproducible next audit action | auditable and reproducible | clear minimal confirmation or repair action |

Its instruction is this exact UTF-8 line:

```text
Score both opaque candidates independently from their admitted source payloads, disposition, and binding view. Do not infer or name an arm. Emit exactly one closed score record per input candidate in input order. A mechanically forced-zero candidate must receive the prescribed zero record.
```

The data-file string excludes a trailing LF; the serialized prompt bytes and
hash are generated and become part of the bundle freeze.

The judge remains a primary usability proxy, not defect truth. A separate
defect projection and clean-control safety result are secondary and cannot
rescue primary completion.

## 10. Frozen reference vectors

The original four versioned vector sets and their 52 expected rows are reused
byte-for-byte unchanged. Six occurrence-summary and sixteen context-v3 rows are
added by the atomic amendments, for 74 rows total. `TEXT_V1` continues to
execute the unchanged section 7 implementation; S01--S07 execute the unchanged
excerpt implementation; canonical JSON boundary attacks use the unchanged
section 2 parser. Vectors that mutate a serialized component (S08--S10, L10, J03--J12)
exercise the offline hostile-artifact or model-output boundary when that
component is no longer a production input. They MUST NOT justify restoring an
intermediate production DTO.

The generator input MUST contain exactly 74 versioned vectors. Every expected
value is a static literal transcribed from this specification. The generator
only parses and copies those literals; it MUST NOT call `normalize`, a closure
constructor, or another production function to synthesize expected values.
The first 52 IDs and values remain unchanged and in the same order.

### 10.1 `TEXT_V1` — 20 vectors

Unless noted, lexeme set is empty and field limit is 1024.

| ID | Input construction | Expected normalized text / distinct tokens | Expected |
| --- | --- | --- | --- |
| T01 | `alpha beta gamma delta!!` (24 bytes) | `alpha beta gamma delta` / 4 | pass, exact lower byte bound |
| T02 | `alpha beta gamma delta!` (23 bytes) | same / 4 | fail, lower bound -1 |
| T03 | field limit 512; `red green blue ` + `x` repeated 497 (512 bytes) | unchanged / 4 | pass, exact 512-byte upper bound |
| T04 | field limit 512; same with `x` repeated 498 (513 bytes) | unchanged / 4 | fail, 512-byte upper bound +1 |
| T05 | field limit 1024; `red green blue ` + `x` repeated 1009 (1024 bytes) | unchanged / 4 | pass, exact 1024-byte upper bound |
| T06 | field limit 1024; same with `x` repeated 1010 (1025 bytes) | unchanged / 4 | fail, 1024-byte upper bound +1 |
| T07 | `alpha alpha beta beta gamma!!` | `alpha alpha beta beta gamma` / `alpha,beta,gamma` | pass, exactly 3 distinct |
| T08 | `alpha alpha beta beta!!!!` | `alpha alpha beta beta` / `alpha,beta` | fail, 2 distinct |
| T09 | `ALPHA Beta gamma delta!!` | `alpha beta gamma delta` | pass, casefold |
| T10 | `ＡＬＰＨＡ beta gamma delta` | `alpha beta gamma delta` | pass, NFKC fullwidth |
| T11 | `ﬁx alpha beta gamma padding` | `fix alpha beta gamma padding` | pass, NFKC ligature |
| T12 | `alpha—beta、gamma delta padding` | `alpha beta gamma delta padding` | pass, Unicode P categories become spaces |
| T13 | lexemes `source:abc`,`source:abcdef`; input `source:abcdef alpha beta gamma` | `alpha beta gamma` | pass, longest overlap removed |
| T14 | lexeme `source:abc`; input `prefixsource:abcsuffix alpha beta gamma` | `prefix suffix alpha beta gamma` | pass, substring removal without concatenation |
| T15 | lexeme `src/Ｆoo.rs`; input `SRC/foo.rs alpha beta gamma padding` | `alpha beta gamma padding` | pass, lexeme NFKC+casefold |
| T16 | lexemes `id:a`,`id:b`; input `alphaid:aid:bbeta gamma delta` | `alpha beta gamma delta` | pass, adjacent removals insert spaces |
| T17 | enum lexemes; input `issue_present issue_absent inconclusive` | empty / empty | fail, enum echo |
| T18 | `猫猫猫 犬犬犬 鳥鳥鳥 padding` | same / `padding` | fail, fewer than 3 ASCII tokens |
| T19 | text containing U+0000 | validation error `invalid_text_codepoint` | reject |
| T20 | text containing an unpaired U+D800 | validation error `invalid_text_codepoint` | reject |

### 10.2 `SOURCE_V1` — 10 vectors

| ID | Input | Expected |
| --- | --- | --- |
| S01 | UTF-8 bytes `fn a() {}\nfn b() {}\n`, range 1..2 | exact input bytes; two LF retained |
| S02 | bytes `a\r\nb\r\n`, range 1..1 | bytes `a\r\n`; CRLF retained |
| S03 | bytes `a\nb`, range 2..2 | bytes `b`; final line without LF |
| S04 | bytes `a\nb\nc\n`, range 2..2 | bytes `b\n`; no adjacent line |
| S05 | bytes `a\n`, range 2..2 | `span_out_of_bounds` |
| S06 | bytes `a\n`, range 0..1 | `span_invalid` |
| S07 | bytes hex `ff0a`, range 1..1 | `non_utf8_source` |
| S08 | valid S04 payload with one ASCII code point changed but stale hash | `payload_hash_mismatch` and packet rejection |
| S09 | valid S04 pair-build source request with `file_bytes_base64` padding removed | `source_blob_base64_invalid` and request rejection |
| S10 | D property literal inside payload text or exact repository path vs same literal in non-path evaluator metadata | payload/path cases pass and preserve exact values; other metadata is `reviewer_lens_leak` |

For S01-S04 the source metadata is exactly `role=changed`,
`snapshot_side=head`, and `path=src/lib.rs`. Their literal expected closure is:

The `text` column uses JSON string notation so `\n` and `\r` denote the
corresponding code points; these escapes are not displayed to the reviewer
after JSON parsing.

| ID | bytes | exact `text` JSON scalar | payload SHA-256 | payload ID | source ID |
| --- | ---: | --- | --- | --- | --- |
| S01 | 20 | `"fn a() {}\nfn b() {}\n"` | `sha256:f334ea00880bc9f3f958220d14eefc216769a299b1db33e7dd8f699feb933b2c` | `source-payload:sha256:9a893b59d60c09fbfbd9be00c7401379644429408aff72f035437d56761801a4` | `source:sha256:6e608d9e27c6d5ff6fe1a909a16b31b5ef8cc97a16b8f980409604bdf700507d` |
| S02 | 3 | `"a\r\n"` | `sha256:8e4621379786ef42a4fec155cd525c291dd7db3c1fde3478522f4f61c03fd1bd` | `source-payload:sha256:87c7f028cd66570a84b8cb65ccdc34fae29e1da5a8e9aa43240ea359e2cfc599` | `source:sha256:14e228c4ce821b74902725dfb9cb14a8fd98b2b0410152c3b19e601f7930c178` |
| S03 | 1 | `"b"` | `sha256:3e23e8160039594a33894f6564e1b1348bbd7a0088d42c4acb73eeaed59c009d` | `source-payload:sha256:fe8f41d806ea9a0ad934bc65aec2efd02a06d09c7dd84c6f38194914dc3977c9` | `source:sha256:24c05768781e512c7712dff020f7dcea9b14c1ae5642f4f528ea099cf657c073` |
| S04 | 2 | `"b\n"` | `sha256:0263829989b6fd954f72baaf2fc64bc2e2f01d692d4de72986ea808f6e99813f` | `source-payload:sha256:ac27005bdcf7aeada30746348b03a534b757c34d5593e7e130dc4e95a3b0eec4` | `source:sha256:565230562d2353c1eef6c6a58114759bd5a10ae00f1ee3139477637cb1a2b681` |

### 10.3 `LOSS_V1` — 10 vectors

| ID | A status signature | B status signature | Expected |
| --- | --- | --- | --- |
| L01 | no task obstruction | no task obstruction | comparable, zero eligible; routine losses false |
| L02 | source-body obstruction | same | comparable; one eligible source reason/question per arm |
| L03 | reference obstruction | same | comparable; one eligible reference reason/question per arm |
| L04 | projection obstruction | same | comparable; one eligible projection reason/question per arm |
| L05 | source+reference | source+reference | comparable; cardinality 2, exact same sorted signatures |
| L06 | source | reference | not comparable; all task losses false in both arms |
| L07 | source | source+reference | not comparable; all task losses false in both arms |
| L08 | two source obstruction IDs | one source obstruction ID | comparable; one aggregate loss per arm and one question opportunity |
| L09 | unknown status enum | any | closed-input rejection before inventory construction |
| L10 | serialized eligible=true contrary to reconstruction | same valid raw status | `pair_opportunity_mismatch` rejection |

### 10.4 `JUDGE_V1` — 12 vectors

| ID | Mutation/boundary | Expected |
| --- | --- | --- |
| J01 | two valid candidates, each dimensions `1,1,2,2`, total 6, usable | both pass |
| J02 | valid sealed permutation bit 1 | output maps back to original hidden arms only through reverse map |
| J03 | duplicate candidate ID | entire batch invalid; both fail |
| J04 | one candidate record missing | entire batch invalid; both fail |
| J05 | third candidate record added | entire batch invalid; both fail |
| J06 | one echoed packet/output/binding hash changed | entire batch invalid; both fail |
| J07 | one score has total unequal to dimension sum | entire batch invalid; both fail |
| J08 | dimensions `1,1,1,1`, total 4, verdict usable | candidate fails threshold; other valid candidate independent |
| J09 | dimensions `0,2,2,2`, total 6, verdict usable | candidate fails per-dimension floor |
| J10 | `hidden_arm_id` injected into input candidate/view | judge-input schema/view rejection |
| J11 | one valid judgeable candidate plus one `mechanical_forced_zero` candidate | valid candidate is scored independently; forced record is exact all-zero/not-usable |
| J12 | batch timeout/unavailable | both candidates fail utility with no retry |

### 10.5 `OCCURRENCE_V1` — 6 vectors

`occurrence.v1.json` contains the literal report/count/digest/ID values. O01
closes the empty observed set while retaining the singular unknown-latent
limitation. O02 closes one direct occurrence. O03 closes three occurrences in
two files and proves that summary-row counts are not occurrence counts. O04
changes the public report count, O05 withholds one internal occurrence from the
public projection, and O06 supplies the rejected `located_call_occurrences`
shape. O04--O06 must be rejected by independent rebuild.

### 10.6 `CONTEXT_V3` — 16 vectors

`context.v3.json` contains the independent v2/v3 hashes, empty and nonempty
commitments, anchor ID, complete projection hash, and loss-summary digest as
static literals. C01 checks exact v3 policy bytes/hash; C02 preserves the v2
hash while proving it is not active; C03 checks known-empty commitment; C04
checks all four commitments and full projection hash; C05 checks the exact
anchor StableId. C06 reverses subject order, C07/C08 are the exact 4,096/4,097
materialization boundary, C09 creates a partition gap, and C10 claims unknown
latent cardinality without qualification. Every negative must be rejected.
C11 binds the independently accepted Option C algorithm and its reference
hash; C12 binds the production/oracle/runtime/real-pair source bytes; C13 binds
the pre-seal measurement record; C14 binds all required semantic mutants; C15
retains the superseded pre-oracle manifest hash. Option C independently
reconstructs accepted, reached, and support-anchor sets without production
context helpers, then compares them before replay-checking windows, loss,
partition, materialization, latent cardinality, IDs, hashes, and canonical bytes.
C16 is a static three-input execution-identity vector. Its literal inputs hash
to `sha256:705059fcb911daf5a4b9ca9f04335e029d39c5d2bf1178596d10e8998330fa46`
under the exact formula in section 12 and also bind the immediately superseded
Wave 7 freeze.

The applied H3N01 attack deletes `.casefold()` from production text
normalization and invokes this same literal vector runner. T09 and T15 then
fail. A generated expectation cannot mask that failure.

## 11. Audit artifacts, D5 disposition, and non-gameable acceptance

### 11.1 One-way audit artifact contract

`ArtifactSink` receives immutable internal values and exact serialized model
requests/responses. It computes bytes and hashes before an atomic create-new
write. It has no read method. Production uses these fixed relative paths:

```text
launch.json  repository.json  obligation.json  pair.json  budget.json
slots/0/packet.json  slots/0/request.json  slots/0/raw.bin
slots/0/execution.json  slots/0/parsed.json  slots/0/mechanical.json
slots/1/packet.json  slots/1/request.json  slots/1/raw.bin
slots/1/execution.json  slots/1/parsed.json  slots/1/mechanical.json
judge/permutation.json  judge/request.json  judge/raw.bin
judge/execution.json  judge/parsed.json  judge/utility.json
primary.json  ledger.json  seal.json
```

For the section 3.3.1 `model_ineligible` terminal variant, the artifact set is
exactly `launch.json`, `repository.json`, `obligation.json`, `pair.json`,
`budget.json`, `slots/0/packet.json`, `slots/1/packet.json`, `ledger.json`, and
`seal.json`; slot request/raw/execution/parsed/mechanical paths and all judge
and primary paths are forbidden. The two packet artifacts are required so
`verify-run` can independently recompute `budget.json` without trusting
`pair.json` or caller-supplied counts.

`parsed.json` is omitted on decode failure and that omission is declared in the
execution record. Each execution record also contains the section 3.3.1
observation-only token record. Every other launched raw response is retained
even on failure. `ledger.json` contains sorted closed entries
`{path,kind,byte_length,sha256}` for every preceding artifact and
`ledger_sha256=H(body_without_hash)`. `seal.json` contains exactly `schema`,
`run_id`, `evaluator_execution_sha256`, `stage_manifest_sha256`,
`ledger_sha256`, `pipeline_terminal_state`, and
`run_seal_id=D("m20-run-seal",object_without_id)`. The permutation artifact is
sealed but never reread to attribute arms.

Offline `verify-run RUN_ROOT` is a separate hostile-artifact program. It reads
all files, rejects missing/extra paths and fields, recomputes every byte/hash/
ID/ordering/source/payload/loss/candidate/batch/primary relation, reparses raw
model bytes, replays scoring, checks the pinned repository objects when the
repository is available, and validates ledger/seal closure. It emits audit
evidence only and cannot change a recorded primary result. Production `run`
MUST NOT import or call its read path.

### 11.2 Generated full-run fixtures

The fixed template seed remains `m20-evaluator-fixtures-v1`; no clock, OS
randomness, repository state, or network is used. A synthetic content-addressed
repository adapter and deterministic in-process `ModelTransport` drive the
actual `RUN` function. `fixtures.generated.json` contains, for every fixture,
the launch selectors, transport raw-byte sequence, expected terminal result,
and the complete artifact set as `{path,bytes_base64,byte_length,sha256}`. It
MUST include A/B claim success, equal-opportunity task abstention success,
routine-only zero, inconclusive zero, identifier-echo zero, reviewer raw/decode
failures, judge whole-batch failure, and all original 52 pipeline-vector
expansions. The 16 Stage-0 closure vectors execute their pure constructors
directly and are not artificial reviewer runs. A list of
case names is not a fixture.

`attacks.generated.json` contains the attack ID, semantic production mutation
or hostile model/artifact operation, the named behavioral oracle below, and
the expected changed result. `run-attacks` applies production mutations to a
fresh temporary evaluator copy or supplies the hostile boundary bytes; it must
execute the named oracle through the real public pipeline or offline verifier.
It MUST NOT infer success from manifest length, test discovery count, a copied
expected value, or displayed `passed: N`. `generate-fixtures --check` and
`run-attacks` regenerate/execute rather than trust generated summaries.

### 11.3 D5 findings under the pipeline design

| D5 | Classification | Why / residual implementation obligation |
| --- | --- | --- |
| B1 nested DTO | **structurally eliminated** | all public intermediate DTOs/schemas disappear; only fully nested reviewer/judge model outputs are decoded |
| B2 double source authority | **structurally eliminated** | source availability is the sole `repository lookup -> EXTRACT` result; no status input exists |
| B3 packet/duplicate/path forgery | **structurally eliminated as an input attack** | no packet/path/source input exists; repository-derived path and private map constructors must still reject duplicate/internal bugs, and offline audit must reject tampering |
| B4 raw/parsed and loss provenance | **remains at the model boundary** | raw bytes are parsed once into the only parsed value; loss/opportunity forgery disappears because those values are internal |
| B5 candidate stale-content hashes | **structurally eliminated** | candidate content and hashes are co-derived and remain in memory; judge can return only echoes/scores, which are closed-decoded |
| B6 primary type/provenance | **structurally eliminated as a separate input** | no primary request exists; exact two results are built together, while foreign judge IDs/bool-as-int are rejected at the judge boundary |
| B7 hollow fixtures/mutations | **remains** | full-run generated artifacts and actual semantic production mutants are required below |
| B8 data/freeze authority | **remains** | runtime loads frozen data, freeze runs every named oracle, dependency/import and G6 checks are normative |

Thus five D5 findings disappear as production trust-boundary classes and three
remain as model-decoder or evaluator/freeze correctness obligations.

### 11.4 D4 production mutations and probes

For every `REQUIRED` row, applying the mutation to the production code in a
temporary copy MUST make the named oracle fail. `UNREPRESENTABLE` means the
exact old public input route is absent; the acceptance oracle verifies the
closed launch/CLI surface. `AUDIT` means production cannot consume the state,
but post-run corruption must fail `verify-run`.

| ID | Attack after redesign | Class | Required oracle |
| --- | --- | --- | --- |
| M01 | source ceiling 65,536 -> 65,537 | REQUIRED | two exact 65,536-byte arms are eligible; one 65,537-byte arm yields the sealed pair-wide `model_ineligible` result, zero calls, and no primary record |
| M02 | judge total floor 6 -> 5 | REQUIRED | J08 remains false while J01 remains true |
| M03 | permutation always identity | REQUIRED | two frozen task IDs producing bit 0/1 map through the in-memory reverse map |
| M04 | omit question ID from pair signature | REQUIRED | L03/L06/L07 exact opportunity results |
| M05 | truncate multiple support IDs | REQUIRED | L08 exact hidden support and loss identity |
| M06 | text upper bound +1 | REQUIRED | T04 and T06 reject, T03 and T05 pass |
| M07 | disable orphan/foreign payload invariant | REQUIRED+AUDIT | constructor corruption oracle and tampered-run verifier both fail |
| M08 | stop retaining raw hash | REQUIRED+AUDIT | reviewer raw/hash artifact oracle and tampered raw file fail |
| M09 | permit unknown source role | REQUIRED | frozen-obligation/source constructor rejects before packet/model call |
| M10 | put runtime into bundle preimage or remove execution contract | REQUIRED | G6 cross-provenance bundle equality and runtime incompatibility oracle |
| P01 | caller supplies unrelated raw hash | UNREPRESENTABLE+AUDIT | no raw-hash input field; raw artifact tamper fails verification |
| P02 | caller forges packet constants then rehashes | UNREPRESENTABLE+AUDIT | launch forbids packet; tampered packet fails verification |
| P03 | caller duplicates payload | UNREPRESENTABLE+AUDIT | launch forbids payload; tampered duplicate fails verification |
| P04 | caller authors eligible loss/opportunity | UNREPRESENTABLE | launch schema and `run` signature contain no such values |
| P05 | caller swaps reverse-map arm under stale seal | UNREPRESENTABLE+AUDIT | pipeline uses memory map; artifact swap fails seal verification |
| P06 | judge dimensions are booleans | REQUIRED | raw judge decode rejects exact type and both utility results are false |
| P07 | judge top/score has extra field | REQUIRED | closed raw judge decode rejects and both utility results are false |
| P08 | caller supplies only one primary arm | UNREPRESENTABLE | no primary command; missing judge record is model-boundary whole-batch failure |

Combined labels mean that both clauses apply.

### 11.5 D5 new attacks N01--N25

| ID | Redesign disposition | Required oracle |
| --- | --- | --- |
| N01 | UNREPRESENTABLE | no mechanical command or nested intermediate envelope exists |
| N02 | REQUIRED | scalar/ill-shaped judge candidate scores fail raw judge decode |
| N03 | UNREPRESENTABLE | no source status exists beside `EXTRACT` result |
| N04 | UNREPRESENTABLE | no status-record object exists in launch or obligation |
| N05 | REQUIRED | only tree-derived normalized relative paths reach packet; absolute frozen path aborts before model call |
| N06 | REQUIRED | parsed reviewer value is produced only by parsing the retained raw bytes |
| N07 | UNREPRESENTABLE | no caller loss/opportunity/packet input exists |
| N08 | AUDIT | payload extra field in a copied run fails hostile artifact decode |
| N09 | AUDIT | source extra field plus rehash fails hostile artifact decode |
| N10 | AUDIT | loss extra field plus rehash fails hostile artifact decode |
| N11 | UNREPRESENTABLE+AUDIT | candidate packet cannot return from judge; artifact stale-hash mutation fails verify-run |
| N12 | UNREPRESENTABLE+AUDIT | binding view cannot return from judge; artifact stale-hash mutation fails verify-run |
| N13 | REQUIRED | internal mechanical failure cannot be judgeable; contradictory artifact fails audit |
| N14 | REQUIRED | judge string `"false"` where boolean/enum is required fails decode; primary never truthy-coerces |
| N15 | REQUIRED | foreign batch/candidate ID in raw judge output invalidates whole batch |
| N16 | REQUIRED | duplicate required IDs make frozen obligation invalid prelaunch |
| N17 | UNREPRESENTABLE+REQUIRED | no source-request input; duplicate internal source construction is an invariant failure |
| N18 | REQUIRED | non-UTF-8 blob yields the unique source obstruction used by loss derivation |
| N19 | REQUIRED | `span_invalid` in frozen obligation is prelaunch invalid, never an eligible loss |
| N20 | REQUIRED | depth 33 and depth 2,000 raw model JSON return typed decode failure without uncaught exception |
| N21 | REQUIRED | unchanged canonical decoder rejects duplicate JSON key |
| N22 | REQUIRED | unchanged canonical decoder rejects integer `2^53` |
| N23 | REQUIRED | unchanged canonical decoder rejects non-ASCII object key |
| N24 | REQUIRED | mutating any production decoder/pipeline oracle makes freeze refuse manifest generation |
| N25 | REQUIRED | changing frozen instruction changes actual request bytes and bundle hash; no hard-coded shadow remains |

Generated data uses only ASCII class labels. The acceptance review must inspect
the mutation operator and named oracle for each row; a green aggregate counter
is neither necessary nor sufficient. The freeze gate runs all REQUIRED and
AUDIT oracles, verifies every UNREPRESENTABLE public-surface claim, and refuses
to emit a manifest if any production mutant survives or any hostile artifact/
model output is accepted.

### 11.6 Atomic amendment mutations

These are applied source mutations, not manifest-only rows:

| ID | Mutation | Required literal oracle |
| --- | --- | --- |
| OCC01 | disable rebuilt/public byte comparison | O04/O05 fail to reject |
| OCC02 | accept `located_call_occurrences` | O06 fail to reject |
| CTX01 | change materialized bound 4,096 to 4,097 | C08 is wrongly accepted |
| CTX02 | remove callee-then-caller order check | C06 is wrongly accepted |
| CTX03 | remove admitted/lost anchor partition equality | C09 is wrongly accepted |
| CTX04 | change Option C algorithm identity to Option B | C11/reference hash fails |
| CTX05 | change the independent-oracle source hash | C11/C12 fails |
| CTX06 | change the pre-seal measurement hash | C11/C13 fails |
| CTX07 | remove the production-helper-reuse mutant | C11/C14 fails |
| SC01 | substitute the runtime hash for the semantic-reference input | C16 fails |
| SC02 | remove `measurement_record_sha256` from the implementation manifest key literal | normative spec check fails |

An attack ID without an explicit source mutation or nontrivial probe is a
failure. The runner refuses unknown IDs and requires each mutation target to
occur exactly once before replacement.

## 12. Freeze protocol

This manifest is the sole atomic re-freeze for ADR 0038 sections 3.4 and 5.4.
It may be emitted only after both amendment vector sets and applied mutations
pass. An occurrence-only manifest is invalid and must never be treated as an
earlier phase of this freeze.

The evaluator bundle contains every regular file below `evaluator/` except
`__pycache__`, `.pyc`, coverage files, and temporary files. It includes Python
modules, closed schemas, registry/rubric/failure-order JSON, templates, all 74
source reference vectors, tests, generated fixtures, and generated inventory.

Before hashing, enforce:

- paths are UTF-8 NFC, relative, `/`-separated, and contain no `.`/`..` segment;
- `.py`/`.md`/`.txt` are UTF-8 without BOM, LF-only, with exactly one final LF;
- `.json` bytes equal `JCS(parsed JSON)` with no trailing LF;
- no symlink, device, socket, executable binary, or unlisted extension;
- every Python import is statically resolved to the standard library or this
  evaluator tree; dynamic import, `exec`, `eval`, and undeclared executable
  lookup are rejected rather than covered by a literal `stdlib_only=true`;
- frozen data files are loaded by the real pipeline and shadow literals are
  rejected by the N25 authority oracle;
- generated full-run artifacts pass `generate-fixtures --check`, all 74
  reference vectors execute, every section 11 REQUIRED/AUDIT oracle detects
  its actual mutation, and every UNREPRESENTABLE surface check holds.

For each sorted path record `{path,kind,byte_length,sha256}` over its canonical
bytes. `design_spec_sha256` is separately the SHA-256 of this file after the
text canonicalization above and is recorded in the manifest and later protocol
freeze; it is deliberately not a bundle-hash input. Adopt two identities
because portable artifact identity and executable semantic compatibility are
different claims.

The portable identity is computed without inspecting the current process:

`evaluator_bundle_sha256 = H({schema:"m20.evaluator_bundle.v1",
evaluator_version:"m20-evaluator.pipeline.v1",files})`.

Thus any clone containing byte-identical canonical evaluator files reproduces
the same bundle hash on any host. Runtime, design-document bytes, executable,
platform, hostname, clock, and environment MUST NOT enter this preimage.

The closed semantic requirement record is exactly:

`runtime_requirements = {schema:"m20.evaluator_runtime_requirements.v1",
implementation:"cpython",python_version:[3,13,5],
unicodedata_version:"15.1.0",stdlib_only:true}`.

Compute `runtime_requirements_sha256 = H(runtime_requirements)` and:

`evaluator_execution_sha256 = H({schema:"m20.evaluator_execution.v1",
evaluator_bundle_sha256,runtime_requirements_sha256,
semantic_acceptance_reference_sha256})`.

The following single-line JSON literal is normative and is parsed by the
executable seal check. Its order is the implementation preimage construction
order; canonical hashing still sorts object keys:

M20_EXECUTION_PREIMAGE_KEYS_JSON=["schema","evaluator_bundle_sha256","runtime_requirements_sha256","semantic_acceptance_reference_sha256"]

The execution hash is also reproducible from frozen data; it identifies the
bundle, runtime contract, and Option C semantic-acceptance reference, not the
machine that happened to freeze it.
The manifest separately records observed provenance exactly as
`{python_version,executable_sha256,implementation,unicodedata_version,platform,
stdlib_only}`. Observed provenance is disclosed but is in neither hash
preimage. A different executable hash or platform is permitted only when all
semantic requirement fields match and the complete generated-fixture,
reference-vector, and named attack checks pass.

The closed `m20.evaluator_freeze.v1` manifest contains exactly these 15 keys:

M20_FREEZE_MANIFEST_KEYS_JSON=["schema","evaluator_version","design_spec_sha256","files","evaluator_bundle_sha256","runtime_requirements","runtime_requirements_sha256","semantic_acceptance_reference_sha256","measurement_record_sha256","supersedes_freeze_manifest_sha256","evaluator_execution_sha256","runtime_provenance","generated_fixture_inventory_sha256","reference_vector_set_sha256","mutation_manifest_sha256"]

No current
runtime value may be copied into `runtime_requirements`; that record is the
literal preregistered contract above.

Verification order is normative:

1. Recompute file records and `evaluator_bundle_sha256`; separately recompute
   `design_spec_sha256`. Any mismatch with its stored value exits 3.
2. Parse the two normative key literals above and compare them with the
   implementation's execution preimage and manifest key set. Then recompute
   the stored requirement, semantic-reference, and three-input execution
   hashes; any mismatch exits 3.
3. Compare the observed implementation, Python major/minor/micro,
   `unicodedata.unidata_version`, and stdlib assertion with
   `runtime_requirements`. Any mismatch is a hard pre-execution incompatibility:
   exit 3 before fixture generation, packet construction, or scoring. It is not
   a warning, arm zero, or launched stage.
4. On a compatible runtime, run fixture regeneration and every reference and
   named attack check. Failure exits 3. Record the new observed provenance even
   when it differs from freeze-time executable/platform provenance.

`verify-frozen` emits the closed result
`{schema:"m20.verify-frozen.v1",bundle_valid,design_spec_valid,
execution_contract_valid,runtime_compatible,checks_valid,ok}`. The first four
fields are booleans. `checks_valid` is null when `runtime_compatible=false` and
otherwise boolean. `ok` is true exactly when the other four booleans and
`checks_valid` are true. This lets an incompatible host demonstrate portable
bundle identity without misrepresenting behavioral replay; command exit remains
3 whenever `ok=false`.

The Unicode pin is semantic, not decorative. NFKC, casefold, and General
Category affect `textnorm.v1`; T10, T11, T12, and T15 explicitly exercise that
surface. A runtime reporting a Unicode database other than 15.1.0 cannot replay
m20. Provisioning a matching runtime does not create a new study. Changing the
required Python or Unicode version changes `runtime_requirements_sha256` and
`evaluator_execution_sha256` and MUST create a new versioned study even if all
vectors happen to pass.

Also record `generated_fixture_inventory_sha256`,
`reference_vector_set_sha256`, and `mutation_manifest_sha256`; the last is the
hash of `attacks.generated.json`. The implementation author MUST write
the portable bundle hash, execution hash, these three artifact hashes, and the
manifest path into `preregistration.json` only after all checks pass. A
normative evaluator-tree file change changes the bundle and execution hashes; a
semantic runtime-requirement change changes the execution hash; a design-spec
change changes its separately recorded hash and the later protocol freeze; an
observed compatible executable/platform provenance change changes none of
those identities. Every normative post-freeze change requires a new versioned
study and may not repair or overwrite a frozen result.

For practical reproducibility gate #9, three claims remain separate: canonical
artifact identity is clone-reproducible without a runtime match; scorer and
fixture behavior are reproducible only under the exact semantic runtime pin;
freeze-time executable/platform provenance is not claimed reproducible. Gate
#9 is therefore satisfied conditionally on provisioning the documented runtime,
not by clone bytes alone. An incompatible runtime must refuse rather than emit
a purported reproduction.

Freeze occurs before Candidate D slice implementation and before resolving m20
repositories/ranges. The evaluator author and slice implementer identities are
recorded and MUST differ. Manifest generation calls the same public `run`,
`verify-run`, model decoders, repository constructors, and attack operators
named in section 11; a separate shallow freeze-only test path is forbidden.

## 13. Required CLI and acceptance checks

The CLI surface is fixed:

- `python3 -m evaluator stage0 NEW_OUTPUT_ROOT [--jobs N]`
- `python3 -m evaluator run FROZEN_LAUNCH NEW_OUTPUT_ROOT`
- `python3 -m evaluator verify-run RUN_ROOT`
- `python3 -m evaluator generate-fixtures --check`
- `python3 -m evaluator verify-reference-vectors`
- `python3 -m evaluator run-attacks`
- `python3 -m evaluator freeze-manifest OUTPUT`
- `python3 -m evaluator verify-frozen MANIFEST`

`run` requires a nonexistent output root and uses create-new writes. It returns
0 when a sealed terminal paired result or the section 3.3.1 sealed non-model
eligibility result exists, including registered post-launch model failure
zeros; 2 on authenticated launch/preflight rejection other than the byte
ceiling; 3 on
freeze/runtime incompatibility; and 4 on evaluator invariant or artifact-write
failure. Exit 4 stops the study and MUST NOT be converted into an arm zero.
Other commands return 0 only for their complete semantic operation, 2 for an
invalid hostile artifact/input, 3 for freeze mismatch, and 4 for an evaluator
invariant failure. Stdout is one canonical JSON record; stderr is
non-normative diagnostics.

No production command accepts an intermediate JSON artifact, raw-response
file, parsed output, asserted hash, packet, status, loss, binding, candidate,
batch, score, seed, arbitrary module/command/shell string, network location, or
repository outside the authenticated allow list. Old intermediate subcommands
must be absent, not retained as undocumented aliases.

### 13.1 Frozen Stage 0 production entrance

`stage0` accepts exactly one initially nonexistent destination, the optional
operational `--jobs` control described below, and no corpus, packet, score,
model result, clock, seed, or asserted intermediate input. Before corpus
resolution or root creation it loads `preregistration.json`, requires every
active hash/path to be non-null, hashes the exact manifest bytes and matches
`freeze_manifest_sha256`, matches manifest bundle/execution hashes to their
active slots, requires manifest `supersedes_freeze_manifest_sha256` to equal the
last `freeze_history` manifest hash, and requires `verify-frozen` to return
`ok=true`; otherwise it refuses without repository observation. It
enumerates the frozen corpus, rejects any frame other than 300 unique clusters,
and invokes the existing frozen generic v3 pipeline twice from independently
empty `build-1` and `build-2` directories for every cluster. The driver owns
only typed closure validation, exact-set reductions, the seven frozen gates,
and label-independent selection. It invokes no reviewer or judge transport.

The driver schedules clusters concurrently. It computes its worker
ceiling once as `max(1, min(16, available_logical_cpus - 2))`, with unavailable
CPU count or a count at most two yielding one worker. The optional `--jobs`
operational argument may select a width from one through that ceiling; it is
public CLI surface but not a corpus/identity input, and its value is absent from
every output, manifest, hash, gate, and selection input. Each job
owns one cluster directory, runs that cluster's two builds against distinct
empty output/cache directories, and shares no mutable cache or reduction state
with another job. The reducer waits for 300 unique
terminal cluster IDs and sorts by their UTF-8 bytes; it never consumes
submission or completion order.

The complete output tree, every file byte, artifact-manifest hash, gate result,
and selection-manifest hash MUST be identical for one worker and the computed
ceiling. Acceptance injects widths 1 and the ceiling plus reversed completion
order over the same fixed corpus. Worker count/order, PID, host, wall time,
per-cluster process CPU time, peak bytes, and utilization are excluded from all
canonical values. Optional timing diagnostics are written only outside the
closed output root and cannot affect a gate, identity, or selection.

For canonical repository ID `r`, base object ID `b`, and head object ID `h`,
the implementation computes exactly
`D("commit-cluster",{"cluster_contract":"m20.commit_cluster@1",
"experiment_id":"m20-changed-public-callee-utility-v1",
"repository_id":r,"base_commit_oid":b,"head_commit_oid":h})`.
The literal, non-generator-derived reference vector is:

```json
{"base_commit_oid":"0123456789abcdef0123456789abcdef01234567","commit_cluster_id":"commit-cluster:sha256:f140a5acf366f5e1195aed47206d17c9c858c8d5a4821181f1588aa02261787e","head_commit_oid":"89abcdef0123456789abcdef0123456789abcdef","repository_id":"github.com/example/repository"}
```

The root is closed to `corpus-manifest.v1.json`, the 300 digest-named cluster
directories with two clean build directories, `stage0-result.v1.json`,
`stage0-selection.v1.json`, and `artifact-manifest.v1.json`. The artifact
manifest lists every file; its own row is an explicit self-description marker,
not a circular self-hash. Missing, duplicate, foreign, reordered, unlisted, or
nondeterministic material is a typed refusal.

`m20.stage0-selection.v1` is a distinct launch-authorization type. It contains
only eligible IDs, frozen hash order, cumulative 10/40 memberships, and its
seal. The primary scorer rejects that type before inspecting score fields, so
Stage 0 cannot contribute to `n`, `b`, `c`, `n00`, or a primary cell.

Implementation acceptance is the conjunction of the named section 11 oracles,
byte-identical full-run regeneration, unchanged original 52-vector
expectations plus the 16 atomic-amendment literals, exact
one-batch/two-result behavior, independent hostile-artifact verification, and
the G6 freeze round trip. No test-count threshold, aggregate pass number, or
self-reported coverage can satisfy this contract.

## 14. Boundary with the other m20 documents

The existing documents retain only question, authority, corpus, unit, sample,
Stage 0 sets/gates, descriptive rectangles, budgets, control timing, sealing
order, and interpretation limits. They reference this specification and the
eventual five freeze hashes for packet/source/loss construction, normalization,
mechanical scoring, judge batch typing/permutation/reconciliation, artifact
generation, and attack behavior. ADR 0038 remains the source of the two
amendment decisions; this evaluator owns their executable summary/context
closure. They MUST NOT restate those algorithms.

Closure summary:

- D3-B1 closes because reviewer packets directly expose and hash-close exact
  strict-UTF-8 Rust excerpt text, while the D-lens exception is confined to
  payload text and exact tree-closed repository paths.
- D3-B2 closes because one total registry function reconstructs losses and
  enables abstention only under exact paired signature equality.
- D3-B3 closes because one closed two-candidate batch returns exactly one
  hash-bound score per opaque candidate and a sealed caller map restores arms.
- D3-B4 closes because hashed code plus 20 exact text vectors fixes every
  normalization and threshold choice.
- ADR 3.4/5.4 close operationally because exact occurrence and context sets
  are rebuilt before their bounded public commitments are admitted, while the
  resolved obligation denominator and primary scorer are untouched.
- D5-B1/B2/B3/B5/B6 close as production boundary classes because intermediate
  states have no public input representation; B4/B7/B8 remain explicit
  decoder/acceptance/freeze responsibilities in sections 2, 8, 11, and 12.
