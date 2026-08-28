# Result

The experiment was a **partial success**. The external fresh-session
checkpoint runtime made the previously failing 4bit/low configuration complete
the full three-expansion review contract. It did not make both configurations
complete: 8bit/high timed out in its third selection node.

| Cell | m16 single-session baseline | m17 checkpoint runtime | Findings | Blind Codex judgment |
| --- | --- | --- | ---: | --- |
| 8bit / high | completed in 782 s | select-3 timeout after two valid selections | 0 | not applicable |
| 4bit / low | timeout at 901 s; stale, batched and duplicate requests | completed; 772.629 s of model-node time | 3 | 1 create / 2 reject |

## Qwen's use of ReviewGraphen

Qwen used the projections adaptively rather than following a fixed enumeration.
Both cells chose `target-body` from the overview. After reading it, 8bit/high
chose `component-rewrite-front`, while 4bit/low chose `statement-rewrite` and
then `expression-rewrite`. The latter route developed a concrete lexical
shadowing hypothesis. Independent Codex judgment supported one resulting claim:
`rewrite_action_items` does not extend the bound-name set after a `Let`, so a
later reference that collides with component state or const can be incorrectly
qualified.

This is evidence that Qwen can select ReviewGraphen projections
intelligently. It is not evidence that its route is deterministic or optimal.
The judge rejected the other two findings, including the high-severity lvalue
shadowing claim.

## What the checkpoint runtime changed

The m16 4bit/low model attempted future tool calls with a stale token, requested
an invalid card, duplicated a request, exceeded the call budget, and timed out
without a report. In m17, each selection was a fresh invocation returning one
typed request. The runtime validated it and performed exactly one deterministic
expansion before starting the next invocation. Invalid batching and stale-token
state corruption were therefore structurally impossible.

This changed the 4bit/low result from failure to completion without reducing
the three-card or final-report contract. The effect belongs to the external
checkpoint runtime, not to CaseGraphen scheduling.

## Thinking, quantization, and output volume

The completed 4bit/low arm consumed 772.629 seconds across its four model nodes;
the final review alone took 380.166 seconds of its 420-second budget. The
8bit/high select-2 took 222.930 seconds and select-3 reached its 240-second
limit. Under these deadlines, low thinking was more viable than high thinking.
Because quantization and effort changed together and there is one replicate,
the result cannot isolate either factor.

The 4bit/low prompts totaled 51,522 bytes across four fresh sessions, while raw
model streams totaled 794,669 bytes. The large volume is model thinking and
event transport, not a 794 KB ReviewGraphen tool result. ReviewGraphen bounded
the admitted source and retained source IDs and information-loss declarations;
it did not constrain generated reasoning length. A roughly 9 KB step is normal
after combining the 2.1 KB overview, one 5–6 KB source card, state, and
instructions. "Partial projection" means bounded and traceable, not necessarily
tiny.

## CaseGraphen contribution

CaseGraphen did not invoke Qwen or choose projections. It recorded the exact
approved topology, exposed arm readiness from hard dependencies, accepted
runtime results only as unreviewed evidence, required separate human waiver
reviews, recorded explicit lifecycle transitions, and preserved a replayable
revision history. No runtime completion claim or Codex disposition was promoted
to verified finding truth.

## Deviation and interpretation ceiling

The first 8bit/high selector completed, but an external-runtime relative-path
trace-writer bug initially classified the arm as incomplete. Its raw result was
preserved and recovered without another model call; its elapsed time is missing.

One replicate per jointly varied cell establishes feasibility, not stable rates.
The defensible conclusion is that fresh-session checkpoint decomposition can
make Qwen's ReviewGraphen use substantially more reliable and allowed the tested
4bit/low cell to finish. It does not establish that CaseGraphen improves model
reasoning, that low thinking is generally better, or that any model finding is
verified.
