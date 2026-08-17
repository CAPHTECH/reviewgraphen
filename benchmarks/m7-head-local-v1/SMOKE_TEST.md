# Judge backend smoke test result

Status: recorded after execution, before any real generation or judge call
on the 9 real units. Required by `JUDGE_PROTOCOL.md` section 8. All content
here is synthetic — no unit from `units.json`, no fsl source content beyond
one hand-written trivial function, was used.

## What was run

`reviewgraphen-benchmark run-process-reviewer-constrained claude`, the exact
subcommand `m7-head-local-v1` specifies for the real judge (§`judge` in
`preregistration.json`), invoked three times with synthetic packets built by
hand (not through any experiment-specific packet builder). Model `haiku`
(not `opus`) was used deliberately for this plumbing test, to avoid spending
against the real judge's model budget on synthetic content; this does not
change which code path or flags were exercised.

## Test A — valid schema, trivially satisfiable

Schema: `{"type":"object","additionalProperties":false,"required":["ok"],"properties":{"ok":{"const":true}}}`.
Result: exit 0. `raw_response: "{\"ok\":true}\n"` — exact schema conformance.
Full record: `diagnostics/judge-smoke-test/record-a-valid-schema.json`.

## Test B — malformed schema string (negative control)

The `output-schema.json` file content was the literal invalid JSON
`{not valid json`. Result: **exit 1**, no record file written. stderr:
```
Error: --json-schema is not valid JSON: JSON Parse error: Expected '}'
```
This confirms `--json-schema`'s argument is genuinely parsed and enforced
by the Claude CLI (a bad value fails loudly, with no record produced), not
silently accepted or ignored. Full result:
`diagnostics/judge-smoke-test/test-b-malformed-schema-result.txt`.

## Test C — arbitrary enum with no semantic pull (constrained-decoding check)

Test A alone does not distinguish "the API constrained the output" from
"the model read the schema, which is also dumped into the prompt body by
`materialize_prompt`, and complied voluntarily," since a capable model could
satisfy `{"ok":true}` either way. To check more directly, this schema
required a field whose only valid values were three meaningless placeholder
strings with no natural motivation: `{"type":"object","additionalProperties":false,"required":["verdict"],"properties":{"verdict":{"enum":["xyzzy_alpha","xyzzy_beta","xyzzy_gamma"]}}}`.
Result: exit 0, `raw_response: {"verdict":"xyzzy_alpha"}` — one of the exact
three allowed values, with no plausible reason to have guessed it correctly
from prompt semantics alone. This is evidence (not formal proof) that
`--json-schema` is enforced at generation time, not merely requested.
Full record: `diagnostics/judge-smoke-test/record-c-arbitrary-enum.json`.

## Checklist from JUDGE_PROTOCOL.md section 8

1. **Claude CLI backend executes inside the bwrap sandbox and returns within
   bounded time on a trivial input**: confirmed — all three tests completed
   within seconds, `record.json`'s `tool_policy_version` reads
   `reviewgraphen.process_reviewer.bwrap-no-tools.v1`.
2. **`--json-schema` rejects invalid and accepts valid**: confirmed for both
   directions (Test B rejects a malformed schema string; Tests A and C
   accept and correctly constrain valid schemas). This experiment did not
   separately test whether a well-formed-but-adversarial prompt could ever
   induce a schema violation despite `--json-schema`; that risk is already
   covered operationally by `JUDGE_PROTOCOL.md` section 6's
   `judge_call_failed` handling for any output that fails validation.
3. **`--no-session-persistence` and `--tools ""` appear in the real
   invocation**: `--tools ""` is confirmed behaviorally — no tool-use
   occurred (nothing in the sandbox could reach the network or filesystem
   beyond the mounted packet, and the output directory used only by the
   Codex backend remained empty as expected for Claude). `--no-session-
   persistence` is confirmed behaviorally — the sandboxed `CLAUDE_CONFIG_DIR`
   contained an empty `sessions/` directory after all three runs (verified
   directly, not inferred). Neither flag's exact byte sequence was captured
   from a live process argv (no attempt was made to race a live process
   listing against these sub-second-to-few-second calls); both are also
   guaranteed by unconditional code construction already cited in
   `JUDGE_PROTOCOL.md` section 1 / `crates/reviewgraphen-reviewer/src/process.rs`
   lines 671-695, which contains no conditional path that omits them.

## An operational finding not previously flagged: real account, real cost

The Claude CLI backend authenticates via `CLAUDE_CONFIG_DIR` pointed at the
sandbox's `credential_home`. Unlike the qwen/LM-Studio generation path
(`OLLAMA_PRIV_API_KEY=ollama`, an arbitrary non-secret string satisfying a
local proxy that requires no real authentication), there is no equivalent
placeholder for the Claude backend: it calls the real Anthropic API under
whatever account's credentials are mounted. For this smoke test,
`~/.claude/.credentials.json` — the same account this Claude Code session
itself runs under — was copied into a fresh, isolated, mode-700 temporary
directory used only as the sandbox's read-only credential mount, and that
copy was deleted immediately after the three test calls completed; it was
never referenced by anything outside the three sandboxed invocations.

This was not called out as a cost or account consideration anywhere in
`preregistration.json`. Before the real run (up to 9 judge calls at
`model: opus, effort: high`, per `preregistration.json` `judge`), the
operator should decide and the preregistration should record: which
account's credentials fund the real judge calls, and whether a
`--max-budget-usd` cap (a real, existing flag on this Claude CLI — see
`claude --help`) should be set on the real judge invocations. This smoke
test's three `haiku`-model calls were negligible in cost; the real run's
9 `opus`/`high` calls are not evaluated here and were not run.

## Result

All three checklist items pass. No problem was found in the backend
plumbing itself. The one open item is the account/cost decision above,
which is an operational decision for the operator, not a plumbing defect —
recorded here rather than decided unilaterally.
