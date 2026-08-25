# m20 normative execution protocol

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, and **SHALL NOT**
are normative. `preregistration.json` and
`POWER_AND_DECISION_BOUNDARIES.md` are part of this protocol.
`EVALUATOR_SPEC.md` is the implementation contract for the executable evaluator.
After freeze, the evaluator bundle and the non-null hashes recorded in
`preregistration.json` control packet construction and scoring; prose MUST NOT
reimplement them.

## 1. Roles and authority

1. The **implementation unit** implements the slice governed by
   `docs/adr/0038-changed-public-callee-relation-slice.md`. It MUST NOT choose
   repositories, ranges, packets, arm order, or outcomes.
2. The **independent evaluator author** implements and freezes the Python
   evaluator bundle before Candidate D slice implementation. This person/agent
   MUST NOT implement the slice.
3. The **protocol custodian** verifies the evaluator hashes and freezes the
   remaining protocol inputs.
4. The **evaluation operator** resolves the corpus and invokes only the frozen
   evaluator to construct and score both arms. It MUST NOT edit generated
   packets, fixtures, or scores.
5. Two **control labelers** who did not implement the slice independently
   classify every model-eligible commit before arm outcomes. Their labels never
   change primary sample membership or order.
6. The **model transport** is invoked only by the in-process evaluator,
   enforces identity, the common requested settings, no-tool execution, and
   timeouts. The evaluator itself enforces the equal source-byte ceilings. The
   transport returns raw bytes plus execution metadata; its token data is
   observation-only, and it cannot provide parsed output or read evaluator
   artifacts.
7. The **utility judge** receives one frozen two-candidate arm-hidden batch per
   commit and emits non-authority usability proxy records.

All roles operate under the same Unix identity on one machine. The role split
is procedural, not a security boundary or organizational blind.

## 2. Freeze point

Before Candidate D slice implementation and before resolving any m20 repository
or range, the custodian MUST verify and hash-bind the independent evaluator
bundle, semantic runtime contract, runtime provenance manifest, all schemas, 74
reference vectors, generated full-run fixtures, and attack manifest as specified in
`EVALUATOR_SPEC.md`. All eight hash slots, including the active exact-byte
`freeze_manifest_sha256`, and the manifest-path slot in
`preregistration.json` MUST be non-null and `verify-frozen` MUST pass on CPython
3.13.5 with Unicode database 15.1.0.

The prior bundle lacked the Stage 0 production entrance required by section
3.1 and is recorded as superseded in `preregistration.json`. Its active hash
slots are intentionally null until the fourth atomic pre-observation re-seal.
Corpus resolution and Stage 0 execution are forbidden while they are null.

`evaluator_bundle_sha256` binds only canonical evaluator-tree file bytes and is
portable across clones. The design-spec hash is recorded and protocol-frozen
separately. `evaluator_execution_sha256` binds that bundle to the frozen
semantic runtime requirements and Option C semantic-acceptance reference. Exact executable hash and
platform are provenance, not hash inputs. A Python implementation/version or
Unicode-database mismatch is exit 3 before packet construction, scoring, or
stage launch; it is never a warning or a scored zero. The operator may provision
the matching runtime without changing the study. Changing the required runtime
creates a new versioned study. A compatible executable/platform change is
allowed only after every frozen vector, fixture, and named attack oracle passes and
its provenance is recorded.

The later protocol freeze also binds the slice revision, the five normative
m20 design files, `MODEL_PIN_RATIONALE.md`, ADR 0038,
rule/property/profile/context identities, the deterministic
source-byte budget procedure and observation-only token-record schema, leakage
scanner, and public repository/obligation/commit/arm/judge-permutation seeds.
The profile DTO hash and context DTO hash remain unchanged.

After this point, the endpoint, failure classification, analysis unit, profile,
repository rule, sample order/sizes, seven Stage 0 gates, rectangles,
failure=`0` rule, control-adequacy rule, stop/advance rules, and arm ceilings
MUST NOT change. Here `arm ceilings` means the identical 65,536-byte
admitted-source ceilings and common requested backend settings, never an input-
token ceiling. Any repair creates a new versioned study and cannot replace the
frozen m20 result.

