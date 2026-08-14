# M7 real-regression corpus v1 — replicate 1

Date: 2026-08-14
Status: completed research artifact; not accepted ReviewGraphen state

## Result

Both arms detected the same 1 of 20 mechanically established regression
targets (5%). The paired G3-proxy minus B1 target count is therefore 0. This
replicate provides no measured evidence that G3-proxy improves known-target
detection over B1.

| Measure | B1 | G3-proxy |
|---|---:|---:|
| Positive targets | 20 | 20 |
| Detected positive targets | 1 (5%) | 1 (5%) |
| Positive candidate findings after protocol normalization | 115 | 47 |
| Positive findings exported for blind adjudication | 114 | 46 |
| Control findings, initially unlabeled | 96 | 38 |
| Control target-anchor allegations | 1 | 0 |
| Protocol-invalid collections | 0 | 0 |

Both detections were on `real-unit-02`; no other selected target was detected.
Candidate-count differences are not coverage or precision measurements.

All 80 isolated runners exited 0. Strict candidate validation admitted 79 raw
outputs. `snapshot-28` G3-proxy used one ontology ID outside the frozen enum, so
its unmodified raw output was retained and the collector input was normalized
to `parse_failure`; its three raw findings were neither scored nor
adjudicated. Across normalized candidates there were 296 findings: two exact
positive-root matches and 294 findings requiring blind adjudication.

## Corpus and presence evidence

The corpus has 20 fix pairs and one replicate. For every pair, the positive is
the fix commit's first parent and the matched control is the fix commit. The
same exact added Rust integration-test argv was run on both revisions: all 20
parent runs exited 101 and all 20 fix runs exited 0. This establishes presence
and removal of the selected regression only; it does not establish that a
control revision contains no other defect.

The selected fixes are dated 2026-07-26 through 2026-08-08. Location roots use
one selected production-code hunk per defect and bind projected file/span
hashes. Tests, commit messages, issue metadata, branch names, diffs, oracles,
and prior pilot candidates were not reviewer input.

The line-preserving `m7-real-production-paths-blind-redacted.v1` projection
removes final inline `#[cfg(test)]` modules and issue/branch metadata. Two
independent preparations produced byte-identical 80-manifest trees. Prepared
G3 inputs contained zero `test_function` facts, and source scans found zero
remaining Rust test attributes, issue-number references, or Git branch refs.
This redaction is meaningful information loss and is declared in every
manifest.

## Blind execution and adjudication

Each trial ran in an ephemeral bwrap environment. Host project/FSL paths were
not mounted as reviewer data; `/workspace` contained only the read-only
manifest and `agent_input/`, plus one writable candidate file. The retained
records contain 1,145 trial shell commands: every command has cwd `/workspace`.
No network command was recorded. Raw responses are included without repair,
and full-transcript hashes are in `trial-records.json`.

Control findings were not converted to false positives by an empty-root rule.
All control findings and every unresolved positive finding were exported
without arm, revision role, oracle, trial ID, candidate prose, severity, or
model identity. A separate set of ephemeral GPT-5.6 Sol sessions adjudicated
19 role-blind batches from location, frozen mechanism tags, and exact bounded
source excerpts. The adjudicator ran 243 shell commands, all from `/workspace`,
with no recorded network command. All 294 decisions passed the private
reconciliation importer.

| Revision role | Arm | Valid novel defect | Duplicate | False positive | Insufficient | Total |
|---|---|---:|---:|---:|---:|---:|
| Matched fix control | B1 | 28 | 37 | 4 | 27 | 96 |
| Matched fix control | G3-proxy | 8 | 20 | 0 | 10 | 38 |
| Positive, unresolved | B1 | 30 | 62 | 4 | 18 | 114 |
| Positive, unresolved | G3-proxy | 8 | 25 | 1 | 12 | 46 |
| **Total** |  | **74** | **144** | **9** | **67** | **294** |

These are blinded model-adjudication dispositions, not verified defects,
human acceptance, or independent test evidence. `valid_novel_defect` counts
item decisions, not necessarily distinct repository defects; duplicate
resolution is batch-bounded. The adjudicator was the same model family as the
reviewer, candidate prose was withheld, and excerpts used a 60-line margin, so
the 67 `insufficient` decisions are expected and must not be forced into a
binary precision calculation. In particular, only 4 of 96 B1 controls and 0
of 38 G3-proxy controls were labeled false positive; treating every control
finding as false would contradict the recorded blind decisions.

## Mechanism ontology

