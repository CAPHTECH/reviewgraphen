# Raw provider-response diagnostic amendment

Status: frozen before the authorized snapshot-34 B1 diagnostic rerun.

The earlier snapshot-34 B1 observation is preserved unchanged: HTTP 200,
212,520 provider input tokens, 49,624 provider output tokens, zero
provider-reported reasoning tokens, zero materialized final bytes, and
4,243.771 seconds. The prior report inferred context-budget exhaustion because
input plus output equalled 262,144. That equality remains an observation, but
it is not sufficient to identify the cause. In particular,
`output_tokens - reasoning_tokens` is not proof that those tokens were final
assistant content. The previous `final_content_tokens=49,624` field was a
derived non-reasoning-output count with an overstrong name.

The diagnostic rerun has one purpose: distinguish among (a) provider output
that never became a final assistant message, including a context-boundary
termination, (b) provider output outside the expected structured response,
and (c) loss in the Codex/adapter extraction path. It is not a second semantic
attempt for target detection and must not replace the frozen pilot result.
Production remains blocked until this distinction is measured.

Before the rerun, the transport is changed to retain the complete upstream
Responses SSE byte stream for every HTTP outcome. The stream is stored as a
deterministic gzip artifact with compressed and uncompressed hashes, byte
counts, request sequence, and a completion flag. The trial result copies and
verifies that artifact before assigning any outcome. A disconnected Codex
consumer does not stop upstream capture. No response repair, normalization,
sampling override, or automatic retry is permitted.

The diagnostic executes exactly plan index 2 (`snapshot-34`, `b1_free_form`)
through the fixed batch wrapper. Its raw event types, completed response
status, incomplete details, output-item/content types, text byte counts, and
usage are summarized without treating provider usage as final-content usage.
The complete raw stream remains available for independent inspection.

Thinking is not disabled by the v2 profile: reasoning effort is `high`, and
the experiment supplies no sampling override. The independent direct chat
exposed a nonempty `message.thinking`, while the Responses proxy reported
`reasoning_tokens=0`. Therefore zero is only a provider usage field; it does
not establish that thinking was absent. Any earlier claim that v2 disabled
thinking or that thinking caused the empty final is withdrawn pending raw
event inspection.