## 3. Corpus resolution after freeze

The evaluator enumerates only direct child Git repositories of
`/home/rizumita/github/` plus
`/home/rizumita/workspace/reviewgraphen`. Eligibility and canonical origin
normalization are exactly those in `preregistration.json`. Eligibility is
decided without running Candidate D or consulting model outcomes. The complete
candidate registry and reasons are sealed first.

Eligible repositories are ordered by the frozen repository-selection hash;
exactly the first three are selected. Fewer than three is corpus-resolution
infeasible. No repository may be substituted after D prevalence is observed.

For each repository, pin resolved HEAD and take exactly the first 100 commits
on its first-parent chain. Each cluster is
`first_parent(commit) -> commit`. Merge, non-Rust, zero-obligation, and
apparently clean commits remain in the 300-cluster frame. The manifest binds
repository ID/path/origin, base/head IDs, order, snapshot, the
`rust.production.v1` ID/hash, rule, and extractor. FSL, CaseGraphen, or
ReviewGraphen selection MUST disclose prior development inspection and cannot
create holdout validity.

Terms such as `production`, `production diff`, `non-test Rust source`, path
normalization, category precedence, and typed exclusion reasons mean exactly
the canonical `rust.production.v1` contract in ADR 0038. This protocol MUST NOT
invent a second matcher definition.

### 3.1 Frozen Stage 0 driver

The 300-cluster driver is part of the hash-frozen evaluator. It is not an
unfrozen orchestration script and MUST NOT be implemented under `scripts/`.
The only Stage 0 production entrance is
`python3 -m evaluator stage0 NEW_OUTPUT_ROOT [--jobs N]`, where
`NEW_OUTPUT_ROOT` does not exist and is used only as a filesystem destination.
Immediately before corpus resolution, the command must load
`preregistration.json`; require every active hash and manifest path non-null;
independently match the exact manifest-byte SHA-256, its bundle/execution hashes,
and its `supersedes_freeze_manifest_sha256` against respectively the active
slots and last `freeze_history` manifest hash; and require `verify-frozen` to
return `ok=true`. Otherwise it refuses without resolving a repository or
creating the output root. The evaluator itself
performs corpus enumeration, two independent clean deterministic builds of
every cluster, all seven gate reductions, model-eligibility derivation, and the
label-independent Stage 1/2A hash selection. It MUST make zero reviewer and
zero judge calls.

Cluster execution MAY be parallel. The serial requirement in section 8 applies
only to the two model arms within a selected commit and does not constrain
model-free Stage 0. At process start the evaluator computes the worker ceiling
as `max(1, min(16, available_logical_cpus - 2))`, treating unavailable CPU
count or a count at most two as one worker. Omitted `--jobs` selects that
ceiling; supplied `N` must be an integer in `1..=ceiling` or is a typed refusal.
`--jobs` is a public operational argument only: its value, worker count,
scheduling/completion order, PID, host, and timing are forbidden from every
identity, canonical result, selection record, and artifact-manifest hash.

Each worker owns one digest-named cluster directory and performs that cluster's
`build-1` and `build-2` against distinct initially empty output/cache
directories. No cache or mutable aggregate is shared across clusters. Final
reduction waits for all 300 cluster terminals and sorts exclusively by the
UTF-8 bytes of `commit_cluster_id`; discovery, submission, and completion order
are irrelevant. Acceptance tests run the same fixed corpus with one worker and
with the computed ceiling, including reversed completion order, and require
byte-identical complete output trees and identical selection-manifest hashes.

For canonical repository ID `r`, base Git object ID `b`, and head Git object ID
`h`, define
`commit_cluster_id = D("commit-cluster",
{"cluster_contract":"m20.commit_cluster@1",
"experiment_id":"m20-changed-public-callee-utility-v1",
"repository_id":r,"base_commit_oid":b,"head_commit_oid":h})` under the
frozen evaluator canonicalization. The preimage contains no path, ordinal,
clock, or result. The closed root layout is exactly
`corpus-manifest.v1.json`, `clusters/<64hex>/build-1/`,
`clusters/<64hex>/build-2/`, `stage0-result.v1.json`,
`stage0-selection.v1.json`, and `artifact-manifest.v1.json`; `<64hex>` is the
digest suffix of the corresponding cluster ID. All files are listed by the
artifact manifest. Missing, duplicate, extra, reordered, or foreign clusters,
unlisted files, and writes outside the root are invariant failure.

