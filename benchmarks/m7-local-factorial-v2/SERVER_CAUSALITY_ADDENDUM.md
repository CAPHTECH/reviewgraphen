# M7 local-factorial server-causality addendum

This addendum changes causal interpretation, not any frozen observation.

The frozen v1 snapshot-34 B1 record remains 867,764 admitted bytes,
4,458.289 seconds, and an empty final response. The interrupted v2 attempt
remains 867,861 admitted bytes, 3,301.536 seconds, and a stream closed before
`response.completed`. Earlier notes treated these as evidence that Qwen's
thinking might not terminate. That attribution is withdrawn: the operator
subsequently identified an Ollama MLX runner hang, adjusted the server, and
demonstrated normal termination. A separate pre-resume probe then measured
HTTP 200 for `/api/version` in 4.9 ms, `/api/ps` in 5.3 ms, and a sampling-
unspecified Qwen chat in 1.006 seconds with final content `OK` and stop reason
`stop`. The retained long/empty observations are therefore consistent with a
server failure and do not establish a thinking-termination defect.

Long prefill is not itself classified as failure. Operator measurements range
from 16.4 seconds for 4k tokens through 37 minutes for 205k tokens, so elapsed
time is reproduction metadata only. An upstream 5xx or an incomplete stream
before `response.completed` is recorded as a server/transport invalid result
and stops the batch without automatic retry. `/api/version` and `/api/ps` are
checked before each batch; they establish control-plane reachability but do
not by themselves prove that generation is healthy.

The interrupted trials are rerun only because the operator explicitly
authorized resumption after the server adjustment. Their earlier records are
not overwritten, and the resumed runs use a distinct attempt label. The
already completed and schema-valid snapshot-06 B1 trial is retained without
rerun.

No temperature, top-p, or other sampling parameter is supplied. The only
upstream is `http://192.168.68.71:11999`; direct port 11434 and mDNS are not
used. The profile continues to use `OLLAMA_PRIV_API_KEY` and must not contain
`forced_login_method`.

## Prefix cache decision

No prompt or trial order is rewritten to seek a longer cache hit. The adapter
already places one fixed prompt before the path-sorted admitted files, so all
trials share that exact leading prefix and same-arm trials may naturally share
more. Moving variable files or grouping arms after preregistration would
change the frozen prompt/order and could give one arm a different warm-state
history. Automatic exact-prefix caching remains an observed provider behavior;
provider-reported cached input tokens are recorded when available and are not
used for validity, target scoring, or the production decision.
