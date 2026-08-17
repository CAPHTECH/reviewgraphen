# qwen_skill arm amendment

Status: frozen before any qwen_skill generation call. Operator-directed
pivot after `CAPABILITY_GAP_DIAGNOSIS.md` and
`docs/measurement-validity-obligation-synthesis-capability-gap.md`
established that the ReviewGraphen implementation's obligation synthesis
is capability-gated shut for all real ingestion. Amends
`preregistration.json`; does not remove or rewrite anything already
frozen there.

## The measurement target changes — stated explicitly, not left implicit

Everything before this amendment measured **ReviewGraphen the
implementation**: the `full` arm presented obligations the running code
actually synthesized. Those obligations are, as established, always
`reviewgraphen.capability_gap` for real input — so the `full` arm measured
a broken analyzer layer, not a review capability.

This amendment introduces a third arm, `qwen_skill`, that measures
**ReviewGraphen the methodology**: the model is given a description of
the ReviewGraphen review process, derived strictly from this repository's
own design documents
(`.claude/skills/reviewgraphen-methodology/SKILL.md`, frozen at commit
`a092c32`, body SHA-256
`a43d6984f9b58d5e47256afed68cddc1598ae84d6fc564e05f1cf462228d670a`), and
performs the obligation-enumeration-through-claim pipeline itself.

These are different things and must never be conflated in reporting:

| Arm | What it measures | Obligations come from |
| --- | --- | --- |
| `qwen_full` | ReviewGraphen the implementation | `MvpRulePack::synthesize` (confirmed capability_gap-only) |
| `qwen_skill` | ReviewGraphen the methodology | the model itself, following the frozen skill |

A `qwen_skill` result says nothing about whether the ReviewGraphen
codebase's obligation synthesis works — that question is already answered
(no) by the diagnosis above. It also does not retroactively make the
`qwen_full` arm's historical results (in this or any prior experiment)
mean something they don't; those stand as recorded, with the addenda
already appended.

## Arm definition

- **Name:** `qwen_skill`.
- **Scaffold:** the frozen skill body (everything in `SKILL.md` after its
  closing `---` frontmatter delimiter), inserted verbatim as the packet's
  primary instruction file. No paraphrase, no summary, no addition.
- **Source:** the same raw production source files as the `qwen_b1` arm
  for each unit (`units.json`), with no ReviewGraphen-synthesized
  obligations, mechanism ontology, or context envelopes — the model
  builds its own obligation set per the skill's Stage 2, from the source
  it is given, exactly as the skill instructs.
- **Units:** the same 9 units already frozen in `units.json`
  (`head-local-00` through `head-local-08`), unchanged, so results remain
  comparable to `qwen_b1`'s and `qwen_full`'s observations on the same
  content.
- **Packet layout:** `agent_input/{instruction.txt, candidate-output.schema.json,
  source/<path>...}` — instruction.txt is the frozen skill body; this is
  structurally the same shape as the `qwen_b1` packet (`base_files` in
  `crates/reviewgraphen-benchmark/src/prepare.rs`) with the instruction
  text replaced. No change to `prepare_real_b1`/`prepare_from_ingest_request`
  is required or made; a new, separate packet-writer script builds this
  packet directly (see `scripts/build_skill_packets.sh`), since the skill
  scaffold is not one of `prepare.rs`'s existing arm variants and this
  amendment does not add a new arm to that Rust code.

## Output schema and obligation_result mapping

`candidate-output.schema.json` is unchanged. Per the skill's own "Output
mapping" section (already frozen as part of the skill body): the model
assigns its own `packet_id` per self-enumerated obligation
(`obligation-1`, `obligation-2`, ...), sets `disposition` to its claim
polarity (`issue_present` / `issue_absent` / `inconclusive` /
`not_applicable` / `abstained`), and only emits `findings` entries for
`issue_present` obligations. This was confirmed sufficient without a
schema change before the skill was finalized (operator-reviewed);
`validate candidate` runs unchanged, with no repair or normalization, per
ADR 0037's discipline.

## Generation execution condition — unchanged

Identical to `qwen_b1`/`qwen_full`: LM Studio behind cch,
`qwen3.8:27b-mlx`, `reasoning_effort=none`, provider-default sampling,
262,144 context tokens, 65,536 output tokens, 3,600,000 ms SSE idle
timeout, zero retries, sequential (concurrency 1). No condition in this
list is changed by this amendment.