Wall-clock, per-cluster process CPU time, peak bytes, and worker utilization MAY
be captured in a separate operational diagnostic outside `NEW_OUTPUT_ROOT`.
They are never a gate, canonical Stage 0 record, artifact-manifest member, hash
preimage, or selection input. The measured 183.2 seconds per cluster remains a
pre-seal feasibility observation; parallel wall time does not replace it or
change any denominator.

## 4. Stage 0 exact sets and seven gates

Run frozen ingest, synthesis, universe, planning, and
`context.subject_windows@3` construction on all 300 clusters. Profile
exclusions remain visible as stable `ExclusionRecord`s; zero-obligation commits
remain in the commit denominator.

Let `C` be the exact sorted set of all 300 cluster IDs. For each `c` in `C`,
define `A_c` as the exact sorted set of substantive
`relation.changed_public_callee@1` obligation IDs whose applicability is
exactly `applicable` after the frozen profile's explicit exclusions.
Target-support-`unknown` obligations, D rule-level gap IDs, and exclusion IDs
are published separately but are not in `A_c`. Define `A=union_c A_c`.

For each cluster define `S_c` as the exact subset of `A_c` whose selected
packet admits both the exact caller subject and exact callee subject, each
closing to its required source ID and complete subject span. Define
`S=union_c S_c`. Every ID in `A\S` MUST have a typed `subject_loss` or
`subject_unknown` record naming source and recovery reference. A profile-
excluded candidate is not in `A` and MUST NOT be used as this remainder.

For each cluster define `D_c` as the exact sorted subset of `A_c` whose plan
status is `deferred`, and define `D=union_c D_c`. Validators MUST reconstruct
all sets from IDs and reject duplicates, an ID outside `A`, a count-only
surrogate, or silent loss. The deferred fraction is zero if `A` is empty,
although prevalence then fails.

Two clean builds start from independently empty output/cache directories with
identical inputs. Canonical JSON uses UTF-8, sorted object keys, contract-defined
array ordering, and no insignificant whitespace.

All seven gates MUST pass:

1. **Prevalence:** at least 45/300 `A_c` are nonempty and `|A|>=60`.
2. **Subject retention:** `20*|S| >= 19*|A|`, using the exact sets above.
3. **Bounded context:** over the same applicable packets,
   `median(admitted_source_bytes) <=
   .50*median(whole_changed_production_files_bytes)` and nearest-rank p90
   admitted bytes is at most 65,536. Median uses the midpoint average for even
   counts.
4. **Determinism:** both builds match obligation/universe/source IDs,
   applicability, windows, inventories, losses, and canonical bytes for every
   cluster.
5. **Enumeration honesty:** `direct_calls=partial`, its source-backed
   limitations, exact `enumeration_obstruction_ids`, and the D rule-level gap
   remain visible for every repository snapshot. Complete-call claims are
   forbidden.
6. **Fan-out:** sort all 300 `(c, |A_c|)` pairs by
   `(|A_c|, c)`, retaining zeros. The 1-indexed nearest-rank p95
   count at `ceil(.95*300)=285` MUST be at most 50.
7. **Deferred fraction:** `|D|/|A|<=.05`, evaluated as `20*|D|<=|A|`.

The subject-retention exact fixture is `|A|=60,|S|=57` and passes; its
one-ID-lower fixture `|S|=56` fails. The fan-out exact fixture has sorted index 285 equal to 50 and passes; its `+1`
fixture changes that value to 51 and fails. Deferred exact and `+1` fixtures
are `|A|=60,|D|=3` (pass) and `|A|=60,|D|=4` (fail). Implementations MUST run
these fixtures before Stage 0. No implicit top-50 cap, drop, changed weight, or
post-result filter is allowed.

