# m7-head-local-v1 blind judge protocol

Status: frozen before any generation or judge call. This document is the
complete, verbatim specification of the judge input, the judge prompt, the
blinding mechanism, and the judge output contract. It follows the design
philosophy of ADR 0035 (opaque content-addressed case identifiers, arm and
generator identity withheld, order not correlated with provenance) but is a
new, independently frozen protocol — it does not reuse ADR 0035's calibration
gate or its known-defect pairs, because m7-head-local-v1 has no ground truth.

## 1. Why Claude judges qwen's findings

The generator (`qwen3.8:27b-mlx` via LM Studio) and the judge (Claude, via
the isolated `reviewgraphen-benchmark run-process-reviewer claude` backend)
are different model families. This avoids same-family correlated error
between generation and judgment, per ADR 0035's reasoning. The judge's label
is a **non-authority model disposition**, not a verified defect. No upstream
issue, branch, commit, patch, or pull request is created from any judge
output, regardless of its content.

## 2. Blinding mechanism

For each of the 9 units (§ preregistration.json `units`), after both arms
(`qwen_b1`, `qwen_full`) complete generation (or fail — see § completion
tracking in `preregistration.json`), their `candidate.json` `findings`
arrays are pooled:

1. For every finding in either arm's `findings` array, compute
   `finding_id = "finding:" + sha256(canonical_json({
     "unit_id": <unit id>,
     "locations": <finding.locations>,
     "mechanism_tags": <finding.mechanism_tags>,
     "rationale": <finding.rationale>,
     "severity": <finding.severity>
   }))[:32]`, where `canonical_json` is the same deterministic
   sort-keys/no-whitespace serialization already used elsewhere in this
   repository (`reviewgraphen_core::canonical_json`). The preimage
   deliberately excludes `local_id`, arm, generator, and any per-response
   ordinal, so the ID is a pure function of finding content and cannot
   encode provenance. If both arms independently report byte-identical
   `locations`/`mechanism_tags`/`rationale`/`severity`, they collapse to one
   `finding_id` and are judged once; this is recorded as a cross-arm
   duplicate in the truth file (§3), not treated as an error.
2. Pool all resulting `(finding_id, locations, mechanism_tags, rationale,
   severity)` records for the unit, across both arms, and sort them by
   `finding_id` ascending (a hash-sort, uncorrelated with arm or generation
   order — the same principle as ADR 0035's `content_hash_sorted`
   requirement).
3. Write `findings.json` (the judge-visible pool — see §4) and, separately,
   a `truth.json` file that maps each `finding_id` back to
   `{arm, unit_id, original_local_id}` for every contributing arm. `truth.json`
   is written to a directory never mounted into the judge's sandbox and is
   opened only after the judge's output for that unit is returned and
   validated.
4. If the pool for a unit is empty (both arms abstained, failed before
   final content, or produced zero findings), no judge call is issued for
   that unit; this is recorded as `judge_call: not_issued, reason:
   empty_pool` rather than treated as a missing observation.

### Structural blinding guarantees (not just prompt instructions)

- The judge process runs via `reviewgraphen-benchmark run-process-reviewer
  claude` with `--no-session-persistence` (no memory of any other call,
  including other units or arms in this same experiment) and `--tools ""`
  (no filesystem, shell, or network access beyond the exact admitted input
  files piped to it). It cannot discover arm identity by exploring anything
  outside what is deliberately included in its input packet.
- The judge's input packet (§4) contains no arm name, generator name, model
  name, trial ID, run label, snapshot ID, or ReviewGraphen-scaffold-specific
  file path. Before a packet is sent, a mechanical scan checks every file in
  it for the literal substrings `b1_free_form`, `full_reviewgraphen`,
  `qwen`, `codex`, `lm-studio`, `local_id`, and the unit's own internal
  packet index string; a match aborts packet construction rather than
  silently redacting, matching the "forbidden marker" pattern in
  `m7-head-issue-v1/scripts/prepare_judge_calibration.py`.
- Findings from both arms are interleaved by hash-sort, not grouped by arm,
  so even relative position within `findings.json` carries no arm signal.

## 3. Truth file (kept apart from the judge)

`truth.json` per unit:
```
{
  "schema": "reviewgraphen.benchmark.head_local_judge_truth.v1",
  "unit_id": "<unit id>",
  "entries": [
    {"finding_id": "finding:...", "contributing_arms": ["qwen_b1"]},
    {"finding_id": "finding:...", "contributing_arms": ["qwen_full"]},
    {"finding_id": "finding:...", "contributing_arms": ["qwen_b1", "qwen_full"]}
  ]
}
```
`contributing_arms` has more than one entry only in the byte-identical
collapse case described in §2.1.

## 4. Judge input packet layout

Every file under the judge's `input_root` is concatenated verbatim into the
materialized prompt by `reviewgraphen-benchmark`'s existing, unmodified
`materialize_prompt` (alphabetical by path). The packet contains exactly:

- `00-instructions.md` — the fixed judge prompt, §5, byte-identical across
  all 9 units.
- `01-findings.json` — the hash-sorted pooled findings for this unit (§2),
  with this exact shape:
  ```
  {
    "schema": "reviewgraphen.benchmark.head_local_judge_input.v1",
    "unit_id": "<unit id>",
    "findings": [
      {
        "finding_id": "finding:...",
        "locations": [{"path": "...", "start_line": N, "end_line": N}],
        "mechanism_tags": ["..."],
        "severity": "critical|high|medium|low|null",
        "rationale": "..."
      }
    ]
  }
  ```
