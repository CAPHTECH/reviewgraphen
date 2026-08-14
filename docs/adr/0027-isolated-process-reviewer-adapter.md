# ADR 0027: Isolated process reviewer adapter and non-authority replay

- Status: Accepted
- Date: 2026-08-14

## Context

The D2 execution contract intentionally admits exactly the deterministic fake/no-tools reviewer. ReviewGraphen has no HTTP client, model SDK, or async runtime, and the only supported review CLI is the fixed `double-submit` fixture. Local `codex exec`, Codex App Server, and `claude --print` processes make a live reviewer technically possible without adding a network crate. They do not make model output deterministic or authoritative.

Phase B also established that serialized V5 schema/gate validation is not journal authentication: locally coherent mutations can retain stale IDs/body hashes and pass. A live adapter therefore must not gain a shortcut into evidence, decisions, or terminal authority.

## Decision

Add a versioned, backend-neutral local-process boundary in `reviewgraphen-reviewer` with these rules:

1. `CodexCli` and `ClaudeCli` are executable backends. `CodexAppServer` is a typed boundary but returns an explicit unsupported error until its versioned JSON-RPC lifecycle is implemented and tested.
2. The adapter uses only `std::process::Command`. No HTTP, LLM SDK, or async dependency is added.
3. The reviewer-visible input is an absolute directory admitted against an exact relative-path-to-SHA-256 inventory. Symlinks, extra files, hash drift, non-normalized paths, and path names containing oracle/private/ground-truth/commit-message/issue-body markers are rejected.
4. bwrap mounts only that input read-only at `/workspace/input`, a fresh output directory at `/workspace/output`, runtime system roots, the backend executable, and an operator-prepared credential home. No repository root, benchmark private tree, oracle tree, or arbitrary extra directory can be configured through the adapter.
5. Prompt template bytes are compiled into the adapter. The adapter deterministically serializes the exact hash-checked UTF-8 input inventory into stdin, so neither backend needs a read or shell tool. Codex runs ephemeral with project/user rules ignored; its raw response is validated downstream because the benchmark schema uses JSON Schema keywords outside the provider's strict structured-output subset. Claude runs non-persistent, safe-mode, strict-empty MCP, with tools disabled, and uses its JSON-schema option. bwrap remains the primary filesystem boundary.
6. Every successful call yields `reviewgraphen.process_reviewer_record.v1`: backend/model/settings/protocol, prompt/tool-policy versions and hashes, exact input inventory and manifest hash, raw response and hash, stdout/stderr hashes, and exit code.
7. Replay validates all deterministic bindings and returns the exact recorded raw bytes without launching a provider. Downstream structured parsing must be deterministic over those bytes.
8. The result type is `NonAuthorityProcessRecord`. It offers no conversion into a Core execution event, evidence, verification, decision, or terminal report authority. Existing D2 fake-only admission remains unchanged. Promotion, if ever introduced, requires a separate major contract and independent authority admission; this ADR defines no promotion condition.
9. Captured raw response is canonical evidence of what the model emitted, not canonical ReviewGraphen state. Only a closed structured parser may create proposed/unreviewed claim inputs.

## Versioning and compatibility

This is additive. It does not alter `reviewgraphen.reviewer_output.v1`, D2 execution identity, existing report schemas, or M7 pilot/real results. The record schema is frozen as `reviewgraphen.process_reviewer_record.v1`; incompatible field or authority changes require a new major schema. Codex App Server activation requires a follow-up ADR or amendment naming the protocol version and replay mapping.

## Consequences

- Live model nondeterminism is isolated to raw record creation; replayed parsing/report reconstruction remains byte deterministic.
- A caller cannot add an arbitrary command, shell argument, prompt, mount, or reviewer tool through the public backend constructors.
- Credential material remains an operational mount needed by the CLI and is not part of the reviewer input inventory. Operators must prepare a minimal credential home; it must never contain benchmark private/oracle material.
- Exact-inventory validation is repeated after process exit. A detected concurrent input mutation invalidates and discards the run, although this is detection rather than a proof that no byte changed during execution.
- App Server support is designed but not claimed as implemented.