All 20 selected regressions mapped to the already frozen eight-ID ontology, so
no extension was made. Root-defect tag incidence was:

| Mechanism ID | Selected units |
|---|---:|
| `cross_file_contract` | 9 |
| `check_write_gap` | 8 |
| `state_transition_gap` | 6 |
| `cross_file_effect` | 2 |

Units may have multiple tags. The other four IDs were unused by selected
roots. Fit on this purposive sample does not prove that the ontology covers all
real FSL defects.

## Temporal contamination assessment

The official GPT-5.6 Sol page gives a knowledge cutoff of 2026-02-16
([OpenAI model documentation](https://developers.openai.com/api/docs/models/gpt-5.6-sol)).
The 935 commits reachable from the inspected FSL HEAD span 2026-06-11 through
2026-08-09, 115–174 days after that cutoff; selected fix dates are 160–173 days
after it. This temporal ordering reduces the plausibility that these exact
revisions were present in cutoff-bounded pretraining.

It does not prove zero contamination. The served model revision is unknown,
the provider may perform post-cutoff training or evaluation, and source-level
similarities can exist independently of repository exposure. Isolation,
redaction, absence of recorded network commands, and withholding commit/issue/
test/oracle material reduce direct leakage during this run but cannot rule out
prior model exposure.

## Selection bias and threats to validity

- This is retrospective rediscovery of bugs already found and fixed. The 5%
  rate is not an estimate of discovery on unknown or future bugs.
- Selection was purposive, not random, from 935 HEAD-reachable commits. It
  required a runnable added integration test and a production-code hunk, which
  favors recent Rust/fslc regressions and reproducible test environments.
- Only changed production paths were projected. That bounds cost and blinds
  tests but can withhold cross-file context needed for detection.
- There is one replicate, one provider/model, one reasoning setting, and one
  prompt version. No uncertainty interval or significance claim is justified.
- Exact location-root matching is deliberately narrow. Semantically related
  findings outside the selected hunk enter adjudication rather than target
  recall.
- Blind adjudication is model-based, same-family, excerpt-bounded, and not a
  human or execution-backed verification pass.

## Conclusions bounded by measurements

1. B1 and G3-proxy each detected 1/20 selected known regressions; measured
   paired delta was 0.
2. G3-proxy emitted fewer normalized findings in both positive and control
   snapshots, but incomplete ground truth prevents a precision conclusion.
3. Control findings cannot be treated as false positives wholesale; their
   recorded blind dispositions are heterogeneous.
4. The existing ontology accommodated all selected roots, so this sample did
   not justify a new ontology version.
5. No claim about repository-wide defect prevalence, general code-review
   performance, future-bug discovery, or model superiority follows from this
   single retrospective replicate.

## Artifact map

- Canonical run summary: [`summary.json`](summary.json)
- Trial model/runner/raw hashes: [`trial-records.json`](trial-records.json)
- Raw model outputs: [`raw-responses/`](raw-responses/)
- Normalized candidates and collections: [`candidates/`](candidates/),
  [`collections/`](collections/)
- Private scores: [`private/scores/`](private/scores/)
- Blind public items/decisions/batches (bounded source excerpts are losslessly
  Base64-encoded for whitespace-safe Git storage):
  [`adjudication/public/`](adjudication/public/)
- Private adjudication reconciliation/summary:
  [`adjudication/private/`](adjudication/private/)
- Trial/adjudication command records: [`tool-commands.jsonl`](tool-commands.jsonl),
  [`adjudication/tool-commands.jsonl`](adjudication/tool-commands.jsonl)
- Compact-package loss declaration: [`package-record.json`](package-record.json)
- Mechanical presence evidence:
  [`../../private/evidence/`](../../private/evidence/)

## Verification

Final repository verification passed on 2026-08-14:

- `cargo fmt --all -- --check`
- `cargo build --workspace`
- `REVIEWGRAPHEN_TRUSTED_CARGO="$(scripts/resolve-trusted-cargo.sh)" cargo test --workspace --quiet`
- `cargo clippy -p reviewgraphen-benchmark --all-targets -- -D warnings`
- `python3 scripts/validate_bundle.py` (1,937 JSON files, 68 Markdown files,
  schemas, semantic mutations, and relative links)
- byte-identical regeneration of all 80 prepared manifests/inputs
- deterministic compact-result packaging (the only non-generated files are
  this report and its artifact guide)
- all 294 blind decisions accepted by the private reconciliation importer
- all 343 Base64 excerpts decoded byte-for-byte to the actual adjudicator
  inputs
- `git diff --check`
