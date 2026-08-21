# Amendment 001 — operator enforcement of the eight-call cap

Status: written after the eighth recorded tool call and before result analysis.

The preregistered prompt limited the reviewer to eight review tool calls, but
`scripts/run_trial.sh` did not mechanically terminate the process at that
boundary.  The reviewer reached eight calls without writing the required
checkpoint and had begun another model response.  The operator sent SIGINT at
the eight-call boundary rather than permit an already non-compliant run to
consume the remaining wall-clock budget.  That in-flight response completed
during signal handling and issued a ninth tool call which wrote a schema-valid
provisional report.  The child process then continued to a successful terminal
result, used twelve tools in total, and updated that artifact at call twelve.
The final report is retained and judged, but it cannot make the run protocol
compliant: the checkpoint-by-five and maximum-eight conditions both failed.

SIGINT interrupted the outer shell before its normal finalization block.  The
retained stream, backend record, source manifests, scratch checkout, and stderr
were preserved.  The protocol checker, output profiler, and tail detector were
then run over that retained stream.  Source hashes were compared again against
the pre-run manifest and recorded in the result directory.  No generation was
retried or replaced.

This amendment does not turn the run into a compliant completion.  The outcome
is a valid report with a protocol violation.  It also exposes a harness defect:
future call limits must be enforced externally rather than entrusted only to
the model prompt.
