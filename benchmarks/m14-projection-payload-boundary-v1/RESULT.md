# Result

m14 stopped after two of four preregistered phase-1 cells under the documented
post-request amendment.  The experiment did **not** establish a payload-size
boundary.

| Trial | Payload | Complete through trailer | Context calls | All calls | Result |
| --- | ---: | --- | ---: | ---: | --- |
| compact-r1 | 1,131 B | yes | 1 | 2 | recognized, 279 s |
| graph-only-r1 | 9,367 B | yes | 1 | 13 | timeout, 600 s |

Both Bash results were marked `is_error=true` with an `Exit code 1` prefix
because Claude's cwd-preservation write targeted a read-only `/tmp` path.  This
same error was present in all four successful m13 compact cells, so error status
alone is not sufficient to explain m12's repeated context calls.

The graph-only result contained its header nonce, projection ID, and trailer
nonce and had no truncation marker.  Qwen did not repeat the context command,
but it discarded the explicit receipt objective and inferred a broader source
review task from the Projection.  It issued 12 further Bash calls, searched for
repository copies, reread the Projection and probe script, attempted to run the
single-use probe directly, and timed out without `probe.json`.

This separates that failure from m12's transport truncation: a complete 9.3 KB
Projection can still trigger objective-retention and operation-selection
failure.  It does not separate bytes from semantics because the compact and
graph-only payloads also differ in graph content.  At the operator's direction,
the remaining one-shot large-payload cells were not run.  The next experiment
tests the more relevant question: whether Qwen can use small, incremental
ReviewGraphen projections selectively and converge on a grounded review.

