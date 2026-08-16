# Reasoning control probe: interrupted run

The first scheduled `model_reasoning_effort=none` compatibility run was
operator-interrupted and is not an evaluable model result.

Measured record:

- frozen input: snapshot-06 B1, replicate 4;
- elapsed before interruption: 480,498 ms;
- process status: 130;
- shaped Responses records: 0;
- retained provider streams: 0;
- candidate output: absent.

The operator stopped the process after incorrectly treating a transient lack
of process visibility as completion. The wrapper then reported its independent
request-count guard (`expected exactly one shaped Responses request`, exit 70).
There was no provider completion, server error, raw response, or candidate from
which support for `none` could be judged. It is therefore classified
`operator_interrupted_before_observable_response`, not valid,
protocol-invalid, server failure, or evidence that `none` is unsupported.

This is a transparent exception to the original no-retry wording in
`REASONING_CONTROL_PROBE.md`: one replacement run may be scheduled because the
registered measurement produced no observable response and was terminated by
the experiment operator, not by the provider. The replacement keeps every
registered model condition unchanged and is the only evaluable compatibility
attempt. It must not be repeated after a provider failure, rejection, ignored
setting, or completed response. This amendment is frozen before starting that
replacement.

The small control-plane records and interruption facts are retained under
`diagnostics/reasoning-none-probe-interrupted/`. No target-detection score is
derived from this run.
