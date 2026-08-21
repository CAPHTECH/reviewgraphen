# Amendment 004 — current-server canonical model ID

Status: frozen before any control candidate-generation token.

The first current-server control launch passed its new identity gate, then the
server rejected the old model alias `qwen3.8:27b-mlx` with HTTP 404
`unrecognized_model`. The process record reports zero input and output tokens;
no review generation occurred.

For the current-server control diagnostic only, the client now sends the exact
model ID advertised by the pinned listing and health default:
`Qwen3.8-27B-MLX-4bit`. The original ReviewGraphen trial retains its old alias
and old server identity. All non-model-ID control conditions remain unchanged.
This further confirms that the current-server control is a separate diagnostic,
not a same-condition arm comparison.

