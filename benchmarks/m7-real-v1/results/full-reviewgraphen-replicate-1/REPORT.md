# M7 full ReviewGraphen — additive replicate 1

Date: 2026-08-14
Status: completed research artifact; model outputs and scores are non-authority

## Result

The full ReviewGraphen arm detected 1 of 20 mechanically established regression targets (5%). The frozen existing result recorded 1/20 for B1 and 1/20 for G3-proxy. Full ReviewGraphen detected `real-unit-15`; both existing arms detected `real-unit-02`.

| Measure | B1 | G3-proxy | Full ReviewGraphen |
|---|---:|---:|---:|
| Positive targets | 20 | 20 | 20 |
| Detected positive targets | 1 (5%) | 1 (5%) | 1 (5%) |
| Positive candidate findings | 115 | 47 | 14 |
| Positive findings sent to blind adjudication | 114 | 46 | 13 |
| Control findings, initially unlabeled | 96 | 38 | 21 |
| Protocol-invalid collections | 0 | 0 | 0 |

The 95% Wilson interval for each observed 1/20 rate is 0.9%–23.6%. Full versus B1 and full versus G3-proxy each had one left-only and one right-only paired detection (two-sided exact McNemar p=1.0). B1 versus G3-proxy had no discordant pair. This single 20-pair replicate is at the floor and cannot distinguish detection performance among the arms. Fewer findings are not evidence of greater precision because ground truth labels only one selected regression root per positive and no exhaustive control defects.

## Full-arm execution

All 40 positive/control trials used the Phase C process adapter with Codex CLI 0.147.0, GPT-5.6 Sol, high reasoning, and one isolated process per snapshot. All accepted processes exited 0. Candidate parsing, trial binding, and the five-obligation denominator passed for every accepted output. Raw response bytes, input-file hashes, prompt hash, stdout/stderr hashes, backend settings, and exit status are retained in `process-records/`; replay returns the recorded response bytes without calling a model.

The reviewer process had no shell or tool executable. Its bwrap view contained only the exact hash-inventoried input at read-only `/workspace/input`, a separate writable output file, minimal credentials, runtime libraries, and resolver state required by the provider transport. The adapter deterministically serialized the admitted input files to standard input. Forbidden oracle/private/ground-truth/commit-message/issue-body path names are rejected, and the typed full-arm preparer constructs the input only from evidence-free ProgramSpace, obligations, canonical context envelopes, and their selected source excerpts. Exact hashes detect later additions or mutations. This is a construction and filesystem guarantee; it cannot semantically recognize a secret deliberately disguised inside otherwise admitted source bytes. Model output remains `NonAuthorityProcessRecord`; the adapter exposes no promotion operation.

Four early live attempts were excluded: two strict-schema attempts because the provider subset rejected JSON Schema `uniqueItems`, then two because a no-tools prompt referred to paths instead of materializing their bytes. The accepted protocol validates the closed candidate schema downstream. These failures and the single excluded adjudication transport failure are recorded in `attempts.json` and did not enter scores.

## Ground truth and blindness

This additive band reuses the exact 20 `m7-real-v1` unit/oracle contracts without rewriting the existing B1/G3 result. Positive snapshots are fix parents; controls are the fixes. For all 20 pairs, the fix-added regression test had already been mechanically recorded as failing on the parent (exit 101) and passing on the fix (exit 0). Location roots remain production-code fix-hunk anchors. Tests, diffs, commit/issue/branch material, oracles, prior pilot outputs, and existing B1/G3 candidates were not reviewer input.

The full arm differs in context construction: it receives canonical ReviewGraphen obligation/context envelopes plus exact selected source excerpts, while B1 and G3-proxy used their frozen arm definitions. Live runs occurred in separate sessions, so model nondeterminism is not paired away. These are declared comparison limitations, not silently treated as identical treatments.

## Blind adjudication