- `sources/<path>` — every source file admitted into this unit's packet
  (the same file set for both arms; see `preregistration.json` `units`),
  at the exact repo-relative path under `sources/`.

No other file is included. In particular, the qwen `candidate.json`,
`process-record.json`, or any raw response artifact is never placed in the
judge's input root.

## 5. Judge prompt (frozen, verbatim)

The content of `00-instructions.md`, byte-for-byte:

```
You are reviewing a set of candidate defect findings against a fixed
snapshot of Rust production source code from FSL, an open-source formal
specification language and verification toolchain. The source files are
under sources/. The findings are in 01-findings.json, one entry per
finding_id, each with claimed locations (file path and line range),
mechanism tags, an optional severity, and a rationale written by the
finding's (unknown to you) originating process.

You do not know how many systems or processes produced these findings, and
you must not guess or state anything about their origin, count, or
identity. Judge each finding only on its own merits against the actual
source code in sources/.

Your judgment is a non-authoritative disposition. It will never itself
create, close, or modify an issue, branch, commit, patch, or pull request
in any repository, upstream or otherwise. No human or system treats your
output as proof that a defect exists. Your job is to estimate, as
carefully and skeptically as you can from reading the actual code, whether
a competent maintainer would want a tracked issue opened for each finding.

For every finding_id in 01-findings.json, in the schema-mandated output,
provide exactly one judgment with:

1. disposition — exactly one of:
   - "issue_should_be_created": you read the referenced code, the
     described behavior is plausible or confirmed from the source as
     written, and a competent maintainer would reasonably want this
     tracked, even if you are not 100% certain it is a true defect.
   - "should_not_be_created": you read the referenced code and concluded
     the finding is very likely a false positive, describes intended
     behavior, is not actionable, or is not supported by what the code
     actually does.
   - "unable_to_determine": you read the referenced code but the
     information available (finding text, code context, or both) is
     genuinely insufficient to decide either way. Use this when you are
     stuck, not as a default for findings you merely find uninteresting.

2. quality.specificity — exactly one of:
   - "file_and_line_identified_and_relevant": the locations point at real,
     existing lines in sources/ that are actually relevant to the claim.
   - "file_and_line_identified_but_questionable": locations resolve to
     real lines, but they seem loosely or wrongly connected to the claim.
   - "file_only_or_vague": a file is identified but line ranges are
     missing, absurdly broad, or clearly wrong (e.g. out of file bounds).
   - "absent": no usable location information.

3. quality.reproduction_conditions_stated — true only if the rationale
   states a concrete triggering condition, input, or sequence of calls
   under which the claimed problem manifests, not just an abstract
   description of a risk.

4. quality.false_positive_suspected — true if, after reading the code, you
   suspect the finding misreads or misunderstands what the code actually
   does (independent of your disposition above; you can suspect a false
   positive and still choose issue_should_be_created if you are not
   certain enough to rule the finding out).

5. quality.design_intent_confusion_suspected — true if the finding appears
   to flag behavior that the surrounding code, comments, or naming make
   clear is intentional (e.g. a documented invariant, an explicit
   assertion, a deliberately partial implementation marked as such).

6. quality.notes — one or two sentences (max 2048 characters) giving your
   concrete reasoning, citing the specific code you checked. Do not
   speculate about who or what produced the finding.

Read every file under sources/ that a finding references before judging
it. If a finding references a path not present under sources/, or a line
range outside the file's actual length, treat that as strong evidence
toward should_not_be_created or unable_to_determine, and say so in notes.

Return only the single JSON object required by the output schema. Do not
include Markdown formatting, commentary, or any text outside that JSON
object.
```

## 6. Judge output validation

The judge's raw output is validated against
`schemas/reviewgraphen.benchmark.head_local_judge_output.v1.schema.json`
using the same unchanged, no-repair, no-normalization discipline as ADR
0037: the judge backend is invoked with `--json-schema` (provider-constrained
mode), so the Claude CLI itself enforces schema conformance before this
process ever sees the output. If the call nonetheless fails
provider-constrained validation or exits non-zero, that unit's judge output
is recorded as `judge_call_failed` and its findings are reported as
unjudged — they are not silently dropped from the finding count, only from
the disposition tally. After validation, `judgments[].finding_id` must be
exactly the set of `finding_id`s sent in `01-findings.json` for that unit
(no fewer, no extra, no duplicates); a mismatch is also recorded as
`judge_call_failed` rather than partially accepted.

## 7. Untested execution path — required smoke test before production

`reviewgraphen-benchmark run-process-reviewer claude` (the generic,
non-Codex-profile isolated backend) has never been exercised end-to-end in
this repository: `m7-head-issue-v1` was interrupted before its calibration
runner issued any model call (see `benchmarks/m7-head-issue-v1/INTERRUPTED.md`).
Before the real 9-unit run, one minimal smoke test must confirm, from
retained raw output:

1. the Claude CLI backend executes inside the bwrap sandbox and returns
   within a bounded time on a trivial one-file, one-finding input;
2. `--json-schema` provider-constrained validation actually rejects a
   deliberately invalid probe output and accepts a valid one; and
3. `--no-session-persistence` and `--tools ""` are confirmed in the
   constructed command line (already read directly from
   `crates/reviewgraphen-reviewer/src/process.rs`, but not yet observed in
   a live invocation).

The smoke test uses synthetic content, not any of the 9 real units, and its
result is recorded before the first real judge call, following the same
minimal-probe-before-real-run pattern already used for the LM Studio
`reasoning_effort=none` transition in `m7-local-factorial-v3`.
