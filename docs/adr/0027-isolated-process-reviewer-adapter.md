# ADR 0027: Isolated process reviewer adapter and non-authority replay

- Status: Accepted
- Date: 2026-08-14

## 2026-08-15 amendment: named Codex profiles

Codex CLI 0.147 can route the same `codex exec` protocol through a named v2
profile stored at `$CODEX_HOME/<name>.config.toml`. The verified
`ollama-priv` profile selects provider `ollama-priv`, model
`qwen3.8:27b-mlx`, and an OpenAI-compatible local endpoint. A separate Ollama
HTTP backend would duplicate Codex's already working transport, structured
output, event stream, and raw final-message behavior. The adapter therefore
extends `CodexCli` with an optional profile and allow-listed pass-through
environment-variable names; profile omission retains the original frontier
behavior byte-for-byte.

The adapter reads but never modifies the profile. It rejects non-normalized
profile names, symlinked or oversized profile files, model mismatch, provider
binding mismatch, non-allow-listed environment names, and any parsed
`forced_login_method` key. In Codex 0.147, running this local profile with
`forced_login_method = "api"` can delete the shared ChatGPT login from
`auth.json`; refusing the key is therefore part of the process boundary, not
an operator convention.

Only `OLLAMA_PRIV_API_KEY` is currently allow-listed. Its value is read from
the parent immediately before launch, passed through bwrap to Codex, and never
serialized. The v1 backend record remains sufficient: `provider` and `model`
contain the profile-observed values, while `inference_settings` records the
profile name, profile content hash, provider base URL, reasoning effort, and
environment-variable name. Secret values remain absent. Raw response,
input-manifest binding, no-tools event validation, replay, and the
non-authority ceiling are unchanged.

Two observed Codex warnings are experimental metadata, not ignored facts:

- missing model metadata causes fallback context/model limits, which can alter
  long-input behavior; and
- the local proxy's OpenAI-style model list lacks the `models` field Codex's
  refresh path expects, although semantic responses succeed.

The local Qwen model emits long thinking traces. Empty final content caused by
an exhausted output allowance is a protocol-invalid observation, not an
abstention and not eligible for retry. The exact profile/model and the
fallback-limit warning are retained in the experiment record and report.

The pre-model M7 local-factorial preparation also exposed an adapter-side
confound: 26 of the 40 frozen G3 inputs exceeded the original 2 MiB
materialized-prompt cap, while the largest exact input was 8,127,461 bytes.
That cap would reject G3 before the local provider could express either schema
conformance or a context-limit failure. The fixed cap is therefore raised to
12 MiB, which admits every unchanged M7-real-v1 input but remains an exact,
tested boundary. No source, obligation, projection, or candidate schema is
truncated or normalized. Provider refusal or empty final content after
admission remains an observed protocol-invalid result.

## Context

The D2 execution contract intentionally admits exactly the deterministic fake/no-tools reviewer. ReviewGraphen has no HTTP client, model SDK, or async runtime, and the only supported review CLI is the fixed `double-submit` fixture. Local `codex exec`, Codex App Server, and `claude --print` processes make a live reviewer technically possible without adding a network crate. They do not make model output deterministic or authoritative.

Phase B also established that serialized V5 schema/gate validation is not journal authentication: locally coherent mutations can retain stale IDs/body hashes and pass. A live adapter therefore must not gain a shortcut into evidence, decisions, or terminal authority.

## Decision

Add a versioned, backend-neutral local-process boundary in `reviewgraphen-reviewer` with these rules:

1. `CodexCli` and `ClaudeCli` are executable backends. `CodexAppServer` is a typed boundary but returns an explicit unsupported error until its versioned JSON-RPC lifecycle is implemented and tested.
2. The adapter uses only `std::process::Command`. No HTTP, LLM SDK, or async dependency is added.
3. The reviewer-visible input is an absolute directory admitted against an exact relative-path-to-SHA-256 inventory. Symlinks, extra files, hash drift, non-normalized paths, and path names containing oracle/private/ground-truth/commit-message/issue-body markers are rejected.
4. bwrap mounts only that input read-only at `/workspace/input`, a fresh output directory at `/workspace/output`, runtime system roots, the backend executable, and an operator-prepared credential home. No repository root, benchmark private tree, oracle tree, or arbitrary extra directory can be configured through the adapter.
5. Prompt template bytes are compiled into the adapter. The adapter deterministically serializes the exact hash-checked UTF-8 input inventory into stdin, so neither backend needs a read or shell tool. Codex runs ephemeral with project/user rules ignored and strict configuration parsing. Its shell, unified execution, apps, browser/computer use, image, multi-agent, plugin, skill, view-image, and workspace-dependency features are explicitly disabled; web search is disabled and the agent registry is disabled. Its JSONL event stream is allow-listed to lifecycle, reasoning, and final message events, so any tool or unknown event refuses the run. Claude receives an explicit empty tool set and strict-empty MCP config. bwrap remains the independent filesystem boundary. The API distinguishes provider-constrained output from downstream-validated output. The latter is only for a typed consumer such as the M7 harness whose closed schema exceeds the provider subset; its raw response remains unusable until the consumer's strict schema, manifest, and denominator checks pass.
6. Before a live call, the adapter invokes the selected canonical executable with only `--version`, requires the backend-specific version shape, and derives the recorded protocol version from that observed output rather than a compile-time guess. Every successful call yields `reviewgraphen.process_reviewer_record.v1`: backend/model/settings/observed protocol, prompt/tool-policy versions and hashes, exact input inventory and manifest hash, raw response and hash, stdout/stderr hashes, and exit code.
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
