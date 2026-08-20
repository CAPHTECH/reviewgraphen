# m9-agentic-local-v4 — final results

Design frozen in `preregistration-v4.json`. This report records the six
trials under the pinned `mode: baseline` condition. Earlier v2/v3 records
remain separate and are not pooled with these results.

**Series completed: 6 of 6 trials, zero retries, zero upstream failures.**
Five loops completed and one reached the preregistered 5,400-second cap.
Every trial passed both backend identity gates, received cache credit, and
had no truncated-tail signature.

## 1. The three implementation questions

| trial | arm | terminates | seconds | compiles | tests pass | verdict |
| --- | --- | --- | ---: | --- | --- | --- |
| `skill-1` | methodology | yes | 4,078 | yes | yes | `verified` |
| `noskill-1` | control | yes | 2,178 | yes | yes | `verified` |
| `skill-2` | methodology | yes | 5,293 | yes | yes | `verified` |
| `noskill-2` | control | yes | 1,598 | yes | yes | `verified` |
| `skill-3` | methodology | **no — cap** | 5,400 | yes | no | `loop_incomplete_timeout` |
| `noskill-3` | control | yes | 3,483 | yes | yes | `verified` |

The half-edited-tree rule classifies `skill-3` from how the loop ended
before considering its code state. Its compiling but test-failing partial
tree is recorded alongside the timeout and excluded from the compile/test
tallies. On completed trials, methodology is 2/2 compiling and passing;
control is 3/3. Across scheduled trials, methodology is 2/3 verified and
control is 3/3.

`skill-3` spent part of its budget constructing a scratch Cargo project to
learn the `syn::ForeignItem` API empirically. The verifier counted 5,380
out-of-scope paths, almost all under that scratch project's build outputs,
discarded all of them by construction, and applied only the target-file
diff. That partial diff changed imports but never completed the behavioural
fix: it compiled and failed three of the five acceptance tests.

## 2. Cost and the recorded counterfactual

| trial | seconds | output tokens | thinking share | max context | deep tok/s |
| --- | ---: | ---: | ---: | ---: | ---: |
| `skill-1` | 4,078 | 71,611 | 94.6% | 96,457 | 17.8 |
| `noskill-1` | 2,178 | 33,359 | 91.2% | 84,874 | 16.1 |
| `skill-2` | 5,293 | 87,359 | 92.7% | 112,526 | 16.8 |
| `noskill-2` | 1,598 | 22,198 | 86.9% | 76,991 | 12.9 |
| `skill-3` | 5,400 | 87,552 | 94.9% | 113,397 | 16.9 |
| `noskill-3` | 3,483 | 53,236 | 91.7% | 99,925 | 17.6 |

The preregistered counterfactual was about 3,017 seconds. Observed elapsed
time was 0.53x to 1.79x that value. It was not a useful point prediction
because output volume varied from 22,198 to 87,552 tokens, exactly the
unbounded term disclosed before the series.

There is complete arm separation on the two cost observations:

- methodology's shortest run (4,078 s) is longer than control's longest
  run (3,483 s);
- methodology's smallest output (71,611 tokens) is larger than control's
  largest output (53,236 tokens).

Deep decode speed does **not** separate: 12.9–17.8 tok/s across the six
trials. The cost difference is therefore observed work volume and loop
behaviour, not evidence that the server decoded one arm systematically
slower.

This complete cost separation is an observation, not an accepted causal
claim. There are only three trials per arm, all on one task, and the frozen
primary output is per-trial descriptive records. The preregistration's only
named suggestive comparative threshold was complete outcome separation;
the observed `2/3 verified` versus `3/3 verified` does not meet it.

## 3. Blind code-quality judgement

The unchanged m8 task-2 replication protocol was used: one fresh Claude
Opus call per distinct resulting file, no tools, no session persistence,
and no arm identity, acceptance test, mechanical result, or reference
solution in the packet. All six judge calls exited 0 and emitted valid JSON.

| trial | gaming | scope | convention | spec gaps | coupling | clarity | overall |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `skill-1` | none | clean | fits | some | some | clear | acceptable as-is |
| `skill-2` | none | clean | fits | none | none | clear | acceptable as-is |
| `skill-3` | none | minor excess | fits | many | some | opaque | **not acceptable** |
| `noskill-1` | none | clean | fits | some | none | clear | acceptable as-is |
| `noskill-2` | none | clean | fits | some | none | clear | acceptable as-is |
| `noskill-3` | none | clean | mixed | none | none | clear | acceptable as-is |

No judged dimension completely separates the arms, so every quality
comparison is noise under the inherited frozen rule. `test_gaming` is
`none` in all six. The only rejected change is also the mechanically
incomplete timed-out trial; there is no judge/mechanical disagreement that
changes a verdict.

The raw packets, prompts, outputs, judgments, status, elapsed time and the
arm-blind truth mapping are retained under `runs-v4/judge/`.

## 4. Honest read

The methodology **can** transfer operationally: two of three methodology
loops terminated with compiling, test-passing changes, and both were
judged acceptable as-is. This is feasibility, not evidence that the
methodology helped.

The experiment does not establish a benefit. The control completed and
verified all three trials, while the methodology arm used more time and
output in every observed pairing and lost one trial to the cap. The cost
shape is unfavorable and is consistent with the economics hypothesis, but
at n=3 it remains suggestive rather than causal.

Accordingly:

- **does it terminate?** Usually, but not reliably in this sample: 2/3
  methodology versus 3/3 control;
- **does completed code compile?** Yes, 5/5 completed loops;
- **do completed-loop tests pass?** Yes, 5/5 completed loops;
- **does the methodology improve implementation?** Not established;
- **does it cost more here?** Complete observed separation in both wall
  time and output volume, without decode-rate separation.

No further experiment is started. The still-relevant discriminating
variants remain the previously named emit-only-the-edit arm and giving both
arms the exact `syn` API surface; either would require a new preregistration
and a new operator-approved frozen server window.

## 5. Limitations

- One task, one repository, one local model/backend condition.
- Three trials per arm; within-arm resulting files are all distinct.
- One judge family. Its outputs are claims, not executable evidence.
- Alternating order reduces but does not remove temporal/server confounds.
- Mechanical verification proves the recorded build and tests only; it does
  not turn the arm-level interpretation into an accepted fact.
