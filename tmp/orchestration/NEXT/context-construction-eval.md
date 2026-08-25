# 次の検証（ユーザー指示 2026-08-25）: 作業用コンテキスト構築の効率と精度

問い: Qwen 単独 vs Qwen + custom ReviewGraphen で、プログラミングタスク用の
**効率的で高精度な作業用コンテキストを探索・構築**できるか。

前提: 現在の検証（5 回目 seal → Stage 0 → pilot 読み出し）完了後に実行。
それまでは設計（sol/high）のみ進め、モデル実行は開始しない。

---

## m21 preregistration draft: task-context construction v1

### 0. Status, separation, and authority

- Study ID: `m21-task-context-construction-v1`.
- Status: **design-only / execution prohibited** until the current m20
  verification, seal, Stage 0, and authorized pilot readout are terminal.
- This is a new benchmark. It does not reuse m20 units, packets, evaluator
  state, freeze history, gates, or results and does not amend or re-seal m20.
- Model is fixed for both model arms to `Qwen3.8-27B-MLX-4bit`; switching model,
  quantization, revision, system prompt, tokenizer, or backend between arms is
  forbidden.
- The result is a descriptive comparison on a frozen retrospective corpus. It
  is not defect truth, implementation correctness, Evidence, Verification,
  human acceptance, or a claim of general software-engineering utility.
- The oracle defined below is a **realized-fix context oracle**: it describes
  context touched or referenced by one historical fix. It is not a proof that
  every oracle line was logically necessary or that no alternative fix could
  need different context.
- No model call, repository selection, pilot, or outcome may occur until all
  `TBD-before-freeze` identities below are non-null, schemas and mutations pass,
  and an independent custodian seals the preregistration and evaluator hashes.

### 1. Frozen question and unit

Question: on a repository snapshot immediately before a real change, does the
same Qwen agent, when initialized with a custom ReviewGraphen task-subject
packet, return a working-context set that has higher retrospective oracle
precision/recall and lower exploration-token cost per covered oracle line than
Qwen with only minimal read-only repository tools?

The analysis unit is one immutable task tuple:

```text
u = (repository_id, base_commit_oid, realized_fix_commit_oid,
     task_kind, task_brief_id, base_profile_id, oracle_id)
```

All arms see only the base tree and the same task brief. The realized fix
commit, its diff, commit metadata other than the frozen brief, oracle records,
and later history are mounted only in the evaluator/oracle process and are
unavailable to the model and repository tools.

Two task strata are frozen:

1. `symbol_change`: “change function/type X to satisfy requirement Y.” `X` is
   one exact base-snapshot symbol supplied in the brief.
2. `symptom_fix`: “fix bug Y.” The brief supplies a normalized historical
   commit title and, when mechanically available in the base tree, one failing
   test/error-string identifier, but no oracle path or changed-symbol list.

The normalized title is UTF-8 NFKC, whitespace-collapsed, has OIDs and absolute
or repository-relative path lexemes removed, and is sealed before either arm.
Tasks whose remaining title is empty or contains an exact oracle-only path are
excluded with a typed reason. A task brief is task input, not oracle evidence.

### 2. Required output contract

Each arm returns exactly one closed `m21.context_set.v1` value:

```json
{
  "task_id": "...",
  "snapshot_id": "...",
  "context_items": [
    {
      "path": "normalized/repository/path.rs",
      "symbol_id": "nullable base symbol ID",
      "start_line": 1,
      "end_line": 20,
      "reason": "subject|caller|callee|reference|type|test|configuration|other"
    }
  ],
  "coverage_claim": "complete|partial|unknown",
  "declared_losses": [
    {
      "scope": "symbol-or-path/range expression",
      "reason": "budget|unresolved_symbol|ambiguous_binding|missing_source|tool_failure|other",
      "attempt_source_ids": ["..."]
    }
  ]
}
```

Items are normalized against the base tree, sorted by
`(path,start_line,end_line,symbol_id)`, deduplicated, and range-checked. Overlap
is evaluated as a union of line atoms; duplicates cannot improve a score.
Foreign paths, future-only lines, malformed ranges, an unavailable base blob,
or no valid final object are a typed task failure with empty selected context.
Raw prose or chain-of-thought is never a context item or canonical state.

### 3. Independent realized-fix oracle

#### 3.1 Frozen extractor boundary

The oracle builder is deterministic, model-free, authored separately from the
two arm adapters, and operates only after units are selected. Before freeze it
pins exact parser, symbol resolver, diff algorithm, file-profile, and source
hashes (`TBD-before-freeze`). It must pass reorder, whitespace, rename,
ambiguous-resolution, macro, generated-source, new-file, and missing-span
mutations. A judge, Qwen output, ReviewGraphen packet, reviewer rationale, issue
label, or current arm result is never an oracle input.

