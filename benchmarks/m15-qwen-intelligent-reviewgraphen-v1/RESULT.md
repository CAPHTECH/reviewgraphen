# Result

Neither Qwen condition completed the preregistered intelligent incremental
ReviewGraphen workflow.

| Cell | Selection/state | Scope | Convergence | Report |
| --- | --- | --- | --- | --- |
| 4bit / low | overview + target only | failed; unrelated Bash/Read | timeout at 900 s, 13 calls | absent |
| 8bit / high | six unique valid requests | passed | timeout at 901 s, 6 calls | absent |

## 4bit / low

The reviewer correctly requested the overview and `target-body`, then abandoned
the registered interface.  It searched for an unrelated historical
`/tmp/scratch/surface-to-kernel`, inspected `.reviewgraphen` internals, invoked
an invented `rg expand` command, and timed out.  Only two of twelve Bash calls
were valid ReviewGraphen requests.  No report was written.

## 8bit / high

The reviewer stayed within the ReviewGraphen command surface, made no duplicate
request, and selected five cards.  This is materially better operation
discipline than the 4bit/low observation.  It did not, however, perform adaptive
incremental exploration: after reading only the overview, it emitted all five
expansion calls in one assistant batch.  The five successful tool results
arrived within milliseconds, totaling about 27 KB, but the next model response
never completed before timeout.

The selection also followed direct-helper coverage rather than the strongest
contract-driven path.  Its own reasoning identified `alias-resolution` as
important to the documented unknown-alias contract, then omitted it to cover
direct helper labels.  Consequently it did not obtain the two source windows
needed to evaluate the previously judge-supported panic path
(`compose-rewrite` plus `alias-resolution`).

## Interpretation

8bit/high improved tool-scope adherence and state uniqueness in this one run,
but neither cell demonstrated intelligent ReviewGraphen use as defined: observe,
select, update, ground, and converge.  There are no completed findings to send
to the independent quality judge.

The interface still allowed the model to prefetch every chosen card before
observing any of them.  The next mechanism probe therefore uses a chained token:
each Projection reveals the capability required for exactly the next expansion.
That moves sequentiality from a prompt preference into ReviewGraphen-controlled
external state.

