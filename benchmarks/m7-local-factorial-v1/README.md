# M7 local-model × review-scaffold factorial v1

Status: preregistered; waiting for the operator-supplied Ollama model tag.

This additive experiment estimates whether the measured scaffold effect on the
twenty `m7-real-v1` known regression targets differs between the historical
GPT-5.6 Sol frontier row and one frozen local Qwen model. It does not rewrite
any historical candidate, score, or report.

The primary endpoint is the existing mechanically scored target-detection bit,
not AI adjudication. The design, stopping rules, protocol-conformance probe,
historical asymmetries, and analysis are frozen in `preregistration.json` and
ADR 0036 before the model tag is known.

No Ollama pull is performed by repository tooling. After download, the exact
tag, digest, capabilities, context limit, thinking setting, and generation
settings must be added to a frozen execution config before the first probe.
