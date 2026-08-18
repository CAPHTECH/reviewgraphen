# 20. M2 Rust and Git Ingestion Contract

> Status: Implemented for M2
> Updated: 2026-08-09
> Schema/API version: frozen `reviewgraphen.program_space.input.v2` (ADR 0011), additive M6 `reviewgraphen.program_space.input.v3` (ADR 0023), and `reviewgraphen.extraction_report.v1`

## Scope

`reviewgraphen-ingest` ingests one specified commit from a local Git repository
into the existing language-neutral `ProgramSpace` contract. It implements the
first Code Review profile adapter set:

- a bounded immutable Git tree snapshot;
- a `syn` 2.x Rust parser adapter;
- an offline Cargo metadata adapter;
- limited containment, syntactically unique direct-call, import/module,
  test-to-direct-target, selected assignment/write, and changed-structure facts.

The crate is deterministic for one `(workspace scope, repository identity,
target revision, configuration)` tuple. `base_revision` bounds only the
changed-structure diff mapping: the `change:*` artifacts (`change_kind`,
`base_path`, `target_path`, and, when the target file actually differs from
the base, `changed_lines`) and the `changed_by` relations from a target
file/module/function/method/type/test/state record to those `change:*`
artifacts. It is deliberately excluded from `snapshot_id` and from every
non-change fact's ID *and* canonical body, so ingesting the same target
revision with a different base never changes an unrelated fact's ID, its
attributes, its location, or its provenance -- the only thing a different
base can add, remove, or change is which `change:*` artifacts exist and
which records a `changed_by` relation points at. It does not execute target
code, invoke a shell, call a network service, or mutate the target
repository.

## Input and security boundary

`IngestRequest` requires an explicit `workspace_root` and `repository_root`.
The canonical repository root must be contained by the canonical workspace root
and must be exactly the Git top-level root. The adapter resolves revisions to
full commit IDs, reads files with fixed `git` read-only subcommands, and accepts
only normal regular-file tree entries.

`IngestRequest` also requires an explicit, non-empty `repository_identity`
(for example a remote URL or another caller-chosen stable name). Unlike
`repository_root`, this identity is never derived from an absolute local
path: `ProgramSpace`'s `repository_id`, `snapshot_id`, every derived
artifact/relation/limitation ID, and every emitted `SourceRef` locator are
derived from `repository_identity`, not from the clone's filesystem location.
Ingesting the same repository revision from two different local clone paths
with the same `repository_identity` therefore produces the same stable IDs.
`repository_root` is retained only as informational, non-canonical
`repository.root` metadata (see `RepositoryDescriptor` in
`reviewgraphen-core`), exactly as ADR 0011's core contract already
anticipates.

Git paths that are absolute or contain a parent component are rejected. Git
symlinks are never followed: they are excluded from accepted facts and retained
as a typed `region_excluded` obstruction. A Git submodule entry (Git object
type `commit`) is likewise never entered: it is retained as a typed
`unsupported_input` obstruction instead of aborting ingestion of the rest of
the snapshot. File and byte limits are enforced before Rust parsing.

The git adapter's own `AdapterReport` makes this explicit as a denominator:
`total` is every discovered tree entry, `parsed` is the accepted regular-file
subset, `excluded` counts entries a declared bound (symlink, submodule, or
any other non-regular-file entry) deliberately excluded, and `failed` is 0
for this adapter today (a size/count bound violation is still a hard,
whole-request `IngestError`, not a per-entry failure -- see the open items
below). `parsed + excluded + failed == total` whenever all four are present.

An excluded entry also propagates into the `extraction.capabilities`
denominator, not only the adapter report: any excluded entry always relates
its `region_excluded`/`unsupported_input` limitation to `git_snapshot` and
downgrades it from `complete` to `partial`, since that capability's claim is
exactly "every discovered tree entry became an accepted fact," which an
excluded entry always contradicts. `changed_structure` downgrades too, but
only when the excluded path is one Git's own `diff --name-status` between
`base_revision` and `target_revision` also reports changed -- that is the
only case where the changed-structure mapping actually loses a fact it
would otherwise have produced, and the same limitation is additionally
related to, and source-keyed against, that `change:*` fact. An excluded
entry with no corresponding diff entry (unchanged between base and target)
leaves `changed_structure` at `complete` and unrelated. Both capabilities'
own `source_ids` are grounded in the facts they actually declare -- the
accepted `file:*` artifacts for `git_snapshot`, the accepted `change:*`
artifacts for `changed_structure` -- falling back to `snapshot_id` only when
that set would otherwise be empty, consistent with every other M2
capability's source-trace pattern. This is a bugfix to M2's existing output
(the obstruction kinds, capability names, and denominator fields all already
existed), not a shape change, so no schema version bump or ADR was required
for it.

The only child process commands are private implementation details:

```text
git rev-parse / ls-tree / show / diff
cargo --version
cargo metadata --offline --no-deps --format-version=1
```

No public arbitrary-command API exists. Cargo receives a private temporary
snapshot copied from Git bytes, never the target repository. Its `CARGO_HOME`
is also a disposable directory inside that private snapshot. Unlike `git`,
`cargo` is never spawned unconditionally: it runs only when the caller has
explicitly admitted a trusted executable -- see "Cargo tool admission" below.
This module never searches `PATH`, and never spawns `rustup`, `mise`, `asdf`,
or any other toolchain manager: an audited runtime `rustup` probe (for
example `rustup which cargo`) can itself touch the toolchain even under a
"never install" configuration, so no automatic resolution performed from
inside this crate can be made safe. Admitting a real Cargo executable is an
external harness responsibility, not something M2 attempts on its own.

