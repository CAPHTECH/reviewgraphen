# ADR 0036: Isolated Ollama curl reviewer adapter

- Status: Superseded before implementation by the 2026-08-15 ADR 0027 profile amendment
- Date: 2026-08-15
- Extends: ADR 0027

## Context

> Historical design only. No curl backend was implemented or executed. A
> verified Codex v2 profile already provides the local OpenAI-compatible route,
> so the accepted implementation is the narrower ADR 0027 amendment.

ADR 0027 admits local Codex and Claude child processes and deliberately adds no
HTTP client, model SDK, or async runtime. The replacement M7 experiment needs a
local Qwen-class model served by Ollama at the operator-approved origin
`http://mac-studio.local:11434`. Ollama documents a non-streaming `/api/chat`
endpoint whose `format` field accepts a JSON Schema, a `/api/version` endpoint,
and model digests in `/api/tags`.

This is a materially new transport boundary. Retrospectively broadening ADR
0027 would make its local-process-only isolation and v1 replay claims
ambiguous. This ADR therefore extends it and leaves every existing backend and
record unchanged.

References:

- <https://docs.ollama.com/api/chat>
- <https://docs.ollama.com/capabilities/structured-outputs>
- <https://docs.ollama.com/api-reference/get-version>
- <https://docs.ollama.com/api/tags>

## Decision

### Backend and request boundary

Add an `OllamaCurl` backend implemented only with `std::process::Command` and
the operator-supplied canonical `curl` executable. No Rust network, model SDK,
or async dependency is added.

The public configuration contains a model tag and a typed Ollama origin, not
arbitrary curl arguments or an arbitrary URL. For this experiment the only
accepted origin is exactly `http://mac-studio.local:11434`; the API path is
compiled as `/api/chat`. Curl arguments are compiled and include bounded
connect/total timeouts, `--fail-with-body`, non-streaming JSON, and
`Content-Type: application/json`. The request body is serialized by Rust and
sent on stdin. It contains:

- the exact ADR 0027 materialized prompt as one user message;
- the exact admitted output schema in `format`;
- the frozen model tag and generation settings;
- `stream: false`; and
- no tools.

Model-specific `think`, context, and generation limits are not guessed. They
must be frozen in an additive execution config after the downloaded model tag,
digest, and capabilities are observed and before any conformance probe.

### Isolation and egress

The existing exact inventory admission, forbidden blind-path checks, UTF-8 and
2 MiB prompt bound, read-only `/workspace/input`, fresh output root, system
runtime mounts, and repeated post-run inventory validation remain in force.
No repository, oracle, private benchmark tree, or credential home is mounted.
The remote model receives only the deterministically materialized admitted
input. It receives no file, shell, or other reviewer tool.

Bubblewrap cannot restrict network traffic to one host. The narrower boundary
is mechanical command construction: the caller cannot supply a curl option or
API path, and curl is invoked only for the accepted origin. HTTP on the local
network provides no cryptographic server authentication or transport
confidentiality. The experiment records this limitation and cannot claim that
DNS or the network peer was authenticated.

### Identity, raw recording, and replay

Before a semantic call the adapter records canonical curl `--version`, Ollama
`/api/version`, and the exact selected model entry and digest from `/api/tags`.
It rechecks the model digest after the call and refuses drift.

Ollama uses a new `reviewgraphen.ollama_process_reviewer_record.v1` rather than
changing ADR 0027's frozen process record. It binds and retains:

- backend kind, curl version, origin, Ollama version, model tag and digest;
- generation settings, prompt/tool-policy/schema versions and hashes;
- exact input inventory and manifest hash;
- canonical request bytes and hash;
- the complete raw HTTP response bytes and hash;
- deterministically extracted `message.content` bytes and hash;
- stdout/stderr hashes, exit code, and returned token/timing counters.

Replay performs no network call. It revalidates every hash, strictly parses the
recorded response envelope, re-extracts the same `message.content`, and returns
those exact candidate bytes to the existing closed downstream validator. A
model response that is valid HTTP but fails the candidate schema remains a
recorded `protocol_invalid` observation; it is never repaired inside the
adapter.

The result is explicitly non-authority and exposes no conversion to evidence,
verification, decision, accepted claim, or terminal gate authority.

## Compatibility

This is additive. Codex CLI, Claude CLI, Codex App Server refusal, ADR 0027 v1
records, generic review behavior, report schemas, and every historical M7
result remain unchanged. Enabling another origin, changing HTTP response
mapping, or normalizing candidate semantics requires a new decision and record
version.

## Consequences

- Local served models can use the same admitted-input and replay boundary
  without a new Rust network dependency.
- Model nondeterminism is captured once; deterministic replay starts from the
  full recorded Ollama envelope.
- The model digest and server version are observed rather than inferred from a
  mutable tag alone.
- Availability, LAN transport, server scheduling, and model quantization are
  new experimental factors and must be reported.
- A schema-conformance probe is required before the 60 positive trials; a bad
  probe cannot be hidden by retries or response repair.