The admitted universe is every Rust source category explicitly included by
`m21.rust.task_context.v1`. Production, test, build/configuration, generated,
vendored, and documentation categories are separately enumerated. No category
is silently excluded: an excluded category remains in the oracle-exclusion
ledger with exact file IDs/count/digest.

#### 3.2 Exact derivation

Let `B` be the base tree, `H` the realized-fix tree, and `Δ(B,H)` the frozen
zero-context, rename-aware line diff. Parse both trees and match symbols by the
frozen stable key `(language, normalized path after accepted rename,
fully-qualified owner/name, kind)`.

```text
T_old = { s in Symbols(B) |
          span_B(s) intersects an old-side changed range }

T_match = { s_B in Symbols(B) |
            stable_key(s_B)=stable_key(s_H) and
            span_H(s_H) intersects a new-side changed range }

R = { resolve_B(x) |
      x is a non-local symbol reference in an added/modified H-side hunk,
      x has exactly one definition in B under the frozen resolver }

O_symbols = sort_unique(T_old union T_match union R)
O_ranges  = coalesce({ full declaration span_B(s) | s in O_symbols })
O_lines   = { (path,line) | line is in a range in O_ranges }
```

Whitespace/comment-only hunks do not create `T_old`/`T_match`; references in
comments, strings, commit text, generated expansion, or unresolved macro output
do not enter `R`. They remain typed oracle limitations. A candidate is eligible
only if every non-local code reference in each admitted changed hunk is either
uniquely resolved to `B` or falls into an explicitly frozen ignorable syntax
class. Added-only files, deleted-only files without a base symbol, ambiguous
method/trait dispatch, unresolved imports, and unknown macro cardinality are
excluded rather than silently counted as known context.

For `symbol_change`, let `S0_lines` be the full base span of the subject already
given in the task brief. The exploration denominator is
`O_eval = O_lines - S0_lines`; the supplied subject cannot create free recall.
For `symptom_fix`, `S0_lines` is empty. Units with empty `O_eval` are excluded.
The oracle record contains snapshot/profile/extractor IDs, exact sorted symbol
IDs, coalesced ranges, `|O_lines|`, `|O_eval|`, both sorted-line-set SHA-256
commitments, limitations, and source IDs. Checked-in fixtures, not current arm
output, fix expected oracle IDs.

### 4. Custom ReviewGraphen construction

Arm B/C uses a benchmark-local, separately versioned policy
`m21.task_subject_windows@1`. “Equivalent to
`context.subject_windows@3`” means subject-first bounded materialization,
deterministic ordering, source IDs, exact denominator commitments, and typed
loss/unknown records; it does not reuse the D rule, m20 packet, or pretend the
two-endpoint D DTO accepts arbitrary task subjects.

The custom work is exactly:

1. **Task-to-subject binding.** For `symbol_change`, validate the supplied base
   symbol ID and source span. For `symptom_fix`, deterministically bind task
   identifiers/error strings to base symbols/tests using exact-name first,
   then normalized lexical matches. Zero/multiple bindings remain typed
   `unresolved`/`ambiguous`; no model resolves them.
2. **Incremental ingest.** Build the first base ProgramSpace from immutable Git
   objects, then update adjacent first-parent snapshots by content hash. Every
   reused and recomputed fact records snapshot, extractor version, source ID,
   and cache-key hash. Future-fix objects are inaccessible. A clean rebuild must
   produce identical accepted facts and packets.
3. **Non-D rule pack.** `task.subject@1`, `task.call_neighborhood@1`,
   `task.reference_neighborhood@1`, `task.type_owner@1`, and
   `task.related_test@1` expand only accepted contains/calls/imports/reference/
   covers facts from task subjects. Partial method, dynamic, cross-crate, and
   macro resolution remains declared unknown and cannot imply global coverage.
4. **Projection.** Reserve all resolved task subjects, then rank accepted
   callers/callees, referenced definitions/types/owners, and related tests.
   Materialize only subject/reached files. Emit exact included and lost
   identities/counts/digests, source ranges, and reason-partitioned losses.
5. **Packet.** Provide the task brief, task-subject bindings, bounded source
   windows, stable source IDs, and typed losses—never oracle identities, fix
   diff, future tree, arm label, expected paths, or evaluator rationale.

### 5. Arms and equal budgets