Any gate failure stops all model execution, is a slice failure, and MUST NOT be
called feasibility success.

The Stage 0 result and selection schemas contain no arm outcome, model/judge
record, primary score, or endpoint cell. If and only if every gate passes,
`stage0-selection.v1.json` seals the exact eligible cluster-ID set, its frozen
hash order, and the first 10/40 cumulative memberships. Every later `run`
launch authenticates that manifest hash and its own membership. Stage 0 values
can authorize or block launch but cannot enter the primary scorer, turn an
ineligible cluster into a zero, or contribute to `n`, `b`, `c`, or `n00`.

## 5. One obligation and one label-independent hash sample

Within a commit, applicable D obligations are ordered by
`SHA256(obligation_sampling_seed || NUL || obligation_id)`, then obligation ID.
Only the first enters a model packet; all others remain in the audit denominator
as `not_sampled_for_model_evaluation`.

A commit is model-eligible when it has a selected obligation and each complete
arm packet retains every mandatory source with `admitted_source_bytes <=
65,536`. This identical source-byte predicate is the only enforced equal input
budget. There is no evaluator-enforced input-token ceiling or tokenizer
preflight. Ineligible commits remain in Stage 0 denominators with typed
exclusions.

All model-eligible commits across the three repositories are ordered once by
`SHA256(commit_sampling_seed || NUL || commit_cluster_id)`, then cluster ID.
Stage 1 uses the first 10 and Stage 2A adds the next 30. Control labels MUST NOT
alter this membership or order.

Both labelers classify every model-eligible commit from the same source-only
rubric in `preregistration.json` before arm outcomes. Agreement
`clean_refactor_control` enters the safety subset; agreement `not_control` does
not. Disagreement or a missing label is `control_status_unresolved`, remains in
the primary hash sample, and is not a control. Every label and rationale is
sealed. Knowledge of the public rank cannot move a commit because labels do
not select the sample.

Stage 1 requires at least two agreed controls among its fixed 10; Stage 2A
requires at least eight among its fixed 40. Insufficiency leaves the primary
cells reportable but blocks advance/success as
`insufficient_control_observations`.

## 6. Executable packet and scoring contract

One in-process pipeline owns repository/object reading, exact Rust excerpt
extraction, content-addressed payload closure, packet/source/loss construction,
paired abstention opportunity, both reviewer invocations and raw-output decode,
text normalization, mechanical scoring, the one judge invocation and decode,
primary conjunction, full-run fixtures, audit artifacts, and attacks. It is
defined exclusively by the hash-frozen evaluator in
[EVALUATOR_SPEC.md](EVALUATOR_SPEC.md). No runner, report, or prose interpreter
may reproduce or override those algorithms.

The production command accepts only an authenticated repository/commit/
obligation launch selector and a new output root. Packet, status, loss,
opportunity, binding, parsed output, hash, candidate, batch, permutation, or
score artifacts are never production inputs. The only non-authority values
entering scoring are raw reviewer and judge invocation results. Audit artifacts
are one-way outputs; offline `verify-run` cannot alter primary cells.

The externally required outcomes are:

- reviewer packets directly expose exact admitted Rust source as strict-UTF-8
  text and expose no evaluator-owned D property lens;
- task-blocking abstention can affect primary cells only when the executable
  evaluator reconstructs equal question/reason opportunity for both arms;
- one commit uses one arm-hidden batch containing exactly two opaque candidates
  and returns one independently closed utility score per candidate; and
- `usable_grounded_disposition_completed` is read only from the generated
  primary-score record.

Until the five registered hashes and manifest path are non-null and verified,
packet construction, model execution, and scoring are forbidden.

## 7. Packet construction and leakage control

Both arms use identical generic instructions, schema, timeout, maximum three
claims, and ceilings. The reviewer model is only
`Qwen3.8-27B-MLX-4bit`; effective effort is the mlx-dspark server default
`low`, and the request MUST omit `reasoning_effort`. An explicit effort field,
an 8-bit or `ornith-*` model, or `xhigh` is a prelaunch refusal. The evidence
and authority ceiling for this pin live only in
[`MODEL_PIN_RATIONALE.md`](MODEL_PIN_RATIONALE.md). Equal ceilings MUST NOT be
called equal realized budget and packets MUST NOT be padded.

