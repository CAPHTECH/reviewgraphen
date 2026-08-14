# ADR 0030: Generic review orchestration over isolated process reviewers

Status: Accepted

## Context

ADR 0029 removes the fixed-fixture product command. ReviewGraphen already has
deterministic Git ingestion, obligation synthesis, planning, source-bound
context construction, and the ADR 0027 Codex/Claude process boundary, but no
product operation composes them for an ordinary repository revision.

The existing V5 report is authority-bearing: its evidence, verification,
decision, freshness, and Store provenance are not supplied by a live model.
Emitting a V5 report from process output would launder a proposal into accepted
state. Conversely, merely returning raw model prose would discard the
obligation denominator, context provenance, parse result, and replay boundary.

## Decision

### Command and request

The sole successful review surface is:

```text
reviewgraphen review --request <request.json> --artifacts <fresh-absolute-dir>
```

The request is `reviewgraphen.generic_review_request.v1`. It names an ordinary
local Git repository, stable repository identity, immutable base and target
revisions, bounded ingest and planner settings, and exactly one reviewer mode:

- `codex_cli` with an absolute executable, model, reasoning effort, absolute
  bwrap executable, and absolute credential home;
- `claude_cli` with the corresponding model and effort tuple;
- `codex_app_server`, retained as the swappable ADR 0027 boundary and refused
  until its protocol adapter is implemented; or
- `replay`, with one absolute process-record path per scheduled obligation.

Reviewer executables and credential paths are trusted host orchestration inputs
and are never mounted as reviewed source. The artifact directory must not
exist. It stores only generated reviewer packets, process outputs, and canonical
process records. It is not a Store or an authority root.

All `review --fixture ...` forms remain rejected by ADR 0029. No repository
identity, environment variable, alias, or hidden option selects a fixture path.

### Deterministic stages

The runtime performs, in order:

1. source-retaining ingestion of the exact target Git tree;
2. `MvpRulePack` synthesis of the complete obligation universe;
3. deterministic bounded planning;
4. registration of every retained snapshot source in an in-memory EventLog;
5. Core `prepare_context` construction for every scheduled obligation; and
6. construction of one exact reviewer packet per scheduled obligation.

The in-memory EventLog is used to enforce source closure and event admission;
it is not represented as durable Store authority. Every packet contains only:

- a fixed versioned instruction;
- the exact obligation;
- the canonical Core context envelope;
- a source index naming envelope source IDs and excerpt ranges;
- only the excerpt bytes admitted by that envelope; and
- a generated closed output schema that fixes the execution ID, property ID,
  and allowed target/source IDs.

Packet paths containing `oracle`, `private`, `ground_truth`, commit-message, or
issue-body markers, symlinks, extra files, and hash drift are rejected by the
ADR 0027 input inventory. Source prose is untrusted data and cannot request
tools. The Codex invocation mechanically disables shell/unified execution,
apps, browser/computer use, image, multi-agent, plugins, skills, view-image,
workspace dependencies, and web search; it also uses strict config parsing.
Its JSONL event stream is allow-listed to lifecycle, reasoning, and final
message events, so any tool or unknown event refuses the run. Claude receives
an explicit empty tool set and strict-empty MCP config. bwrap
mounts the admitted packet read-only at `/workspace/input`, a
fresh output directory at `/workspace/output`, the chosen backend, credentials,
and fixed runtime roots only. No repository or Store root is mounted.

### Model observation, parsing, and replay

Each scheduled obligation receives a deterministic execution ID derived from
the run, plan wave, obligation, envelope, and attempt. Codex CLI and Claude CLI
produce `reviewgraphen.process_reviewer_record.v1`. The raw response remains
byte-for-byte in that record. Parsing uses the existing closed
`reviewgraphen.reviewer_output.v1` wire contract and caller-fixed execution,
property, target, and source closure. Results are structured proposals,
abstention, malformed output, or provider failure. Parse failure is recorded;
it is not repaired from prose.

Replay rebuilds ingestion, synthesis, plan, contexts, and packets. For each
execution it validates the supplied process record and requires its exact input
inventory to equal the rebuilt packet. It then parses the recorded raw bytes;
no model process is invoked. The public run record omits live/replay mode,
executable paths, credential paths, artifact paths, and request-file paths.
Given unchanged Git objects and process records, live output and replay output
therefore have identical canonical bytes.

### Output and authority ceiling

The command emits canonical `reviewgraphen.generic_review_run.v1`. It contains
the ProgramSpace/extraction result, complete universe and obligations, plan,
context envelopes with source/loss declarations, process records, parsed
non-authority proposals/outcomes, closed coverage counts, and typed incomplete
reasons.

The record contains no Evidence, EvidenceBinding, Verification, Decision,
Finding, accepted claim, V5 report, or gate. Its authority object is fixed to:

```json
{"classification":"non_authority","trusted_pass":false,
 "result_status":"incomplete"}
```

Neither structured output, model confidence, process exit success, complete
model coverage, nor replay changes that ceiling. Promotion requires a future
separate Core/Store operation with the existing evidence and human-authority
admissions; generic orchestration exposes no such operation.

## Consequences

- ReviewGraphen can review arbitrary admitted Git revisions through Codex CLI
  or Claude CLI without adding an HTTP, LLM SDK, or async-runtime dependency.
- A successful generic run demonstrates ingestion through proposed findings,
  not verified or accepted bug detection.
- Deferred obligations remain in the denominator and force an explicit
  incomplete reason. Counts are derived from the universe, plan, and outcomes,
  never accepted from model output.
- Live nondeterminism is preserved as raw observation. Determinism is tested at
  the replay boundary, where identical records must yield identical run bytes.
- App Server remains substitutable at the backend type boundary but is reported
  unsupported rather than silently falling back to another backend.