Before Cargo metadata ever runs, every `Cargo.toml` in the snapshot (root and
every workspace member) is parsed as TOML and checked for **workspace
containment**: any `path` reference anywhere in the document (a `[dependencies]`
/ `[dev-dependencies]` / `[build-dependencies]` / target-specific dependency
table, or a `[patch]`/`[replace]` override), `package.workspace`, and every
`[workspace] members` entry are each resolved relative to their own
manifest's snapshot directory and rejected only if they would cross above the
snapshot root. A sibling-crate reference that stays inside the snapshot (the
normal shape of a real Cargo workspace) is accepted; only a genuine escape,
or a manifest that fails to parse as TOML at all (never silently treated as
safe), skips Cargo metadata and records a `capability_missing` limitation for
`cargo_metadata`.

### Subprocess resource bounds

Every allow-listed `git`/`cargo` invocation runs through a bounded
executor: a fixed 30s wall-clock timeout (the process is killed, not awaited
indefinitely, past that bound) and a 64 MiB captured-output cap on stdout and
stderr each, independent of and in addition to `IngestLimits::max_file_bytes`
(which bounds one tracked file's content after a successful `git show`). A
blob's size is also precomputed from `git ls-tree -l` and checked against
`max_file_bytes` before the separate `git show` fetch of its full content, so
an oversized blob is rejected without first paying for its download.

These are fixed internal constants today, not part of the public
`IngestConfig`. What explicitly remains out of scope for this bound:

- The 30s timeout and 64 MiB cap are not configurable per request.
- No bound on Cargo's own disk usage while staging the private snapshot or
  writing to the disposable `CARGO_HOME` (only wall-clock time and captured
  stdout/stderr are bounded, not the staged directory's on-disk size).
- No bound on peak memory used while parsing one accepted Rust file with
  `syn`, independent of the already-enforced `max_file_bytes` byte bound on
  its source text.
- No protection against a pathological Git object graph (for example an
  extreme delta chain) that makes a single `git show`/`ls-tree` slow within
  the 30s bound but still resource-heavy.
- No rate limiting or concurrency bound across multiple simultaneous
  `ingest()` calls sharing one host.
- `[workspace] members` glob patterns (for example `"crates/*"`) are checked
  lexically for a `..` component, never expanded/resolved to their real
  matched paths; a member string without `..` that a glob library would
  still resolve outside the snapshot is not (and, on the local M2 evidence
  available, cannot be) caught by this static check.

The captured-output cap fails closed, not silently: if a stream (stdout or
stderr, checked independently) actually exceeds its cap, the bounded
executor never returns the truncated bytes as a successful result, even
when the child process itself exited with a success status. Git snapshot
loading and changed-structure diffing propagate this as an ordinary
`IngestError` through `ingest()`'s existing `?` chain, failing the whole
request rather than accepting a partially-read `ls-tree`/`diff`/`show`
output as if it were complete. Cargo metadata's existing generic
`Err(error) => ...` handling in `extract_cargo_metadata` already converts
any `IngestError` from a `cargo metadata` invocation -- including this one
-- into the pre-existing `cargo_metadata: missing` /
`CargoMetadataUnavailable` obstruction; no separate code path was needed for
that conversion. A reader-thread panic is likewise never absorbed into an
empty result: it becomes the same typed I/O failure a genuine read error
would.

**New public error variant**: `IngestError::OutputLimitExceeded { command,
stream, limit }` was added. `IngestError` is a plain (not
`#[non_exhaustive]`) public enum, so in principle an external exhaustive
`match` on it would need a new arm; in practice, every existing consumer of
this enum (this crate's own tests) already matches it via `matches!`/
specific-variant patterns, never an exhaustive `match`, so this is treated
as the same kind of additive change as the pre-existing
`FileTooLarge`/`FileLimitExceeded` variants rather than a breaking one. It
does not change `reviewgraphen.program_space.input.v2` or
`reviewgraphen.extraction_report.v1`'s JSON shape at all (it is a
Rust-level `ingest()` failure, never serialized into either output), so no
schema version bump or ADR is required for it.

### Extractor-set provenance binds real tool identity

`ExtractionReport.adapter_set_hash` (also embedded in
`ProgramSpace.extraction.adapter_set_hash`) is a SHA-256 over a canonical
JSON object that binds every input that can change what this run actually
extracts, not just the fixed `reviewgraphen.ingest.*@1` adapter-contract
version strings:

- `limits` (`IngestConfig.limits` -- `max_files`, `max_file_bytes`): the
  only other `IngestConfig` field that affects parsing. `profile_id`,
  `profile_version`, `rule_set_hash`, and `policy_version` are deliberately
  excluded: no adapter ever consults them while deciding what to accept,
  they are pass-through identity already recorded separately in
  `ProgramSpace.profile`, and folding them in would make the same extractor
  behavior hash differently for no extraction-relevant reason.
- `tool_versions.git`: the *real* `git --version` output for the binary
  this run actually executed, fetched once per `load_snapshot` through the
  same bounded allow-list executor every other `git` invocation already
  uses -- never a hardcoded or guessed string. `git` is unconditionally
  required already (every other Git call in this module already needs a
  working `git`), so a failure here fails the whole request exactly as an
  early `rev-parse` failure already would; it is not a new hard
  requirement.
- `tool_versions.cargo`: a structured `{"available": true, "version": "..."}`
  or `{"available": false, "unavailable_kind": "..."}`, fetched through
  the same bounded executor `cargo metadata` itself uses -- never a bare
  `null`, so an unavailable *kind* is itself part of the fingerprint
  rather than indistinguishable from "no opinion" (two different failure
  kinds, for example a missing binary versus non-UTF-8 output, hash
  differently). Determined from the *exact same* working directory `cargo
  metadata` itself later runs from -- the snapshot is staged to a private
  temporary directory exactly once (`load_snapshot`), `cargo --version` is
  run there immediately, and `extract_cargo_metadata` later reuses that
  same staged directory rather than staging a second, independent copy.
  This matters because, even though the admitted `cargo` binary itself is
  already fixed under strict host admission (below) and never re-resolved
  based on the working directory, Cargo's own config and manifest
  interpretation -- workspace-root discovery, `.cargo/config.toml` lookup,
  and path-relative dependency/patch resolution -- is directory-relative;
  running `cargo --version` from one staged copy while `cargo metadata`
  runs from a separately staged copy of otherwise-identical content could
  still resolve that context differently, even though the identical binary
  produced both outputs. A snapshot need not be a Cargo project and a host
  need not have Cargo installed, so `cargo --version` failing alone
  (including a staging failure, treated identically) does not fail the
  whole request, matching `cargo_metadata`'s existing optional-capability
  contract. But
  `extract_cargo_metadata` treats that failure as an unconditional
  precondition, checked before even looking for a root `Cargo.toml`:
  while `cargo --version` could not be determined, `cargo metadata` is
  never run and no package/dependency fact is ever accepted, regardless of
  whether the manifest is otherwise perfectly valid and a `cargo metadata`
  invocation would have succeeded. `cargo_metadata` is declared `missing`
  (`NotRun`) for that run, with a `cargo_metadata_unavailable` limitation
  naming the cause -- the same shape every other "Cargo metadata did not
  run" case already produces. This closes a gap where an unattributed
  `cargo` fingerprint (`null`) could previously still sit alongside
  accepted `cargo metadata` facts nothing could actually vouch for.

  A `cargo --version` (or its preceding snapshot-staging) failure is a
  typed `CargoToolFailure { kind: CargoToolFailureKind, diagnostic: String
  }`, never a raw, unclassified `String`. `kind` is one of a fixed set of
  stable categories (`staging_failed`, `not_admitted`,
  `admitted_executable_invalid`, `unavailable`, `timed_out`,
  `output_too_large`, `non_success_exit`, `not_utf8`, `empty_output`).
  `not_admitted` and `admitted_executable_invalid` are the two kinds strict
  host admission (below) added: `not_admitted` is `CargoToolAdmission::
  Disabled` (the default -- no executable was ever admitted for this
  request); `admitted_executable_invalid` is a `TrustedExecutable(path)`
  whose `path` fails the caller-independent structural check (`git::
  admit_cargo_executable`) -- not absolute, not canonicalizable to an
  existing entry, or not a regular, executable file.
  `diagnostic` is a human-readable message with the private staged
  snapshot root -- and its canonicalized form, when it differs -- replaced
  by the fixed placeholder `<staged-snapshot>`, since a real failure (for
  example a `rust-toolchain.toml`-driven toolchain-resolution error) can
  echo that randomly-named directory verbatim in its `stderr`; it is kept
  on the internal `CargoToolFailure` value for local diagnosis only and is
  never read while building the resulting `cargo_metadata_unavailable`
  obstruction. That obstruction's `description` -- like every other M2
  obstruction's -- is built only from the failure's stable `kind` and
  fixed wording, never from `diagnostic`, so it can double as this
  limitation's own stable-ID input the same single way every other M2
  obstruction kind's `description` already does: there is no separate
  identity-only field a `description` can diverge from.
  `adapter_set_hash`'s `unavailable_kind` above and the resulting
  `cargo_metadata_unavailable` limitation's own stable ID are therefore
  both bound only to `kind` -- never to `diagnostic`, the staged
  `TempDir`'s path, or a raw subprocess `stderr` capture, and the
  redaction placeholder above exists solely so a human reading
  `diagnostic` in isolation (for example while debugging a real failure)
  never sees a raw local path, not to protect any canonical output, since
  `diagnostic` never reaches one. The result: two `ingest()` runs of the
  identical input that merely staged into two different `TempDir`s -- or
  whose raw diagnostics are deliberately, unredactably different for the
  same `kind` -- still produce the same `adapter_set_hash`, the same
  limitation ID, the same `Limitation` value, and byte-identical
  `canonical_output()`; two runs whose failures are genuinely different
  `kind`s never collide. This is a bugfix to M2's existing provenance
  behavior, not a shape change (the previous `unavailable_reason` field is
  renamed to `unavailable_kind` and now carries a fixed enum label instead
  of a raw string, but `adapter_set_hash` is not part of either versioned
  public schema, as already noted below), so no schema version bump or ADR
  was required for it.
- `tool_versions.syn` and `tool_versions.proc_macro2`: the exact `syn` and
  `proc-macro2` versions this crate was actually compiled against, each
  read from the workspace `Cargo.lock` by `build.rs` at compile time
  (never copied by hand into source, which could silently drift from the
  real pin) and exposed as a `rustc-env` compile-time constant.
  `proc-macro2` is bound for the same reason as `syn` itself: `syn`'s
  `Span`s -- and this crate's derived `Location`s and symbol IDs -- are
  backed directly by `proc_macro2::Span`, so a different `proc-macro2`
  resolution is a different span/location-computation implementation, not
  merely an unrelated transitive dependency.
- `git_command_policy`: the fixed values of the deterministic Git command
  policy described below (`version`, `diff_algorithm`,
  `rename_similarity_percent`, `rename_limit`, `force_text_diff`,
  `no_replace_objects`, `no_color`). Not a tool *version*, but exactly as
  load-bearing: every allow-listed `git` call is forced through this one
  fixed policy, so a future change to its shape must visibly change the
  fingerprint too, not be silently absorbed.
- `cargo_resolver_policy`: the fixed values of the deterministic Cargo tool
  admission policy described below (`version`, `automatic_path_resolution`,
  `rustup_invocation`, `admission`). Bound the same way, and for the same
  reason, as `git_command_policy`: this policy decides *whether and which*
  `cargo` binary a run actually executes, so a future change to its shape
  must visibly change the fingerprint too.

None of these six inputs depend on the repository's clone path, so
`adapter_set_hash` stays identical across clones of the same repository
at different filesystem locations, exactly like `repository_id`/
`snapshot_id` already do; two ingests of the same input on the same host
and toolchain therefore still hash identically. This is a bugfix to M2's
existing provenance behavior -- `adapter_set_hash` and its JSON shape
already existed and are not part of either versioned public schema (it is a
`ContentHash` field inside `ExtractionReport`/`ProgramSpace`, whose own
schema does not pin the hash's internal input shape, and the checked-in
`reviewgraphen.extraction_report.v1` example only illustrates the `hash`
format, never a value derived from a real `ingest()` run) -- not a new kind
of fact, so no schema version bump, ADR, or example/pinned-hash update was
required for it.

### Cargo tool admission (strict host admission)

An audit of the previous automatic `PATH`/rustup resolution established that
it could not be made safe from inside this crate: even the read-only runtime
probe used to identify and query a `cargo`/`rustup` pair (`rustup which
cargo`) is itself, on an unaudited host, capable of touching the toolchain
-- for example by resolving a `rust-toolchain.toml` override to a toolchain
`rustup` has not yet installed -- regardless of environment variables such as
`RUSTUP_AUTO_INSTALL` that only *reduce* that risk rather than eliminate it
structurally. No automatic resolution performed from inside the SUT (the
System Under Test this crate ingests) can therefore guarantee "never install,
never touch the toolchain." M2 replaces automatic resolution with **strict
host admission**: this module never searches `PATH`, never spawns
`rustup`/`mise`/`asdf`, and never installs or downloads anything. It runs
`cargo` at all only when a caller has explicitly admitted an absolute,
already-verified executable through `IngestConfig.cargo_admission`.

`IngestConfig.cargo_admission` is a public `CargoToolAdmission`:

```text
enum CargoToolAdmission {
    Disabled,                    // the default
    TrustedExecutable(PathBuf),  // caller-admitted absolute cargo path
}
```

- **`Disabled` (the default).** No Cargo executable is ever resolved or
  spawned. `cargo_version` is `Err(CargoToolFailure { kind: NotAdmitted,
  .. })` immediately, without staging or touching the host at all beyond
  what `git_snapshot`/`rust` already need. `extract_cargo_metadata`'s
  existing unconditional precondition check (below) means `cargo metadata`
  is never run and no package/dependency fact is ever accepted for that
  run; `cargo_metadata` is reported `missing` (`NotRun`) with a typed
  `cargo_metadata_unavailable` obstruction naming the cause, exactly as if
  Cargo were unavailable on the host outright.
- **`TrustedExecutable(path)`.** The caller/harness -- outside this crate's
  own trust boundary -- has already admitted `path` as a real,
  host-installed `cargo` executable. Admitting that path (verifying it is a
  genuine, safe-to-run Cargo binary before ever handing it to this crate)
  is the caller's/harness's responsibility, not something M2 attempts to
  re-derive or re-audit; M2's own contribution is only the bounded,
  deterministic *use* of that already-trusted path, never its discovery.
  `path` must be absolute; it is then canonicalized and checked to be an
  existing regular, executable file (`git::admit_cargo_executable`). A
  relative path, a path that fails to canonicalize, a directory, or a
  non-executable entry is a typed `CargoToolFailureKind::
  AdmittedExecutableInvalid` failure, never trusted as-is. The identical
  admitted, canonicalized path is then reused for both `cargo --version`
  (the fingerprinted version string) and, later, `cargo metadata` itself
  (`extract_cargo_metadata`) -- the same executable, from the same staged
  snapshot cwd, for both, exactly as the previous resolution logic already
  guaranteed once a binary was found.
- **Resolution failure fails closed through the existing typed-failure
  contract**, unchanged in shape from before this admission model existed:
  whether the failure is `Disabled`, an invalid admitted path, or a genuine
  spawn/timeout/output failure of an already-admitted executable,
  `GitSnapshot::cargo_version` becomes `Err(CargoToolFailure)`, and
  `extract_cargo_metadata`'s existing unconditional precondition check
  means `cargo metadata` is never run and no package/dependency fact is
  ever accepted for that run.
- **The admitted absolute executable path is host-specific and is never
  serialized into any identity/canonical output.** It can point anywhere
  the caller/harness chose to admit it from; that is not a fact about the
  target repository. `GitSnapshot::cargo_executable` exists purely to let
  `cargo_version` and `cargo metadata` reuse the identical binary and
  working directory, and is deliberately excluded from
  `ExtractionReport`/`ProgramSpace`/`adapter_set_hash` alike -- only the
  `cargo --version` *text* that binary reports (the existing
  `tool_versions.cargo` fingerprint input described above) and the fixed
  `cargo_resolver_policy` values are bound into identity, matching every
  other host-specific-path exclusion already documented for
  `cargo_executable`. `IngestConfig.cargo_admission`'s own path is
  similarly host-specific and is not folded into any adapter/canonical
  identity for the same reason.

**External harness responsibility and remaining trust limitation.** M2
itself cannot verify that an admitted `TrustedExecutable` path is a genuine,
unmodified `cargo` binary rather than an arbitrary executable that merely
answers `--version`/`metadata` plausibly -- that verification (for example
resolving it once, outside the SUT, via a trusted `PATH` lookup or a
pinned toolchain installation, and passing the resulting absolute path in)
is now explicitly the calling harness's responsibility, not this crate's.
This is a deliberate, narrower trust boundary than the previous
automatic-resolution design attempted: M2 no longer claims to discover a
trustworthy `cargo` on its own, only to use one it has already been handed,
bounded and deterministically, from a fixed working directory with a fixed
environment (`CARGO_HOME` inside the disposable staged snapshot,
`CARGO_NET_OFFLINE=true`, `--offline --no-deps`, no color, the same bounded
executor as every other allow-listed subprocess).

**Concrete external harness: mise, for this repository's own tests.**
`crates/reviewgraphen-ingest/tests/m2.rs` needs a concrete admitted `cargo`
to exercise the Cargo metadata adapter. `mise.toml` and
`scripts/resolve-trusted-cargo.sh`, entirely outside this crate, are that
harness (see
[ADR 0012](adr/0012-mise-host-admitted-cargo-toolchain.md) for the full
rationale); this crate never reads either of them.

```bash
mise trust      # once per clone: accept this repository's mise.toml
mise install    # once per clone: install rust@1.95.0 -- never automatic
mise run test-ingest
```

`mise run test-ingest` resolves the mise-installed `rust@1.95.0` toolchain's
real `cargo` via `mise exec rust@1.95.0 -- rustc --print sysroot` -- never
`mise which cargo`, which resolves to a rustup-style proxy that re-dispatches
based on whichever toolchain happens to be active, not the one fixed pinned
binary this harness must name -- canonicalizes and verifies it, exports the
result as `REVIEWGRAPHEN_TRUSTED_CARGO`, and runs `cargo test -p
reviewgraphen-ingest`. `tests/m2.rs`'s `trusted_test_cargo_admission` helper
requires `REVIEWGRAPHEN_TRUSTED_CARGO` and builds an ordinary
`CargoToolAdmission::TrustedExecutable(path)` from it -- the same public
value any other caller of this crate already constructs directly; nothing
about this call path is special-cased inside `reviewgraphen-ingest` itself.
There is deliberately no `CARGO`-environment-variable fallback: `cargo
test`'s own `$CARGO` names whatever binary is running the test process,
which can itself be a rustup/mise multiplexer shim rather than one fixed,
independently verified toolchain, so it is not treated as an admission
boundary. `mise run test-ingest` is therefore the official entry point for
any test that needs real Cargo metadata. Most of `tests/m2.rs`'s tests do
not: `TempGitRepository::request()` defaults to `CargoToolAdmission::
Disabled` and runs under a plain `cargo test -p reviewgraphen-ingest`
without mise at all; only tests whose own assertions depend on a Cargo
metadata result (a `package` artifact, a `depends_on` relation, or a
`cargo_metadata` capability other than `missing`) call the separate
`request_with_trusted_cargo()` helper, which requires
`REVIEWGRAPHEN_TRUSTED_CARGO`. `scripts/ci.sh fast` carries the same
admission into its workspace test run: local fast runs resolve it through
`mise run trusted-cargo-path` when the variable is absent, while CI exports a
host-resolved absolute path after explicit toolchain setup. Both are external
to `reviewgraphen-ingest`; neither creates a runtime discovery path. Every
mise-mediated auto-install path is
disabled (`mise.toml`'s `[settings]` -- `auto_install`, `exec_auto_install`,
`not_found_auto_install`, and `task.run_auto_install`, all `false` -- and the
resolver script's own exported
`MISE_*_AUTO_INSTALL=false`/`MISE_OFFLINE=true`), so neither `mise run
trusted-cargo-path` nor `mise run test-ingest` ever installs the toolchain
themselves -- both fail with an explicit error directing the caller back to
`mise install` when rust@1.95.0 is not already present.

**`Disabled` never touches the staged-snapshot filesystem at all.** Staging
a private on-disk copy of the accepted snapshot exists purely so a real
admitted `cargo` has a working directory to run from; when
`IngestConfig.cargo_admission` is `Disabled`, `load_snapshot` skips that
staging step entirely and resolves directly to the same `NotAdmitted`
`CargoToolFailure` `resolve_cargo` would otherwise have produced -- no
temporary directory is created and no file is written, keeping `Disabled`
free of host contact beyond what the git/Rust ingestion this request
actually asked for already needs.

**Every allow-listed `cargo` subprocess shares one command builder with a
cleared environment.** `cargo --version` and `cargo metadata` alike are
built by `cargo_command(executable, staged_root)`, which -- exactly like
`git_command` on the Git side -- calls `Command::env_clear()` before
setting only `CARGO_HOME` (inside the staged snapshot), `CARGO_NET_OFFLINE
=true`, `CARGO_TERM_COLOR=never`, and `LC_ALL=C`. No other variable,
including `PATH`, is ever propagated: the admitted executable is always
invoked by its already-canonicalized absolute path (never looked up on
`PATH`), so ambient shell state -- `PATH`, `RUSTUP_TOOLCHAIN`, any `MISE_*`
variable, `RUSTC`, `RUSTFLAGS`, `CARGO_TARGET_DIR`, or any other ambient
`CARGO_*` -- can never reach the child and silently select a different
toolchain, target directory, or registry for an unchanged admitted
executable and staged snapshot. `cargo_resolver_policy`'s `version` is `3`
for this reason (`2` was strict host admission itself; `3` is this cleared,
fully-enumerated child environment), and the fixed variable set is bound
directly into `cargo_resolver_policy_fingerprint()`, so a future change to
it changes `adapter_set_hash` too, exactly like a different tool version
would.

### Deterministic Git command policy

Every allow-listed `git` invocation is built by one shared function
(`git_command`) instead of each call site constructing its own `Command`.
This closes a gap where the *same* snapshot tuple, tool versions, and
`adapter_set_hash` could still produce *different* `changed_structure`/
`changed_lines` facts depending on the cloned repository's own Git
configuration (or the executing host's system/global configuration),
because that configuration was previously inherited rather than fixed:

- **Environment is stripped to an explicit allowlist.** The child's
  environment is cleared (`Command::env_clear()`) and only `PATH` is
  propagated back in, so ambient shell state -- locale, `GIT_CONFIG_*`,
  `GIT_EXTERNAL_DIFF`, credential helpers, and anything else in the calling
  process's environment -- can never reach the child. `LC_ALL=C` is set
  explicitly for stable, non-localized diagnostic text.
- **System and global Git config are disabled outright**, via
  `GIT_CONFIG_SYSTEM=/dev/null` and `GIT_CONFIG_GLOBAL=/dev/null`: neither
  `/etc/gitconfig` nor `$HOME/.gitconfig`/`$XDG_CONFIG_HOME/git/config` is
  ever read, regardless of the executing host or user. Only the
  repository's own `.git/config` -- part of the snapshot's own tree of
  concerns, not ambient environment -- can still apply, and even that is
  overridden per command wherever it could change which facts are
  extracted (below).
- **A per-user default `.gitattributes` file is also disabled.**
  `GIT_CONFIG_GLOBAL=/dev/null` alone only stops Git from reading a
  *config* file that might point `core.attributesFile` elsewhere; Git
  separately falls back to a hardcoded per-user attributes file
  (`$XDG_CONFIG_HOME/git/attributes` or `$HOME/.config/git/attributes`)
  unconditionally, not only when a config file sets it. `-c
  core.attributesFile=/dev/null` closes that gap.
- **Binary/text classification can no longer affect `changed_lines` at
  all.** A repository's own *tracked* `.gitattributes` is real snapshot
  content, not ambient environment -- but Git also consults an
  *untracked*, clone-local `$GIT_DIR/info/attributes`, which can declare a
  path (for example `*.rs binary`) as binary independently of the commit
  history two clones of the exact same revisions otherwise share. Without
  a fix, a path Git believes is binary gets "Binary files ... differ" with
  *no* `@@` hunks at all from `git diff --unified=0`, silently emptying
  the `changed_lines` fact for a file that genuinely changed -- so this
  was not, in fact, already snapshot-deterministic. `--text` on every
  `GitCommand::Changes`/`GitCommand::ChangedLines` invocation forces every
  path to be diffed as text unconditionally, regardless of any binary
  classification from either a tracked or an untracked `.gitattributes`.
- **Diff output can no longer carry ANSI color codes.** A repository's own
  `.git/config` can set `color.ui=always`/`color.diff=always` (or the
  executing host's ambient config could, before the environment was
  stripped above) to force-colorize `git diff` output even though this
  module never runs interactively. Without a fix, `changed_lines`'s
  `line.strip_prefix("@@ ")` parse of `GitCommand::ChangedLines`'s hunk
  headers would silently stop matching once those headers are ANSI-wrapped
  (`\x1b[36m@@ ...`), emptying the fact for a file that genuinely changed.
  `--no-color` on both `git diff` invocations (`GitCommand::Changes`,
  `GitCommand::ChangedLines`) forces plain-text output unconditionally,
  overriding any repo-local `color.ui`/`color.diff` config.
- **No external diff, no textconv, no optional locks, no replace
  objects.** `--no-ext-diff` and `--no-textconv` are passed on every `git
  diff`/`git show` invocation (`GitCommand::Changes`,
  `GitCommand::ChangedLines`, `GitCommand::ShowFile`), so a
  repository-declared `diff.external` or a `diff=<driver>`/`textconv`
  attribute/config combination is never invoked, regardless of what the
  repository's own config or `.gitattributes` declare.
  `--no-optional-locks` (equivalent to `GIT_OPTIONAL_LOCKS=0`) is passed
  on every invocation. `--no-replace-objects` (equivalent to
  `GIT_NO_REPLACE_OBJECTS=1`) is likewise passed on every invocation: a
  `refs/replace/*` ref is itself clone-local and untracked, and
  transparently substitutes a different object wherever the original is
  read (`rev-parse`, `ls-tree`, `show`, `diff` alike), so without this a
  clone carrying a replacement for the requested target revision could
  silently ingest the replacement's content instead of the real target's.
- **Diff algorithm and rename/copy detection are fixed, not configured.**
  `GitCommand::ChangedLines` (the invocation that generates a textual
  patch, used to derive the `changed_lines` fact) always passes
  `--diff-algorithm=myers`, overriding any repo `diff.algorithm`, so hunk
  boundaries -- and therefore `changed_lines` -- cannot vary with the
  clone's config. `GitCommand::Changes` always passes
  `--find-renames=50%`/`--find-copies=50%` (Git's own historical
  similarity default, now pinned rather than left to `diff.renames`) and a
  fixed `-l20000` rename/copy detection limit, overriding any repo
  `diff.renameLimit` -- a repo-configured low limit can otherwise silently
  downgrade a real rename/copy into an unrelated `Added`+`Deleted` pair
  once a changeset is large enough to hit it.
- `--no-pager` (unchanged from before this policy existed) already fully
  disables paging non-interactively regardless of `core.pager`/`$PAGER`.

The command-line `-c`/flag overrides above always take precedence over
whatever the repository's own `.git/config`/`.git/info/attributes` sets
for the same key, per Git's own precedence rules; the adversarial-config
regression tests exercise that precedence directly, since the base fixture
repository does not otherwise set any of the overridden keys:

- Two clones of the same content, given different repository configs
  (including one declaring an external diff helper that would leave a
  detectable side effect if it were ever actually invoked, and both forcing
  `color.ui=always`/`color.diff=always`), produce identical accepted fact
  IDs, `snapshot_id`, and `adapter_set_hash`, the declared external diff
  helper is never invoked, and -- checked directly, not only via ID
  equality -- both clones still carry the same non-empty `changed_by` edge
  for the function whose file actually changed, proving the forced color
  config did not silently empty out `changed_lines`
  (`divergent_repo_config_does_not_change_accepted_facts_or_hashes`).
- A clone with a clone-local `*.rs binary` override in
  `.git/info/attributes` still produces the same accepted fact IDs,
  `adapter_set_hash`, and -- checked directly, not only via ID equality --
  the same `changed_by` edge for the function whose file actually changed
  (`clone_local_binary_attribute_override_does_not_change_changed_lines_or_hashes`).
- A clone carrying a `refs/replace/*` ref that would substitute a decoy
  commit's content for the requested target revision still produces the
  same accepted fact IDs, `snapshot_id`, and `adapter_set_hash` as a clone
  without it, and the decoy's own content never appears as an accepted fact
  (`a_replacement_ref_present_in_only_one_clone_does_not_change_accepted_facts_or_hashes`).

Each of these three end-to-end tests was verified to actually fail without
its corresponding fix (`--text`, `--no-replace-objects`, `--no-color`), not
merely constructed to pass -- confirmed by temporarily reverting each flag
and re-running the affected test during development.

## Accepted facts and traces

Every accepted artifact and relation has Git provenance with the resolved
revision, relative source path where available, content/tree hash, extractor
method and tool version. The adapter emits the following bounded facts:

| Fact family | M2 behavior |
| --- | --- |
| file/module/function/method/type/test/package/state | accepted from the snapshot, parser, Cargo metadata, or syntactic assignment target |
| containment | file → module and module → declared symbols |
| direct calls | only a unique syntactic local target; unresolved calls remain unknown |
| imports/module dependencies | syntactic `use` targets, explicitly marked `syntactic_only` |
| test mapping | `covers` only for a test's resolved direct calls |
| state writes | direct path/field/index assignment targets only |
| changed structure | Git name-status entries plus a reciprocal `changed_by`/`contains` relation pair between every touched record and a `change:*` artifact carrying the changed hunk lines and `changed: true` |
| concurrency model | per function/method: declared `async`, syntactic `.await` points (also emitted as `awaits` self-edges), spawn/channel call paths, and shared-state type-name occurrences |

`changed_by` describes source-tree change, not semantic equivalence or safety.
Cargo package/dependency facts likewise describe metadata, not a build or a
verification result.

All base-comparison-specific data -- whether a target path was added,
modified, deleted, renamed, copied, or type-changed, which target lines
changed, and the `changed: true` flag itself -- lives exclusively on the
`change:*` artifact and the relations tying it to the records it changed. A
file, module, function, method, type, test, or state record gets a
`changed_by` relation to a `change:*` artifact when its location overlaps
that change's changed lines (files always link to their own change entry,
regardless of location); it never carries a `changed` or `changed_lines`
attribute itself. Every non-file record that gets a `changed_by` edge also
gets the reciprocal `contains` edge from the same `change:*` artifact, so a
consumer walking containment down from a change reaches the individual
symbols it touched -- this is the path by which `changed` reaches an
individual function at all, and it is why obligation synthesis never needs a
base-relative attribute on the function itself. This is what makes the
ID-stability promise above actually hold at the level ADR 0011 cares about:
two ingests of the same target revision with different `base_revision`
values produce byte-identical `Artifact`/`Relation` records (attributes
included, not only IDs) for everything except the `change:*` artifacts and
the change family's own edges -- identified by their
`reviewgraphen.ingest.git.changed_structure.v1` extraction method, not by
relation kind, since `contains` is otherwise an ordinary base-invariant Rust
containment kind. This is a bugfix to M2's existing output, not a shape
change -- `change:*`, `changed_by`, and their attribute names already
existed -- so no schema version bump or ADR was required for it.

Every accepted `package` and `depends_on` edge is bound to exactly one of two
provable states, never a guess: a `depends_on` target is either an existing
accepted `package` artifact from this same run's `packages[]` (a
workspace-internal/path dependency, resolved by Cargo's own `source: null`
signal plus an exact name -- and, when Cargo reports it, `path` -- match
against exactly one accepted package), or a fabricated `external-package:*`
stub explicitly attributed `external: true` (a registry/git dependency,
identified by Cargo's own non-null `source`). A local dependency that does
not resolve to exactly one accepted package -- an ambiguous same-named
match, or a path dependency to a crate `cargo metadata --no-deps` never
turned into a `packages[]` entry (for example a non-workspace-member local
crate) -- creates neither kind of edge: it is retained only as a
`relation_unresolved` obstruction related to `cargo_metadata`, which is then
declared `partial` (not `complete`) for that run, consistent with the same
non-empty-source-trace/related-capability contract every other M2
obstruction already follows. This is a bugfix to M2's existing
`reviewgraphen.extraction_report.v1`/`reviewgraphen.program_space.input.v2`
behavior, not a shape change: no new artifact kind, relation kind,
capability name, or obstruction kind was added, so no schema version bump
or ADR was required for it.

## Completeness and unknowns

`ExtractionReport` is a new versioned public output. Its capability map and
typed `IngestionObstruction` records are part of the result, not log-only
diagnostics. The matching limitations and per-capability source traces are
also embedded in `ProgramSpace`'s existing `extraction` declaration, per ADR
0011's v2 capability-trace contract: every `extraction.capabilities` entry
carries a non-empty, resolvable `source_ids` set alongside its state, and
every non-`complete` capability has at least one related, source-backed
`Limitation`.

"Resolvable" is enforced the same way for every draft-level source-key
reference, not only for a relation's `source_key`/`target_keys`: an
`IssueDraft`'s `source_keys` (which become one `Limitation`'s `source_ids`)
and one named capability's own declared source keys are each either empty
-- the deliberate "grounded in the whole snapshot" case, for example a
symlink excluded before any artifact existed for it, which falls back to
`snapshot_id` -- or every key in the set must resolve to an artifact this
same run actually drafted. A non-empty set that names even one key nothing
drafted is always a hard `IngestError::AdapterOutput`, the same typed error
a relation with an unknown source/target key already produced; it is never
silently narrowed to just the keys that happened to resolve, since that
would let a broken adapter still produce an apparently valid, non-empty
source trace. This is a bugfix to M2's existing resolution behavior -- the
error variant, the fallback rule, and the "resolvable" requirement above all
already existed -- not a shape change, so no schema version bump or ADR was
required for it.

- Rust parse failure: retain the file fact, emit `parse_failure`, and mark
  the affected snapshot's `ast`/`containment`/`concurrency_model`
  capabilities `partial`.
- Macro invocation: emit `macro_expansion_unresolved`; M2 does not expand it.
- Pattern-position macro invocation (`Pat::Macro`, for example `let
  mymac!(target) = ..;`): emit `macro_expansion_unresolved`, grounded to the
  enclosing function's source, even when nothing else in that function's
  body is otherwise unresolved.
- Any other opaque/unrecognized pattern shape (`Pat::Verbatim` -- the shape
  `syn` produces for tokens it cannot parse into a known pattern, for
  example `let box target = ..;` -- or a future `syn::Pat` variant this
  adapter has no case for; `syn::Pat` is `#[non_exhaustive]`): emit
  `unknown`, grounded the same way.
- Method/trait/dynamic dispatch: emit `dynamic_dispatch_unresolved`.
- Non-unique/non-path call: emit `relation_unresolved`.
- Unsupported file region, symlink, or Cargo safety precondition: retain a
  typed limitation and lower the relevant capability.
- Cargo metadata unavailable (no manifest, unsafe path dependency, or a
  staging/`cargo metadata` failure): emit `capability_missing` and mark
  `cargo_metadata` `missing`.

`concurrency_model` is declared exactly like `ast` and `containment`, and
for the same reason: its facts -- declared `async`, syntactic `.await`
points, spawn/channel call paths, and shared-state type-name occurrences --
are read straight off the same unexpanded syntax tree with no resolution
step of their own, so a parse failure is the only observed condition that
leaves any of them unread. It is deliberately *not* a semantic concurrency
model: it never claims which runtime a `spawn` belongs to, that a named
`Mutex` is any particular crate's type, or that two invocations can actually
interleave. Like every other fact read off that tree, it is bounded to the
unexpanded source; a construct a macro would have generated is reported
through `macro_expansion_unresolved`, exactly as it is for `ast`, rather
than by weakening a syntax-scoped capability that is complete with respect
to the tree the adapter actually has.

No such condition creates an `issue_absent`, `verified`, or accepted review
claim. The bounded direct-call, import, module-dependency, test-mapping, and
state-write capabilities are intentionally `partial` even if a small
repository happens to exercise only resolved examples; M2 satisfies ADR
0011's required-correspondence contract for that permanent `partial` state by
always emitting one deterministic, snapshot/file-grounded limitation for
those five capabilities, in addition to any limitation tied to a specific
unresolved call, import, or write.

## Versioning and migration

M2 originally produced the frozen `reviewgraphen.program_space.input.v2`
(ADR 0011). The M6-capable adapter now produces the additive
`reviewgraphen.program_space.input.v3` carrier defined by ADR 0023 together
with the unchanged `reviewgraphen.extraction_report.v1`. `ExtractionReport` and the
`ProgramSpace` it accompanies are both built directly from the same typed
adapter intermediate (`ArtifactDraft`/`RelationDraft`/`IssueDraft`); there is
no intermediate v1 JSON document and no implicit or hidden migration step.
`reviewgraphen-core`'s explicit `migrate_program_space_v1_to_v2` API exists
only for pre-existing v1 producers/fixtures, not for M2 itself.

V3 adds only accepted deterministic mapping inputs. The bounded Git adapter
records the resolved 40-character lowercase base and target commit OIDs and
both exact `git:<40 lowercase hex>` tree hashes. The Rust adapter emits one
`reviewgraphen.rust_symbol_anchor@1` for every accepted Rust
function/method/type by hashing parsed token forms that remove only spans,
comments, and formatting. Its producer is fixed to
`reviewgraphen.ingest.rust_syn.anchor.v1` and syn `2.0.119`; changing that
normalization requires a new contract version. Relation drafts retain their
extractor-declared endpoint sequence, and relation identity commits that
sequence separately from the set-valued reference domain. Core rejects a
missing anchor, mutated producer/version, non-permutation endpoint order, or
Git closure inconsistent with the accepted clean snapshot. V1/v2 decoding and
canonical historical bytes remain unchanged.

A future incompatible change to either output requires a new schema/API
version and an explicit migration or major-version decision; it must not
reinterpret a prior limitation as a clean result.

ADR 0008 already defines the required language-neutral-core/profile-specific
extractor boundary, and ADR 0011 already defines the v2 capability-trace
contract M2 implements here, so M2 requires this concrete contract rather
than a new ADR.
