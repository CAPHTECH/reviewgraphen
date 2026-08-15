# M7 local-model × review-scaffold factorial v1

Status: schema probe stopped at the preregistered threshold (4/6 candidate
outputs compliant); stage 2 and positive trials were not run.

This additive experiment estimates whether the measured scaffold effect on the
twenty `m7-real-v1` known regression targets differs between the historical
GPT-5.6 Sol frontier row and one frozen local Qwen model. It does not rewrite
any historical candidate, score, or report.

The primary endpoint is the existing mechanically scored target-detection bit,
not AI adjudication. The design, stopping rules, protocol-conformance probe,
historical asymmetries, and analysis are frozen in `preregistration.json` and
the 2026-08-15 amendment to ADR 0027. ADR 0036 preserves the superseded
standalone-curl design; it was never implemented or exercised.

No Ollama pull is performed by repository tooling. The verified local route is
Codex CLI 0.147 profile `ollama-priv`, provider `ollama-priv`, model
`qwen3.8:27b-mlx`. Its profile hash and effective fallback metadata warning
must be recorded before the first probe.

The frozen control-only probe plan is `schema-probe/plan.json`. Its stage 1
uses `snapshot-06` (minimum B1 admitted bytes) and `snapshot-34` (maximum),
with all three arms for each. The pre-model input audit found that the
historical G3 packets exceed the original process-adapter prompt cap in 26 of
40 snapshots, so ADR 0027 raises the fixed cap without truncating an arm.

Latency is recorded only for reproduction and never gates execution or reduces
the twenty-unit row. The FSL presence-eligible population is 28 while the
conservative target is 83, so this experiment reports descriptive cells and
does not test or claim a significant interaction. A later multi-repository
Rust corpus is the preregistered route to a larger denominator if the local
protocol proves usable.

The observed result and immutable raw artifacts are under
`schema-probe/results/`. This v1 experiment did not measure target detection.