Neither packet may contain known oracle/root labels, past judge rationale,
commit subject/body, issue/PR body, subsequent fixes, hidden/regression tests,
control labels, or arm labels. Profile-defined production diffs are constructed
from pinned trees without log text. Profile exclusions and oracle exclusions
are distinct typed inventory records.

Evaluator-owned packet keys/values and instructions additionally MUST NOT
contain the D rule/property literals; rule/property/obligation/relation/
endpoint/caller/callee/subject fields; typed caller/callee/endpoint/subject
roles; or explanations of D selection or subject-first projection. A/B task ID,
instruction, and output schema bytes MUST match. Exact repository source bytes
and paths are not rewritten if user code itself contains such a literal.

The frozen evaluator MUST build both packets and include exact content-addressed
source payloads. Before execution, seal the generated packet, admitted-source
UTF-8 bytes, payload/inventory closure record, and exact serialized packet hash.
The pipeline continues with its original immutable in-memory packet and MUST
NOT read the sealed artifact back.
Leakage canaries from all locally available forbidden artifacts
use normalized exact strings and hashed n-grams; any collision stops the stage.
Unavailable external issue/PR content is `unavailable_to_scan`, never
`confirmed_absent`. These are provenance controls, not an access boundary.

For each arm the single pipeline MUST seal and later disclose:

- closed `budget.json` fields for the pair: each packet hash, exact
  admitted-source UTF-8 byte count, `<=65,536` result, pair eligibility, and
  content-derived audit ID;
- exact serialized packet bytes and total byte count;
- system/instruction, schema, metadata/inventory, admitted-source, and
  serialization-overhead byte counts whose sum equals total bytes;
- provider-reported tokenizer identity and input/output/cache token usage, or a
  typed unavailable reason, in a closed `observation_only` record that is
  never a budget or scoring authority;
- client and provider truncation flags/reports, exact output bytes, and parsed
  claim count.

`verify-run` recomputes `budget.json` from the two sealed packets but does not
recompute observational token data. Both arms exactly at 65,536 bytes pass.
The `+1` fixture changes one arm to 65,537 bytes and requires a pair-wide sealed
`model_ineligible` result with reason
`admitted_source_byte_ceiling_exceeded`, null arm results, zero reviewer/judge
calls, CLI exit 0, and no primary record or membership in `n`, `b`, `c`, or
`n00`. No post-outcome replacement is allowed. Client-side truncation is
forbidden. Detected provider truncation is a post-launch primary failure (`0`).

Equal source-byte ceilings do not imply equal tokens, information, serialized
request bytes, or cost, and m20 makes no equal-token-budget claim. The common
requested maximum-output value is 12,000 for both arms. Its empirical basis is
recorded only in `MODEL_PIN_RATIONALE.md`; backend token reports remain
non-authoritative observations.

## 8. Backend and execution

Immediately before each model stage and every reviewer invocation, reacquire
the backend listing and health documents and compare their canonical hashes
with the two pins in `preregistration.json`. Current reachability is unmeasured;
this preregistration contacted no endpoint. A pre-launch mismatch/unavailable
endpoint stops the stage. A post-launch failure scores `0` without retry.

Each commit receives both arms serially (`concurrency=1`). Arm order is the low
bit of `SHA256(arm_order_seed || NUL || commit_cluster_id)`. Each reviewer call
uses only `Qwen3.8-27B-MLX-4bit`, omits `reasoning_effort` so that the server
default `low` applies, uses 900 seconds, has no evaluator input-token limit,
and requests the common maximum-output value 12,000. Calls use no tools, no
repository mount, and an empty workspace-scoped cwd. Record
provider/model/revision, effort/settings, prompt, packet identity, empty tool
trace, raw/parsed hashes, timestamps, truncation, outcome class, and any backend
tokenizer/usage report as observation only. A backend context-length rejection
after an eligible request is sent is a post-launch arm failure scored `0`
without retry; a reported token count never changes eligibility or that result.