Arm order per unit is the low bit of
`SHA256("m21-arm-order-v1" || NUL || task_id)` and is sealed before execution.
Each arm begins with empty conversation/tool state and the same read-only base
tree. No shell, Git log, network, compiler, test execution, arbitrary command,
or access outside the snapshot is allowed.

#### A — Qwen only

Qwen receives the task brief and only three bounded tools:
`list_paths(prefix)`, `search_text_or_identifier(query)`, and
`read_range(path,start,end)`. Tool results contain base source only. The model
must explore and return `m21.context_set.v1`.

#### B — Qwen + custom ReviewGraphen

Qwen receives the same task brief, tool surface, and limits plus the sealed
`m21.task_subject_windows@1` packet. Packet serialization counts against model
input tokens and packet construction/ingest counts against wall time. Qwen may
discard, retain, or augment packet ranges, but its final context set—not packet
size alone—is scored.

#### C — packet only (descriptive lower bound)

No model call. The deterministic packet's admitted ranges are converted
directly to the same context-set schema and its typed losses are preserved.
Arm C is descriptive and cannot decide A-vs-B success.

#### Equal resource contract for A and B

- model/revision/quantization/backend: exact same non-null frozen identities;
- temperature `0`, no retries, one terminal agent trajectory;
- cumulative serialized model-input ceiling: **65,536 tokens**;
- cumulative raw model-output ceiling: **24,000 tokens**;
- wall-clock ceiling: **1,800 seconds** from task handoff to valid terminal
  context; B includes deterministic ingest/projection time;
- at most 32 repository-tool calls; each response at most 16,384 UTF-8 bytes;
- same prompt/schema/tool descriptions and output parser except for B's packet;
- no hidden baseline-only or treatment-only repository read.

Before freeze, the exact Qwen tokenizer files, tokenizer revision, chat
template, special-token rules, and their SHA-256 values must be vendored or
otherwise immutable and non-null. Token cost is recomputed client-side over
the exact serialized request for every call and every raw response; repeated
conversation/tool/packet bytes are counted each time transmitted. Backend
usage reports are observation-only. A ceiling excess terminates that arm as a
typed budget failure; unused budget is not transferred.

The 24,000 output ceiling replaces the rejected 12,000 pilot pin: the pilot
showed that Qwen spends output-body tokens on reasoning before its structured
answer. All raw output tokens still count, but prose is neither oracle nor
score. The parser accepts only the final closed tagged object; truncation or no
valid object is failure. No judge supplies truth or rescues a malformed output.

### 6. Sample frame and mechanical selection

Repositories are exactly `reviewgraphen`, `fsl`, and `casegraphen`, each bound
to canonical repository ID, immutable origin, and a pinned HEAD selected only
after current m20 work terminates and before any m21 arm result. For each repo,
enumerate at most the first 500 commits on the pinned first-parent history.

Eligibility is mechanical and outcome-blind:

- exactly one parent; base and fix objects locally present;
- at least one admitted Rust hunk and no path escaping the repository;
- 1–8 changed admitted files and 5–300 non-whitespace changed lines;
- no added/deleted Rust file or unresolved rename in the admitted diff;
- nonempty, fully derivable `O_eval` with 2–40 oracle symbols and 20–2,000
  oracle line atoms;
- task brief passes the leakage/normalization rules;
- oracle extractor reports no unresolved non-local reference outside its frozen
  ignorable syntax set.

Candidate reasons, exclusions, counts, and sorted candidate-ID-set digests are
sealed before task sampling. A normalized title whose first ASCII-casefolded
token is exactly one of `fix`, `fixes`, `fixed`, `bugfix`, `repair`, `correct`,
`prevent`, or `handle` enters the `symptom_fix` stratum. All other eligible
commits enter `symbol_change`; one fix commit can create at most one unit. For a
`symbol_change` unit, choose `X` from base-resident function/method/type symbols
in `T_old union T_match` by
`SHA256("m21-task-subject-v1" || NUL || task_candidate_id || NUL || symbol_id)`,
then symbol ID; absence of such a symbol is a typed exclusion. A
`symptom_fix` test/error identifier is included only when exactly one identifier
in the normalized title resolves to exactly one base test or string-bearing
symbol; otherwise the brief contains the title alone.

Within each repository and stratum, order by
`SHA256("m21-task-sample-v1" || NUL || repository_id || NUL || base_oid ||
NUL || fix_oid || NUL || task_kind)`, then task ID. Select exactly 10
`symbol_change` and 10 `symptom_fix` tasks per repository: **60 paired tasks**.
If any repository/stratum has fewer than 10, m21 is corpus-infeasible; there is
no substitution, threshold relaxation, or cross-repository borrowing.

