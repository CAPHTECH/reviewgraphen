# Reasoning control compatibility result

The compatibility of `model_reasoning_effort=none` with the current Qwen ×
Codex Responses path was **not verified**.

The preregistered replacement ran for 1,178,580 ms without a single observable
shaped response record or provider SSE event. During that condition,
`/api/version` returned Ollama 0.32.13 in 4.758 ms. This matches the
operator-supplied server-failure rule: the control plane responds immediately
while the chat request remains unresponsive. The run was stopped, produced
process status 130, and the wrapper's request-count guard returned 70. No raw
response or candidate existed to classify.

Consequently this result does not show whether Codex 0.147 forwarded `none`,
whether the provider accepted it, or whether reasoning would have been
disabled. It counts as neither success nor protocol-invalid. In accordance
with the no-retry server rule, no further model call was made.

The Stage 1 gate remains blocked. Production cannot start from this evidence.
The current Qwen × Codex route is not classified permanently unmeasurable;
instead, reasoning control is `not_verified_due_to_server_chat_unresponsive`.
Once server operation is independently restored, a new explicitly approved
study revision would be required. This frozen v2 result must not be silently
replaced.
