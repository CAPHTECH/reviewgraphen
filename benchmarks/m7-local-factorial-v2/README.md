# M7 local-model × ReviewGraphen factorial v2

Status: preregistered before the first v2 semantic model call.

This additive experiment keeps `m7-local-factorial-v1` frozen and compares
`b1_free_form` with `full_reviewgraphen` on the twenty frozen positive units
from `m7-real-v1`.  It uses Qwen through Codex CLI with explicit transport
metadata: a 262,144-token context window and a 65,536-token output ceiling.

G3-proxy is excluded.  Its observed 8,131,409-character input cannot fit the
model context without changing the historical projection, which would no
longer be the frontier G3 condition.  The v1 inability to measure local G3 is
retained as an experimental result, not repaired in place.

The primary endpoint is mechanical known-target detection against the frozen
private oracle.  Protocol-invalid trials count as not detected.  Elapsed time
is reproduction metadata and never a stopping or sample-size variable.

The local row is descriptive: n=20, the FSL presence-eligible population is
28, and the conservative target is 83.  No significance, equivalence, or
factorial-interaction claim is permitted.
