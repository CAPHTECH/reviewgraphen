# Result

All four cells recognized the compact ReviewGraphen Projection correctly.  In
every trial the exact first tool was `reviewgraphen-context-probe`, it was called
once, the second and final tool was Write, and `probe.json` copied the hidden
nonce and projection ID exactly.  No trial repeated the context command.

| Cell | Actual response model | Result | Tool sequence | Elapsed | Output tokens |
| --- | --- | --- | --- | ---: | ---: |
| 4bit-low | `Qwen3.8-27B-MLX-4bit` | recognized | Bash, Write | 63 s | 892 |
| 4bit-high | `Qwen3.8-27B-MLX-4bit` | recognized | Bash, Write | 50 s | 547 |
| 8bit-low | `Qwen3.8-27B-MLX-8bit` | recognized | Bash, Write | 73 s | 529 |
| 8bit-high | `Qwen3.8-27B-MLX-8bit` | recognized | Bash, Write | 89 s | 861 |

## Interpretation

The m12 repetition is not reproduced by 4bit or requested-low alone.  A 4bit
model under requested low effort retained the tool-result provenance and hidden
nonce when the Projection was compact.  Moving to 8bit did not change the
binary outcome, and requested high effort did not produce a consistent latency
or output-volume reduction: high was smaller for 4bit but larger for 8bit.

This shifts the leading explanation toward an interaction involving m12's
large/truncated Projection, the long review task, and the Claude-agent/tool
transport.  It does not prove quantization and effort have no effect under that
larger load.  Each cell has only one replicate, the compact probe removes nearly
all review reasoning, and the server exposes the requested model through the
assistant response but does not expose the applied reasoning-effort state.
Consequently, `low` and `high` here are verified client requests, not verified
backend decode modes.

The appropriate next escalation, if needed, is a payload-size series at one
fixed model: compact, medium, and the original full Projection, while keeping
the task and single-use tool identical.  Only after locating a size-dependent
failure boundary would repeating that boundary with 4bit and 8bit distinguish
quantization sensitivity from transport/payload sensitivity efficiently.
