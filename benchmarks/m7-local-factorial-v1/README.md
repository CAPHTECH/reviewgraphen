# M7 local-model × review-scaffold factorial v1

Status: preregistered; Codex-profile adapter implementation in progress.

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
