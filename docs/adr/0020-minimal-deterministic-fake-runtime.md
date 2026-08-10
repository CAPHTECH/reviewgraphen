# ADR 0020: Minimal Deterministic Fake Runtime

- Status: Accepted
- Date: 2026-08-10
- Scope: implements only ADR 0018 §7's fake D2 execution path on ADR 0019's
  admission-bound replayed V2 session. No provider, network, process, tools,
  CLI, index, report, verification, or acceptance authority is added.

## Decision

`reviewgraphen-runtime` is a deterministic orchestration crate depending on
core, store, and reviewer. Its caller opens a `ReplayedV2RunSession` with
exact admissions; runtime never constructs an unchecked envelope or obtains a
mutable log.

For one plan/wave/envelope/obligation/attempt, runtime resolves every included
source from CAS and checks the retained registration, hash, size, line/source
closure, and reviewer request. It derives the execution ID before one fake
invocation, strictly parses the response, puts raw bytes in CAS, records a
sensitive reviewer-execution artifact, records one atomic validated execution,
and transitions only structured outcomes to `Completed`. All other outcomes
remain `InProgress`.

Durable state is the resume authority. Runtime resumes at the first missing
step and never invokes the same attempt after its raw registration or execution
exists. An unregistered raw CAS object is a typed orphan/incomplete condition,
not an invitation to guess a registration. Post-append session uncertainty is
returned to the caller; reopening/recovery is required. Crash seams are after
CAS publication, raw registration, execution record, and before completion.

## Consequences

The fake is deterministic and no-tools only. It produces review claims, never
program facts, evidence, verification, decision, or sign-off. A real provider
or broader scheduling/execution surface needs a later ADR and versioned
contract.
