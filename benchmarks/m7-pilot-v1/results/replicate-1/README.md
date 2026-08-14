# M7 pilot replicate 1 — audit bundle

This directory preserves the first paired pilot run. It is research evidence, not
accepted review state. The current deterministic score is **invalid for detection
performance claims** because the scoring contract exposed a mechanism-tag
vocabulary defect described below.

## Post-fix protocol status

Raw candidate JSON and the legacy `scores/`/`summary.json` are immutable audit
evidence. Under protocol v2, `a05-b1` is `protocol_invalid` because B1 emitted
obligation results, and `a03-g3` is `protocol_invalid` because its packet result
set is not the exact manifest-bound G3 proxy set. Both have no valid detection
denominator. The legacy scores remain frozen invalid evidence and must not be
reported as benchmark performance.

## Protocol

- Eight opaque units were reviewed once in each arm: `b1_free_form` and
  `g3_proxy`.
- Each trial used a fresh isolated `gpt-5.6-terra` session with reasoning effort
  `high` and only its prepared `agent_input` plus the sibling manifest.
- The reviewer was prohibited from inspecting the repository, sibling arm,
  private oracle, or prior outputs.
- Units a01–a04 contain one seeded root each. Units a05–a08 are matched safe
  controls with no oracle roots.
- One attempted agent launch was rejected by the orchestration concurrency limit
  before the a05-g3 reviewer accessed any input. The trial was subsequently run
  in a fresh session; no model output was retried, repaired, or normalized.
- `candidates/` contains the raw model JSON. `scores/` contains the canonical
  output of the deterministic scorer, and `summary.json` contains its paired
  aggregate.

## Recorded metrics

| Metric | B1 | G3 proxy |
| --- | ---: | ---: |
| Structured outputs | 8/8 | 8/8 |
| Parse failures | 0 | 0 |
| Candidate findings | 7 | 8 |
| Scorer-matched candidates | 0 | 0 |
| Scorer-unmatched candidates | 7 | 8 |
| Scorer-detected seeded roots | 0/4 | 0/4 |
| Safe-control units with one or more findings | 3/4 | 3/4 |
| Findings on safe-control units | 3 | 3 |

The safe-control findings are not labelled false positives. That requires blinded
expert adjudication, which was not performed for this bundle.

## Measurement defects discovered by the pilot

The apparent 0/4 versus 0/4 detection result is not interpretable. On every
seeded unit, both arms emitted a finding whose location overlaps the oracle root
and whose rationale describes the relevant defect, but the scorer additionally
requires a literal intersection between unconstrained candidate tags and private
oracle tags. For example, a01 uses oracle tag `check_write_gap` while a candidate
uses `check_then_act_race`. No candidate happened to reproduce an oracle tag, so
all location-overlapping findings were counted as unmatched. This is an
unblinded audit observation, not a replacement score or adjudication.

The pilot also exposed two contract-boundary defects:

1. Candidate validation does not bind `obligation_results.packet_id` to the
   manifest arm or packet set. Consequently, a05-b1 was accepted with six
   obligation results even though B1 supplied no obligation packets.
2. The nominal blind-adjudication export retains `trial_id`, whose current value
   contains `b1` or `g3_proxy`; it therefore reveals the arm and cannot support
   an arm-blind adjudication as written.

Until those defects are corrected and a fresh, uncontaminated replicate is run,
this bundle supports harness diagnosis only. It does not establish equal,
superior, or inferior detection performance for either arm.

## Limitations and reuse

This is one replicate over four seeded positives and four matched controls in a
narrow synthetic corpus limited to current payment/idempotency and cross-file
concurrency rules. `g3_proxy` is a bounded non-authority packet projection, not a
full authority-bound ReviewGraphen execution. Unmatched findings have no
precision label without blinded expert adjudication. The pilot records neither
provider token telemetry nor a model revision beyond what the manifest exposes.

These outputs and private oracle material must not be fed to future blinded
runs. A future valid comparison needs new opaque units or otherwise proven
uncontaminated inputs after the scoring, manifest-binding, and blinding defects
are fixed.
