# Full ReviewGraphen verification

This directory records the 2026-08-14 Phase A–D audit and experiment. Results distinguish mechanical enforcement, test evidence, documentation-only claims, unsuccessful attacks, broken serialized boundaries, and unverified claims.

- Phase A: `capability-matrix.json` and `capability-matrix.md`
- Phase B: `phase-b-adversarial.md`, `adversarial-results.json`, and `scripts/run_adversarial.py`
- Gate trust-boundary follow-up: `gate-trust-boundary-root-cause.{md,json}`,
  `gate-trust-boundary-after.json`, and `gate-trust-boundary-remediation.md`
- R1 tuple integrity: `r1-adversarial-results.json` and `r1-report.md`
- R2 denominator closure: `r2-adversarial-results.json` and `r2-report.md`
- Phase C: ADR 0027 and implementation/tests in `reviewgraphen-reviewer`
- Phase D: `phase-d.md` and the additive result bundle under `benchmarks/m7-real-v1/results/full-reviewgraphen-replicate-1/`

No pre-existing M7 result is replaced by this work.
