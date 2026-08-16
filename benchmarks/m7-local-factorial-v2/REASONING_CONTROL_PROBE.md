# Reasoning control compatibility probe

Status: frozen before the first `model_reasoning_effort=none` model call.

This is an additive transport compatibility diagnostic, not a target-detection
replicate. It uses the already prepared snapshot-06 B1 control packet because
that is the smallest frozen B1 pilot input. Its candidate output is not scored
and cannot replace any frozen v1 or v2 trial.

The only changed request setting is the adapter's existing
`model_reasoning_effort`, from `high` to `none`. The profile, model, provider,
262,144-token context, 65,536-token total output limit, schema, prompt, tool
policy, isolation, and sampling settings are unchanged. The complete provider
stream is retained. There is no retry, repair, normalization, or automatic
fallback to another reasoning level.

The control is effective only if all of the following are observed:

- `response.created.response.reasoning.effort` is `none`;
- no `response.reasoning_*` delta event is emitted;
- at least one `response.output_text.delta` is emitted;
- the provider completes and Codex materializes a nonempty final response.

Candidate-schema conformance is reported separately. If `none` is rejected,
silently changed, or still emits reasoning deltas, thinking cannot be disabled
through this Codex 0.147 profile boundary. A total output limit is not a
reasoning-only cap and is not treated as a substitute. In that case the current
Qwen × Codex Responses B1 condition is classified as not operationally
measurable, and production remains blocked.