## Obligation-count cap: an experiment-side constraint, not part of the methodology

The skill body itself already states this, and it is repeated here for
the preregistration record: **at most 8 self-enumerated obligations per
unit**, an experiment-side execution constraint set before any
`qwen_skill` call, justified by this model's own empirical output-budget
behavior in this benchmark program (65,535/65,536 output tokens spent on
unstructured reasoning with zero final content, twice, versus
15,159/65,536 for the one completed run handling a fixed 5-obligation
set). This is not derived from ReviewGraphen's design documents, which
describe a budget concept for the Review Plan stage without a concrete
number (docs/07 §13, docs/09 §2, docs/11 §7) — the skill and this
amendment both say so explicitly, so a reader cannot mistake it for
doc-derived methodology.

## Known risk, recorded before execution, not preempted

Self-enumerating obligations, forming a claim with source citations, and
reasoning explicitly about evidence status for each one is strictly more
generation work per obligation than the degenerate `qwen_full` arm's fixed
5-`capability_gap`-obligation packet ever required the model to produce.
**The reasoning-budget-exhaustion risk for `qwen_skill` is assessed, before
execution, as plausibly higher than what was already observed for
`qwen_b1`** (2/2 failures in this benchmark program, both
`empty_final_after_process_completion`, both consuming 65,535/65,536
output tokens on reasoning with zero final content).

This risk is not mitigated by loosening any execution condition, raising
the output cap, or reducing the obligation cap below 8 as a preemptive
safety margin beyond what is already set. If `qwen_skill` trials fail the
same way, that failure is recorded as `reasoning_runaway` (per the
existing failure taxonomy) and reported as a real, measured result: that
the ReviewGraphen methodology, executed by a model of this size under
this local-inference condition, is too heavy to complete — not explained
away, not quietly worked around after the fact.

## Comparison groups and their asymmetry — stated plainly

- `qwen_b1`: n=1 (`head-local-00` only), failed
  (`empty_final_after_process_completion`). Per
  `DEVIATION_FULL_ARM_PRIORITY.md`, no further `qwen_b1` trials are
  planned in this amendment.
- `qwen_full`: **zero completed generation trials.** The one attempt
  (`head-local-00`) was operator-terminated mid-flight before this
  pivot (`STOPPED_CONSTRUCT_VALIDITY.md`) and consumed no semantic
  attempt; it is not restarted by this amendment, since its obligations
  are already known by direct diagnosis to be capability_gap-only
  regardless of which unit or how long it ran. `qwen_full`'s only
  evidence in this experiment is the packet-content diagnosis
  (`CAPABILITY_GAP_DIAGNOSIS.md`), not a generation result.
- `qwen_skill`: n=9 (all units), the primary arm this amendment adds.

No superiority claim is made between any pair of these arms. Any report
comparing them states the exact n for each side and does not average or
otherwise obscure the asymmetry. `qwen_b1`'s and `qwen_full`'s roles are
now: `qwen_b1` as the "does raw local qwen complete at all" baseline
observation (n=1, limited), and `qwen_full` as a closed line of inquiry
whose only contribution is the capability-gap diagnosis itself, not a
comparison point with a completed generation result.

## Extensibility for a later codex+skill comparison

The operator's Codex usage is rate-limited until 2026-08-20. To allow a
`codex_skill` arm to be added later without disturbing this amendment:
`units.json`, the frozen skill file, and the generation execution
condition above must not change before that comparison is run. When
added, `codex_skill` reuses the identical 9 units and the identical frozen
skill body (same hash), with its own execution-condition block (Codex
CLI/model/effort, to be specified in that later amendment) recorded
separately — it does not retroactively change anything in this amendment.

## Judge — unchanged

The blind judge protocol (`JUDGE_PROTOCOL.md`), its blinding mechanism,
duplicate-clustering, and quality rubric are unchanged by this amendment.
Adding a third arm does not change the blinding design: the judge still
never learns which arm (or how many arms) contributed to a unit's pooled
findings; `truth.json` records `qwen_skill` as a contributing arm exactly
like any other.
