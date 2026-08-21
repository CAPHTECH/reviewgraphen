# Amendment 003 — current-server no-ReviewGraphen diagnostic

Status: frozen after the original server identity drifted and before any
control model request.

The attempted `control-1` launch made no model request: its identity gate
observed that the server had restarted or changed its control-plane projection.
The previously exposed `x_mlx_dspark` listing metadata and the detailed health
configuration fields disappeared. The current health endpoint reports one
loaded model and default model `Qwen3.8-27B-MLX-4bit`, but does not declare mode,
drafter, reasoning effort, context window, or server max output through the
fields used by the frozen gate.

The original hashes remain unchanged and continue to identify
`reviewgraphen-1`. Separate control-diagnostic pins identify the current
control-plane response:

- listing: `99f80c57621fbb0956d17a74916ecc6480c146f1c8a5f0e111c28c4736eeb7ab`
- health: `29aa6e9b732b131a25395f367448132d6b8185c3771e01c6285635b347930f74`

`control-1` is now a fresh current-server diagnostic only. The client still
sends model alias `qwen3.8:27b-mlx`, uses the same Claude agent harness and
`reasoning_effort=low` path, and sets client max output 32,000. Server decode
mode, drafter, and server-side maximum are review-required unknowns, not inferred
from the model name or loaded count. This result cannot be compared causally or
economically with `reviewgraphen-1`; it answers only whether no-ReviewGraphen
review completes under the server state actually available now.