Sixty units balance the three known repositories and two task forms while
keeping 120 model trajectories feasible under the 30-minute ceiling. The count
is not a power claim. Results are descriptive paired distributions only; there
is no confirmatory p-value, alpha, significance label, confidence interval, or
population-generalization claim. Any pre-freeze engineering pilot uses units
outside these 60 and is never pooled with them.

### 7. Primary and secondary metrics

For an arm output, normalize its selected lines and remove supplied subject
lines for the exploration score:

```text
C_lines = union of valid (path,line) atoms in context_items
C_eval  = C_lines - S0_lines
I       = C_eval intersection O_eval

precision = |I| / |C_eval|                    if |C_eval|>0 else 0
recall    = |I| / |O_eval|                    (O_eval is nonempty by design)
F1        = 2*precision*recall/(precision+recall)
            if precision+recall>0 else 0

exploration_tokens = sum(exact input tokens + exact raw output tokens)
tokens_per_oracle_line_covered = exploration_tokens / |I|
                                 if |I|>0 else +infinity
wall_seconds_per_oracle_line_covered = elapsed_seconds / |I|
                                       if |I|>0 else +infinity
```

The frozen primary report is the paired A/B task table and, overall and by
repository/task stratum, median/IQR and win/tie/loss counts for:

1. `F1` (context accuracy);
2. `tokens_per_oracle_line_covered` (primary efficiency);
3. elapsed wall seconds and wall seconds per covered oracle line.

No scalar composite hides the accuracy/cost tradeoff. “More efficient” means
strictly fewer exploration tokens per covered oracle line, reported alongside
recall and F1; zero coverage is infinite cost, not omitted. Packet-construction
CPU/wall time is included in B wall time but deterministic code consumes no
model tokens.

Secondary outputs are file-level and symbol-level precision/recall/F1, raw
selected/oracle line counts, input/output tokens separately, tool-call count,
packet bytes/tokens, ingest/projection time, budget/parse failures, and Arm C's
same metrics.

Loss honesty is separate from accuracy. Report whether `coverage_claim` is
`partial|unknown`, the exact declared-loss count/reasons/source IDs, and
`declared_miss_recall = |(O_eval-C_eval) atoms within a declared loss scope| /
|O_eval-C_eval|` when the denominator is nonzero. A complete claim with missed
oracle lines is a false-completeness observation; a declared loss does not turn
a miss into a hit or improve F1.

### 8. Determinism, blinding, and stopping rules

- Oracle, sample order, task briefs, arm order, schemas, tokenizer, budgets,
  prompts, tool implementation, ReviewGraphen policy/rules, and analysis script
  freeze before the first included model call.
- Reordered ProgramSpace facts, candidate enumeration, tool-result maps, and
  context items must reproduce identical IDs/bytes after required sorting.
- Clean and incremental ingest of each base snapshot must produce identical
  accepted facts, subject bindings, packet sources/windows/losses, and hashes.
- The model process receives a detached base-tree view with no `.git`, fix OID,
  oracle, later commit, issue resolution, arm label, or other arm's output.
- Evaluator/oracle code never calls a model or judge. Agent adapters never read
  target/future objects. The same raw-output parser handles A and B.
- A task failure, timeout, token overflow, malformed output, missing source, or
  typed ReviewGraphen loss remains in the 60-task denominator. No retry,
  replacement, post-result exclusion, or threshold change is permitted.
- If model identity, tokenizer pin, base/fix object, frozen source hash, or
  evaluator hash mismatches, execution stops before that task and the study is
  infrastructure-incomplete rather than assigning an accuracy zero.
- Results may be inspected only after all arm outputs and canonical cost records
  for all 60 units are sealed. Pilot observations (12,000 output insufficiency,
  reasoning in response body, and judge non-truth) justify this design but are
  not m21 observations or favorable evidence for either arm.

### 9. Required pre-execution artifacts

Execution remains forbidden until a later implementation phase supplies and
freezes, under `benchmarks/m21-task-context-construction-v1/` or another
explicitly approved root:

1. preregistration JSON mirroring this document and a freeze manifest;
2. repository/candidate/exclusion/task manifests with immutable OIDs;
3. oracle extractor, schemas, reference fixtures, and mutation tests;
4. A/B agent adapter and identical tool/budget enforcement;
5. custom task-subject ingest/rule/projection/packet implementation;
6. tokenizer/chat-template files and exact hashes;
7. raw-output decoder and context-range normalizer;
8. metric/reduction script with zero-coverage infinity fixtures;
9. leakage tests proving neither arm can reach fix/oracle/history;
10. determinism tests for clean/incremental packets and reordered inputs.

No item above authorizes execution while the current m20 validation is active.
