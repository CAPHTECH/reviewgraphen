# ADR 0051: macOS portable CLI boundary

## Status

Accepted, 2026-09-15.

## Context

The `reviewgraphen` binary was structurally a single Rust executable, but the
CLI refused every input read outside Linux and its runtime/report crates
compiled Linux-only durable Store surfaces unconditionally. The portable
generic review path itself does not require the durable Store. Treating all of
ReviewGraphen as Linux-only therefore made the deployment boundary broader
than the implementation dependency required.

macOS does not provide the Linux `openat2`, `O_TMPFILE`, and `AT_EMPTY_PATH`
combination used by the Store's descriptor-rooted, create-only durability
contract. A path-based Store fallback would weaken an accepted security
boundary and is not an acceptable portability implementation.

## Decision

1. The portable product boundary is the `reviewgraphen` binary's generic
   provider-free/replay `review` path and `schema` surface.
2. Linux and macOS use the Unix `O_NOFOLLOW` descriptor-open followed by
   `fstat`, regular-file admission, and the same 128 MiB byte ceiling for CLI
   input files.
3. Runtime durable orchestration and Store-bound report generation are
   compiled only on Linux. Generic runtime diagnostics and non-authority
   generic report projection remain portable.
4. On non-Linux hosts, report-v4/v5 schema validation refuses before semantic
   validation with the typed wire reason `unsupported_platform`; semantic
   checks are never silently omitted.
5. macOS 15+ Apple Silicon is continuously checked by a pinned CI job that
   runs the complete `reviewgraphen-cli` test target, builds the release
   binary, and exercises embedded schema operations.
6. The bubblewrap-backed external process reviewer remains Linux-only. This
   does not narrow the currently implemented generic product path, which
   already rejects live Codex/Claude execution and admits provider-free or
   verified replay execution.

## Consequences

- A native Apple Silicon macOS binary can run the currently supported portable
  CLI without a Linux VM.
- Linux retains all existing Store and report behavior unchanged.
- macOS does not claim durable Store, Store-bound report-v4/v5 semantic
  validation, or isolated live-provider execution.
- Intel macOS is not yet CI-verified and is not part of this support claim.
- Extending durable storage to macOS requires a separate ADR proving an
  equivalent descriptor-root and atomic create-only publication contract.
