# m14 Projection payload-boundary probe

This benchmark follows m13 by holding the reviewer configuration at
`Qwen3.8-27B-MLX-4bit` with requested effort `low` and varying only the
ReviewGraphen Projection payload.  All payloads are deterministic projections
of m11's frozen `lower_compose.json`.

Each payload contains a header receipt near its beginning and a trailer receipt
at its end.  The first tool is single-use.  The validator records transport
length/error/truncation observations separately from whether the reviewer
recognized the result as its completed tool call.

This is a transport/recognition mechanism probe, not a review-quality test.

