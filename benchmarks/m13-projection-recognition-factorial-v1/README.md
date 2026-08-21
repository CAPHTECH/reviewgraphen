# m13 Projection-recognition factorial probe

This benchmark isolates one behavior observed in m12: after receiving a
ReviewGraphen target-context projection as a Bash tool result, did the reviewer
recognize that the call had already completed, or did it invoke the same tool
again?

It runs one trial in each `Qwen3.8-27B-MLX-{4bit,8bit}` × `{low,high}` requested
effort cell.  The projection is deliberately compact.  Its nonce is absent from
the prompt and available only in the first tool result.  The projection command
is single-use, making accidental repetition visible without returning the
projection twice.

This is a mechanism probe, not a code-quality benchmark.  With one trial per
cell it can identify strong configuration-specific behavior but cannot estimate
stable rates.
