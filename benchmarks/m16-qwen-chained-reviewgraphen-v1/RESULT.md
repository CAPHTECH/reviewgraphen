# Result

Qwen **can** use ReviewGraphen selectively and sequentially under an externally
enforced chained interface, but the success was configuration-dependent in this
single comparison.

| Cell | Chained operation | Convergence | Findings | Blind Codex judgment |
| --- | --- | --- | ---: | --- |
| 8bit / high | compliant, four unique projections | completed in 782 s | 3 | 1 create / 1 reject / 1 unable |
| 4bit / low | stale-token batch attempts, duplicate request | timeout at 901 s | 0 | not applicable |

## 8bit / high

The reviewer performed exactly one tool call per assistant message:

1. overview;
2. target body;
3. sync/alias boundary;
4. component rewrite front;
5. final Write.

Every opaque token was correct, no request was duplicated, all Bash calls used
the registered ReviewGraphen interface, the exact Projection trace was retained,
and every finding cited only source IDs actually received.  The resulting JSON
passed the deterministic schema/provenance validator.

The blind Codex judge, given only the three normalized findings and the full
frozen `compose.rs`, judged the unknown `internal` alias/action silent-no-op
finding `issue_should_be_created`.  It rejected the init-meta first-wins claim
as speculative design-intent confusion and marked the duplicate sync-name claim
`unable_to_determine`.  Thus the run produced one issue-worthy claim, not merely
a well-formed report, but its raw issue-worthy precision was only 1/3.

The selection was still imperfect.  It did not request `compose-rewrite` or
`alias-resolution`, so it missed the previously judge-supported unknown-alias
panic path.  Successful tool operation is therefore not complete or optimal
review planning.

## 4bit / low

The reviewer correctly consumed overview and target-body tokens.  It then
emitted three expansion calls in one assistant message using the same token.
The first (`statement-rewrite`) succeeded; the next two were rejected as stale,
including one nonexistent card name.  It later recovered the newly returned
token and obtained `component-rewrite-front`, but by then it had exceeded the
five-call budget, duplicated a requested card in its trace, and never wrote a
report before timeout.

The chained controller did its job: invalid prefetch did not corrupt or advance
accepted state.  It did not make the 4bit/low reviewer itself reliable.

## Overall interpretation

Across m13–m16, the evidence now separates four behaviors:

- Compact tool-result recognition works in all tested configurations.
- A complete semantic Projection can still cause objective/operation failure.
- 8bit/high preserves tool scope better than 4bit/low in the two intelligent-use
  probes, but an unconstrained interface still allowed non-adaptive batching and
  timeout.
- ReviewGraphen-controlled capability chaining enabled 8bit/high to finish and
  produce one issue-worthy finding; 4bit/low still failed.

This supports making ReviewGraphen the controller, not just a context dump:
small projections, opaque continuation capabilities, single-use state,
mechanical budgets, and externally guaranteed checkpoints.  It does **not**
prove whether the observed cell difference comes from 8-bit quantization,
requested high effort, their interaction, or run variance, because both factors
changed together and each cell has one replicate.