## 9. Primary scoring

The canonical endpoint name is
`usable_grounded_disposition_completed(commit, arm)`. Its value is exactly the
`completed` field of the generated `m20.primary_score.v3` record after frozen
mechanical scoring and two-candidate judge reconciliation. A prose
implementation, report renderer, confidence value, defect projection, or safety
result MUST NOT alter it. Post-launch missing or failed results are zero without
retry.

Publish all four cells and `(b-c)/n` overall, by repository, and for each of the
three leave-one-repository-out subsets. No p-value, alpha, confidence interval,
or significance label is allowed. The optional arithmetic in the decision-
boundary document is labeled only an independence-reference tail and never
affects a decision.

## 10. Judge, safety, stages, and time

After both arm results and bindings are sealed, the frozen evaluator constructs
one batch from their original in-memory values, with two opaque candidates, a
sealed reverse map, and no arm/control identity in judge input. It restores arms
from the in-memory map, never the artifact. The judge is `gpt-5.6-sol`, effort `high`, no tools,
with a 90-second ceiling per commit batch. Batch validation, thresholds, and
both-fail behavior are owned only by the frozen evaluator. A separate defect
projection may be reported as secondary but cannot rescue primary.

For agreed controls, the safety gate fails exactly when
`J_B_clean > J_A_clean + 1`. Because utility judging is primary, a missing
batch sets both affected arm cells to zero without retry. Fewer than 2 controls
at Stage 1 / 8 at Stage 2A blocks advance/success without changing already
scored primary cells. Secondary endpoints cannot rescue the rectangle.

Stage 1 runs 10 fixed hash-ranked pairs. It advances only if all seven Stage 0
gates pass, `b>=8,c<=1`, sensitivity tables are published, backend/leakage/
judge gates pass, control adequacy holds, and safety passes. Its
`cumulative_model_ceiling` is 18,900 seconds (5.25 h) inside a 21,600-second
(6 h) `wall_clock_envelope` with 2,700 seconds reserve.

Stage 2A adds 30 fixed pairs and analyzes all 40. Final success additionally
requires `b>=18,c<=7` and continuing gates. Its `cumulative_model_ceiling` is
75,600 seconds (21 h) inside an 86,400-second (24 h) `wall_clock_envelope` with
10,800 seconds reserve. Stage 2B/72 h is unauthorized. Unused time never
authorizes retries, extra pairs, or threshold changes.

## 11. Seal and reveal order

The mandatory order is:

1. independent single-pipeline evaluator implementation/generated artifacts,
   named attack oracles, portable bundle hash, semantic execution hash, and
   runtime-provenance manifest freeze
   and verification before slice implementation;
2. slice/protocol/profile/context/seed freeze;
3. candidate registry and corpus manifest seal;
4. both deterministic builds, exact denominators, seven gates, and boundary
   fixture results seal;
5. all candidate control labels/rationales and the label-independent model
   sample seal;
6. evaluator-generated payloads/packets, deterministic source-byte budget
   records, observation-only token schema, arm order, and backend gate seal;
7. raw/parsed reviewer results, usage/truncation, per-arm hidden bindings, judge
   permutation, opaque candidates, and reverse-map hash seal;
8. primary judge inputs/outputs, generated scorer records, cells, sensitivities,
   safety counts, and decision seal; then
9. terminal disclosure of manifests, exclusions, labels, inventories, hashes,
   leakage records, results, usage, sensitivities, and calculations.

Steps 6--8 are one-way artifact writes within one `run` process. Their seal
order does not create an intermediate input API, pause/restart seam, or
permission to deserialize an earlier artifact into scoring state.

If Stage 1 advances, public label disclosure waits until Stage 2A terminates.
Hash sealing does not provide secrecy on this machine.

## 12. Required final interpretation

The final report MUST state: open development evaluation; no true access
boundary, organizational blind, or holdout validity; frozen-corpus descriptive
utility only; no significance, population precision/recall, or generalization;
the required utility judge is not defect truth and introduces frozen
judge/model dependence; `direct_calls=partial` prohibits complete-caller
claims; and development-observed repositories remain development evidence.
