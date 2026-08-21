# Result

The bounded ReviewGraphen-first review produced a schema-valid two-finding
report, but **did not satisfy the convergence protocol**.  The first tool call
was exactly `reviewgraphen-context lower_compose`, but the agent invoked that
same command five times, then used one Read and two Grep calls.  At the
preregistered eight-call boundary there was still no report.  An in-flight
response subsequently wrote a provisional `review.json` as tool call nine and
the continuing child process finalized it at call twelve, after both the
checkpoint-by-five and maximum-eight constraints had failed.

## Deterministic observations

| Item | Observation |
| --- | --- |
| backend identity gate | passed (`99f80c…` listing, `29aa6e…` health) |
| first tool | exact required Bash command |
| ReviewGraphen context calls | 5 |
| source-inspection calls | 1 Read, 2 Grep |
| all recorded tool calls | 12 |
| `Skill` calls | 0 |
| Write indexes | 9 and 12 (checkpoint required by 5) |
| final valid `review.json` | present; 2 findings |
| source mutation | none; pre/post hashes match |
| retained event span | 1564.4 seconds; terminal duration 1596.3 seconds |
| terminal tail detector | no suspected truncated tail |
| final Codex judge | 1/2 `issue_should_be_created`; 1/2 `should_not_be_created` |

The terminal result reports 18,555 output tokens, 129,302 cache-creation input
tokens, and 595,968 cache-read input tokens.  The current backend separately
emits incremental `system/thinking_tokens` estimates; summing the per-response
maxima gives 16,440, with 10,940 in the longest response.  The older output
profiler sees 60,121 retained thinking characters plus 1,022 final-text
characters (about 15,286 tokens).  These are runtime observations with
different measurement methods, not interchangeable authoritative usage facts.

## Why it failed

The retained reasoning shows a concrete history-accounting failure.  After a
successful ReviewGraphen call and tool result, the model repeatedly reasoned
that it had not yet made the required first call, or treated the returned
projection as if it had been injected into the prompt.  It therefore repeated
the command five times.  Later it stated that it had used only two calls
(`Bash` plus `Read`) when the stream already contained six.  It then issued two
Greps believing they were calls three and four; they were actually calls seven
and eight.  It wrote a provisional report at call nine, performed two further
Greps, and finalized at call twelve.

The model eventually read `compose.rs` and developed possible findings, but its
unfinished reasoning also enumerated substantially more than three hypotheses
before ranking three of them.  It transitioned to Write only after the external
call budget, then withdrew the third hypothesis when its required syntax source
was unavailable.  Thus narrowing to one symbol, requesting low effort, capping
one model response at 12,000 tokens, and stating a checkpoint rule enabled
eventual termination but did not enforce bounded convergence when the agent
could not reliably account for completed tool turns.

## Judgment and interpretation ceiling

ReviewGraphen was genuinely used as a command; the repository methodology
skill was not installed or invoked.  The final report retained two findings.
Codex judged the unknown-alias panic issue-worthy.  It rejected the second
finding because the candidate assumed a ForAll binder could be an assignment
target; the code instead supports interpreting lvalue bases as component state
and binders as read-only expression names.  The agent itself withdrew the
Until/Unless hypothesis before finalization because the required variant
definitions were unavailable.

This run does **not** show that bounded ReviewGraphen review is intrinsically
impossible.  It shows that the current server can eventually produce a usable
bounded review and that one of its two final findings survives an independent judge,
while also showing that prompt-only checkpoint and call-count rules are not
reliable for this Claude-agent/local-API combination.

The next valid test needs mechanical controls: make the context command
single-use, enforce the tool-call counter outside the model, and guarantee a
schema-valid checkpoint independently of the model's remembered call count.