One exact root-matched positive finding was withheld from adjudication. The remaining 13 positive findings and all 21 control findings became 34 public items containing only locations, frozen mechanism tags, and exact bounded excerpts. Trial ID, role, arm, oracle, model identity, candidate prose, and severity stayed in private reconciliation or were withheld. Three isolated GPT-5.6 Sol/high batches returned a complete item-ID bijection. The adjudicator also had no shell or tools. One provider transport failure produced exit 1 and an empty response; it was excluded and the unchanged batch was rerun once.

| Revision role | Valid novel defect | Duplicate | False positive | Insufficient | Total |
|---|---:|---:|---:|---:|---:|
| Matched fix control | 8 | 10 | 1 | 2 | 21 |
| Positive, unresolved | 1 | 10 | 0 | 2 | 13 |
| **Total** | **9** | **20** | **1** | **4** | **34** |

These are blinded model dispositions, not verified defects or human acceptance. `valid_novel_defect` is item-level and duplicate resolution is batch-bounded. Controls are not empty-root false-positive proxies; every control finding remained unlabeled until adjudication.

## Ontology observation

All candidate outputs were admitted by the frozen eight-ID mechanism enum and collectively used all eight IDs. The 20 selected oracle roots use four existing IDs: `cross_file_contract`, `check_write_gap`, `state_transition_gap`, and `cross_file_effect`. This run produced no schema-level evidence requiring an ontology extension, so the ontology version was not changed. Nineteen target misses cannot determine whether limitations arose from ontology, context, prompt, or model behavior.

## Temporal contamination and selection bias

The official GPT-5.6 Sol documentation gives a knowledge cutoff of 2026-02-16 ([OpenAI model documentation](https://developers.openai.com/api/docs/models/gpt-5.6-sol)). The inspected FSL history spans 2026-06-11 through 2026-08-09, and selected fixes span 2026-07-26 through 2026-08-08, all after the cutoff. This ordering lowers the plausibility that the exact revisions were in cutoff-bounded training, but does not prove no contamination: the served model revision is unknown, provider-side post-cutoff changes are unverified, and similar code may exist elsewhere. The model had no browsing/tool route during review.

The corpus is retrospective and purposive: every target was already discovered and fixed, had a runnable added regression test, and had a production-code hunk. Thus 1/20 measures rediscovery of this selected fixed-bug sample, not discovery of unknown, current, future, or repository-wide defects. One model, one reasoning setting, one replicate, narrow location matching, non-exhaustive control labels, and model-based adjudication further limit inference.

## Bounded conclusions

1. Full ReviewGraphen, B1, and G3-proxy each detected 1/20 selected known regression roots in these recorded runs.
2. The detected unit differed, and paired testing did not distinguish full ReviewGraphen from either existing arm.
3. Full ReviewGraphen emitted fewer findings, but this cannot be translated into precision or superiority.
4. Control findings had heterogeneous blind dispositions and cannot be called false positives wholesale.
5. No ontology extension was empirically justified by this run.

## Artifact map

- `summary.json`: full-only denominator-preserving summary.
- `comparison.json`: three-arm measurements and paired comparisons.
- `inventory.json`, `manifests/`, `collections/`: expected and collected trial bindings.
- `candidates/`, `process-records/`: raw structured model responses and replay records.
- `private/scores/`: oracle-bound non-authority scores.
- `adjudication/public/`: role-blind items, decisions, and exact bounded batches; excerpts are losslessly Base64 encoded.
- `adjudication/private/`: private item/trial reconciliation and disposition summary.
- `attempts.json`: excluded failed attempts.

## Verification

Final repository checks passed on 2026-08-14:

- `cargo fmt --all --check`
- `cargo build --workspace`
- trusted-Cargo `cargo test --workspace --quiet`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `python3 scripts/validate_bundle.py` (2,199 JSON files, 14 benchmark schemas, 75 Markdown files)
- byte-identical second preparation of all 40 full trials and 200 canonical context envelopes
- exact-byte replay of all 40 process records
- runtime validation of all 40 manifests, candidates, collections, and scores
- lossless decode comparison of all 37 adjudication excerpt artifacts
- `git diff --check`
