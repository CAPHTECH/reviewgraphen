# M7 generic-adapter consumer — full ReviewGraphen replicate 2

Date: live review began 2026-08-14 UTC and completed 2026-08-15 UTC
Status: completed research artifact; model output, scores, and adjudication are non-authority

## Result

The full ReviewGraphen replicate-2 arm detected 1 of 20 mechanically established regression targets (5%), `real-unit-15`. The frozen B1 and G3-proxy replicate-1 results each detected 1/20, `real-unit-02`; full ReviewGraphen replicate 1 also detected `real-unit-15`.

| Measure | B1 r1 | G3-proxy r1 | Full ReviewGraphen r2 |
|---|---:|---:|---:|
| Positive targets | 20 | 20 | 20 |
| Detected positive targets | 1 (5%) | 1 (5%) | 1 (5%) |
| Positive candidate findings | 115 | 47 | 17 |
| Positive findings sent to blind adjudication | 114 | 46 | 16 |
| Control findings, initially unlabeled | 96 | 38 | 18 |
| Protocol-invalid collections | 0 | 0 | 0 |

The 95% Wilson interval for 1/20 is 0.9%–23.6%. Full-r2 versus B1-r1 and full-r2 versus G3-r1 each had one finding unique to each side (two-sided exact McNemar p=1.0). Full r1 and r2 had no discordant target detection. This remains a floor result with insufficient power to distinguish the arms. Finding counts are not precision estimates because the corpus labels one selected regression root per positive and does not exhaustively label controls.

## What the generic path contributes

The ordinary product path and the M7 consumer share the isolated process adapter, exact input inventory, raw response record, and deterministic replay boundary. They do not share an obligation profile. The ordinary `MvpRulePack` smoke input generated only `reviewgraphen.capability_gap` obligations and therefore measured analyzer capability gaps rather than code-defect detection. The M7 consumer explicitly supplies the frozen eight-ID mechanism ontology, five admitted obligations per snapshot, canonical Core context envelopes, and exact selected excerpts. This is the missing explicit review profile; it is not inferred from model confidence or hidden prompt prose.

Codex CLI used the provider-constrained nested-union v2 transport for the ordinary generic smoke. The M7 candidate schema contains invariants unsupported by the provider subset, so the adapter used its explicitly separate downstream-validated mode. A raw response was usable only after strict candidate schema validation, trial/manifest binding, and exact five-obligation closure. Both modes retain raw model bytes as non-authority observations and expose no promotion operation.

## Execution and replay

All 40 replicate-2 trials ran Codex CLI 0.147.0, GPT-5.6 Sol, high reasoning, sequentially under bwrap with no reviewer tools. All process records report `codex-exec@0.147.0` and exit 0. Exact replay of all 40 raw responses matched the stored candidate bytes; all 40 candidates, collections, and scores passed their typed validators. The result has 40 prepared trials, 40 valid collections, zero missing trials, zero excluded fix pairs, and zero protocol-invalid trials.

An initial 40-trial session was excluded before scoring because its generated trial IDs collided with the frozen full replicate 1. Responses were not relabeled. The valid session was rerun from independently prepared replicate-2 packets. A collection helper also initially appended one newline to raw JSON; candidates were regenerated directly from record replay before any accepted score. `attempts.json` records these exclusions.

## Blindness and adjudication

The run reused the exact 20 unit/oracle contracts, positive fix parents, matched fix controls, presence evidence, and production root anchors. Reviewer packets excluded oracle/private/ground-truth, regression tests, diffs, commit messages, issue material, branch names, and prior candidate results. The process saw only the hash-inventoried packet at read-only `/workspace/input`; credentials and runtime roots were operational mounts.

One exact root-matched positive finding was withheld from adjudication. The remaining 16 positive findings and all 18 control findings produced 34 role-blind items. Three isolated no-tools batches returned a complete item-ID bijection, with zero tool commands and zero outside-workspace cwd records.

| Revision role | Valid novel defect | Duplicate | False positive | Insufficient | Total |
|---|---:|---:|---:|---:|---:|
| Matched fix control | 4 | 9 | 2 | 3 | 18 |
| Positive, unresolved | 3 | 10 | 0 | 3 | 16 |
| **Total** | **7** | **19** | **2** | **6** | **34** |

These are blinded model dispositions, not verified defects or human acceptance. Controls were never treated as empty-root false-positive proxies.

## Ontology and limitations

All outputs were admitted by the frozen eight-ID ontology and collectively used all eight IDs. No schema-level evidence required an ontology extension. The [official model documentation](https://developers.openai.com/api/docs/models/gpt-5.6-sol) gives GPT-5.6 Sol a 2026-02-16 knowledge cutoff, while the inspected FSL history spans 2026-06-11 through 2026-08-09. That ordering reduces but does not eliminate contamination risk; the served model revision and provider-side post-cutoff changes remain unknown.

This retrospective, purposive corpus contains already discovered and fixed bugs with runnable added regression tests and production hunks. Therefore 1/20 measures rediscovery of selected known regressions, not discovery of unknown or repository-wide defects. The comparison also crosses independent model sessions and uses B1/G3 replicate 1 against full replicate 2. No claim of safety, superiority, or precision follows from this run.

## Bounded conclusions

1. The malformed-output defect was removed from the ordinary generic smoke, but that path still abstained on 5/5 capability-gap obligations and detected no bug.
2. With an explicit M7 review profile, the generic process adapter completed a real 40-trial review and full replay/score closure.
3. Full ReviewGraphen replicate 2 detected 1/20 selected roots, the same measured rate as B1, G3-proxy, and full replicate 1.
4. The current data do not distinguish detection performance among the arms.
5. The generic product still lacks a general correctness-obligation synthesis contract; the M7 profile demonstrates a bounded consumer integration, not a universal solution.

## Addendum (2026-08-17, appended, not a rewrite)

This report's "What the generic path contributes" section states the M7
consumer "explicitly supplies... five admitted obligations per snapshot"
as "the missing explicit review profile." A later investigation
(`docs/measurement-validity-obligation-synthesis-capability-gap.md`)
directly reconstructed this replicate's snapshot-01 packet from its
manifest-declared source hashes and regenerated its obligations through
the same code path this replicate used
(`benchmarks/m7-head-local-v1/diagnostics/m7-real-v1-full-obligation-
regeneration/`). The five admitted obligations are the same
`reviewgraphen.capability_gap` obligations this report's own summary point
5 already describes as measuring "analyzer capability gaps rather than
code-defect detection" for the ordinary path — not a distinct, substantive
review profile. No adapter in this repository has ever declared the
capabilities `MvpRulePack`'s rules require at `Complete`, nor set the
`changed` artifact attribute its one capability-reachable rule needs; see
the linked finding for the full root-cause analysis. This does not change
the recorded detection counts above, which remain frozen; it corrects how
"the missing explicit review profile" should be read: as packaging
(mechanism ontology, context envelopes, excerpt selection), not as
substantive obligation content.

**Further addendum (2026-08-18):** the capability gap described above was
partially closed on 2026-08-18 for one rule (`node.changed_public_symbol`,
concurrency-evidence-gated); see `docs/23_current_capability_status.md`
for the current, verified state. This replicate's recorded results are
unaffected and remain a measurement of the pre-fix system.
