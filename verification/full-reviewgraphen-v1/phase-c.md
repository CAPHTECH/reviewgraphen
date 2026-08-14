# Phase C: isolated process reviewer adapter

ADR 0027 adds a backend-neutral process adapter without HTTP, SDK, or async dependencies. Codex CLI and Claude CLI are implemented with `std::process::Command`; Codex App Server is a typed backend boundary that explicitly returns unsupported until its protocol lifecycle is implemented.

## Enforced boundary

- Exact relative-path/SHA-256 input inventory; extra, changed, symlinked, non-normalized, oracle/private/ground-truth/commit-message/issue-body-named input is rejected.
- bwrap read-only mounts only the admitted input at `/workspace/input`; output has a separate mount. The backend executable, minimal credential home, resolver, and runtime system roots are operational mounts, not reviewer corpus input.
- Prompt and tool-policy bytes are compiled/versioned. Callers cannot pass arbitrary command arguments, prompts, tools, or extra mounts.
- With tools disabled, the adapter serializes every admitted UTF-8 input file in normalized path order to the fixed standard-input prompt. The model does not need filesystem tools to observe the hash-bound bytes.
- Codex runs ephemeral with user rules/config ignored. Claude runs non-persistent safe mode, strict empty MCP, and no tools.
- `NonAuthorityProcessRecord` contains backend/model/settings/protocol, prompt/input/raw hashes, raw response, stdout/stderr hashes, and exit code. It has no conversion to Core claims, evidence, decisions, or terminal authority.
- Replay revalidates all bindings and returns only the exact raw bytes; it never calls a provider.

## Measured results

| Backend | Configuration | Result |
|---|---|---|
| Codex CLI 0.147.0 | `gpt-5.6-sol`, `high` | exit 0; raw `{"answer":"ok"}`; SHA-256 `2b82c37965faf7db40dae134cc94675e0f6dac048a0173a1861b6ed1b8db7d5b` |
| Claude Code 2.1.227 | `sonnet`, `high` | exit 0; raw JSON plus newline; SHA-256 `6053e074a9d246a6355af35528cc596bc0233b8e4e46ba68dd111d4a71f3bd6c` |

Codex replay output compared byte-for-byte equal to the recorded raw file; both SHA-256 values were `2b82c37965faf7db40dae134cc94675e0f6dac048a0173a1861b6ed1b8db7d5b`.

Negative observations were also useful: the first Codex attempt failed because the resolver symlink target was not mounted; after matching the proven M7 resolver mount, the provider was reachable. A later request was correctly rejected by the provider as `invalid_json_schema` because the smoke schema's property lacked `type`. Claude first rejected `{}` as an invalid MCP configuration; the strict empty configuration was corrected to `{"mcpServers":{}}`. None of these failed responses was admitted as a successful record.

The M7 candidate schema contains `uniqueItems`, which Codex strict structured output does not accept. Therefore the Codex backend does not pass that schema to the provider: it records the unmodified raw response, and the existing closed Rust candidate parser plus manifest-bound collector perform admission. Claude retains provider-side `--json-schema`. This is an explicit backend capability difference; it does not weaken the common downstream candidate contract.

## Full-arm construction evidence

The new `prepare-real-full` path registered exact production source bytes through a Core `EventLog`, recorded `SnapshotSourcesRecorded`, and called `prepare_context` for every obligation. On the checked-in M7 real corpus it successfully materialized all 40 snapshots and 200 canonical `ReviewContextEnvelope` values (five per snapshot). Reviewer source files are only the envelope-selected exact excerpts plus source indexes; the G3-proxy context packet is not used.

The model response remains non-authority. This phase introduces no promotion condition and leaves the existing D2 fake-only canonical execution contract unchanged.
