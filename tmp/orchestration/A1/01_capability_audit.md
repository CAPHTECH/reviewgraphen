# ReviewGraphen capability audit

Audit date: 2026-08-23 (Asia/Tokyo)  
Audited revision: `1a2fefb10c1bffb088f8377e102d391086e094a9`  
Method: read-only source/contract review plus the bounded commands in §4. The
pre-existing untracked `benchmarks/m17-*`, `m18-*`, `m19-*`, and unrelated
`tmp/` content were treated as user state and were not changed.

## 1. Practical-state checklist

The status describes the **ordinary Git generic path**, not what can be made to
work only through the double-submit fixture or a benchmark-only consumer.

| Required practical capability | Status | Code-grounded finding |
| --- | --- | --- |
| Local Rust repo + base/head → deterministic ingest → versioned obligation universe | **Implemented, narrow** | The CLI accepts only `review --request … --artifacts …` and schema operations (`crates/reviewgraphen-cli/src/lib.rs:46-89`). The request binds workspace/repository/base/target, ingest bounds, plan, and reviewer (`crates/reviewgraphen-runtime/src/generic.rs:64-124`). The runtime calls retained-source ingest, `MvpRulePack::synthesize`, aggregate creation, deterministic planning, and source registration in that order (`crates/reviewgraphen-runtime/src/generic.rs:594-632`). Universe identity includes snapshot/profile/rule/extractor/policy metadata in the Core contract (`crates/reviewgraphen-core/src/synthesize.rs:41-65`). This was rechecked with the ordinary-Git runtime replay test (§4). The qualifier is material: request v1 hard-wires `MvpRulePack` (`crates/reviewgraphen-runtime/src/generic.rs:599-603`). |
| A non-fixture Node obligation from real Rust | **Implemented, one rule** | Real ingest propagates changed structure and the rule selects a changed public function with local syntactic concurrency evidence (`crates/reviewgraphen-core/src/synthesize.rs:575-615`). The real ingest→synthesize test asserts a non-capability-gap `async.concurrent_reentry` obligation at `applicable` (`crates/reviewgraphen-ingest/tests/m2.rs:2825-2879`); it passed in §4. |
| A non-fixture Relation, Path, or Invariant obligation from real Rust | **Unimplemented** | The relation rules require `handled_by/concurrency=unbounded_reentry` or payment attributes on a `calls` edge (`crates/reviewgraphen-core/src/synthesize.rs:617-688`). Path/invariant generation is downstream of those same fixture-shaped relations (`crates/reviewgraphen-core/src/synthesize.rs:690-825`). Real Rust ingest never produces those attributes. The canonical status table reaches the same conclusion (`docs/23_current_capability_status.md:130-149`). |
| Bounded per-obligation projection with source IDs, included/excluded/unknown/loss/hash | **Implemented contract; practically partial** | `ReviewContextEnvelope` carries snapshot/policy IDs, candidate and included source IDs, exclusions, unknowns, assumptions, information-loss records, and projection hash (`crates/reviewgraphen-core/src/context.rs:370-387`). `prepare_context` is obligation-seeded and source-closure bound (`crates/reviewgraphen-core/src/context.rs:1852-1906`). However, excerpt construction emits one contiguous range beginning at the lowest anchor and truncates forward at 400 lines (`crates/reviewgraphen-core/src/context.rs:3179-3252`); the documented real probe showed that this can omit the subject even with a perfect seed (`docs/24_context_projection_feasibility_for_implementation.md:61-98`). The prior `state:*` hard failure is fixed, but multi-window and an external symbol-seed API remain absent (`docs/24_context_projection_feasibility_for_implementation.md:118-140,176-187`). |
| Reviewer returns a structured claim or abstention; prose does not become canonical state | **Implemented contract; live provider not re-run here** | Output DTOs deny unknown fields and close the outcomes to structured, abstained, malformed, or provider failure (`crates/reviewgraphen-reviewer/src/lib.rs:922-949,1097-1169`). Parsing requires canonical JSON, caller-fixed execution/property/target/source closure, and records malformed output rather than repairing it (`crates/reviewgraphen-reviewer/src/lib.rs:2283-2411,2422-2539`). The generic runtime converts only parsed proposals and fixes them to `proposed`, `ai`, `unreviewed` (`crates/reviewgraphen-runtime/src/generic.rs:188-218`). Codex/Claude execution is isolated and produces a non-authority process record (`crates/reviewgraphen-reviewer/src/process.rs:441-512,528-559`). I did not invoke a model/provider. |
| Claim / Evidence / Verification / human decision remain distinct; unsupported/stale/inconclusive survive | **Partially implemented, not connected to generic review** | Generic claims remain proposal DTOs and the authority ceiling is always non-authority/incomplete with `evidence_not_executed` and `human_decision_not_recorded` (`crates/reviewgraphen-runtime/src/generic.rs:1243-1266`). The source-bound report family and fixed M4/M5 paths have separate evidence/verification/decision concepts, but generic runtime returns only ProgramSpace, extraction, obligations, plan, contexts, executions, coverage, and authority (`crates/reviewgraphen-runtime/src/generic.rs:126-139,707-718`). Thus separation is preserved by omission, but an ordinary Git claim cannot yet acquire typed unsupported/inconclusive verification, freshness, or a human decision in the same workflow. |
| Allow-listed, workspace-scoped verification; no arbitrary reviewer shell | **Reviewer sandbox implemented; generic verification unimplemented** | Reviewer tools are disabled and only the admitted packet is read-only mounted; the repository itself is not mounted (`crates/reviewgraphen-reviewer/src/process.rs:584-628,657-726`). The verifier registry is closed to two process-free M4 descriptors and every network/process/workspace-write flag is false (`crates/reviewgraphen-verifier/src/lib.rs:27-98,121-145`). Its only test witness is the compiled double-submit fixture (`crates/reviewgraphen-verifier/src/lib.rs:246-300`). ADR 0021 explicitly defers a real repository sandbox with fixed argv, workspace cwd, resource limits, and no wildcard descriptor (`docs/adr/0021-m4-evidence-bound-verification.md:372-382,1260-1266`). |
| One CLI workflow produces a short human PR report plus auditable JSON | **Partial** | Generic review writes canonical audit JSON to stdout and packet/process/replay artifacts (`crates/reviewgraphen-cli/src/lib.rs:77-89`; `crates/reviewgraphen-runtime/src/generic.rs:633-718`). It does not call the report crate or emit Markdown. The existing report API consumes Store/journal/index/CAS authority roots (`crates/reviewgraphen-report/src/lib.rs:116-184`) and is not wired to `GenericReviewRun`; the report crate also marks the public V5 seam fail-closed pending a Store continuation (`crates/reviewgraphen-report/src/lib.rs:8-15`). The implemented CLI contract explicitly excludes Evidence, Verification, Decision, Finding, V5 report, and gate (`docs/13_cli_contract.md:68-82`). |
| Provider-free deterministic/fake quickstart plus real-model adapter | **Partial** | Codex CLI and Claude CLI are real request modes; app-server is a fail-closed placeholder and replay consumes prior records (`crates/reviewgraphen-runtime/src/generic.rs:98-124,333-375`). A deterministic `FakeReviewer` exists, but it is keyed to predeclared fixture keys (`crates/reviewgraphen-reviewer/src/lib.rs:2645-2714`) and is not a generic request mode. Request schema v1 admits only Codex, Claude, app-server, or replay (`schemas/reviewgraphen.generic_review_request.v1.schema.json:60-66,72-121`). Therefore a fresh clone cannot run the ordinary CLI without credentials or already-existing records. |
| Clone-after-docs reproducibility | **Unimplemented for the full practical flow** | No checked-in ordinary generic request/quickstart was found (`rg --files schemas examples docs | rg 'generic|review-request|quick'` found only the schemas and ADR). The accepted command needs a fresh absolute artifact path and trusted backend/credentials or exact replay records. `schema list` works after build, but there is no provider-free command that generates the advertised review JSON/report from a new repository (§4). |

## 2. Deterministic and authority boundaries that are already sound

- Rust parsing is `syn` over immutable Git bytes; parse failure downgrades the
  relevant syntax capabilities (`crates/reviewgraphen-ingest/src/rust.rs:19-54`).
- `ast`, `containment`, and syntax-scoped `concurrency_model` are complete only
  when all admitted Rust files parse. Name-resolution-dependent capabilities
  remain unconditionally partial (`crates/reviewgraphen-ingest/src/rust.rs:116-193`).
- Direct `calls` edges are admitted only for one conservative local syntactic
  target; shadowing, glob imports, ambiguous/unresolved paths are not guessed
  (`crates/reviewgraphen-ingest/src/rust.rs:1609-1768`). Method calls and macros
  remain explicit unresolved obstructions (`crates/reviewgraphen-ingest/src/rust.rs:1941-1969`).
- Generic coverage derives its denominator from the universe and requires the
  planned/deferred and outcome partitions to close exactly
  (`crates/reviewgraphen-runtime/src/generic.rs:1180-1240`). Serialized semantic
  validation re-derives those equalities (`crates/reviewgraphen-runtime/src/generic.rs:378-427`).
- A successful model or replay cannot change `trusted_pass=false`
  (`crates/reviewgraphen-runtime/src/generic.rs:1243-1266`).

## 3. Recheck of `docs/23_current_capability_status.md`

### Reconfirmed in current code/tests

1. **Exactly one real-ingest substantive rule is reachable.** Current rule
   selection is at `synthesize.rs:575-615`, and the focused real-ingest test
   passed. This reconfirms the substance of `docs/23:19-31,104-111` at the
   audited revision.
2. **Concurrency facts are syntax-scoped, not semantic resolution.** The code
   explicitly limits the capability to unexpanded syntax and disclaims runtime,
   type, interleaving, and macro conclusions
   (`crates/reviewgraphen-ingest/src/rust.rs:913-973`). The focused syntax test
   is `crates/reviewgraphen-ingest/tests/m2.rs:2740-2822`.
3. **Rules 2–5 remain fixture-specific.** Their current trigger code is cited in
   §1 and still matches `docs/23:130-149,168-174`.
4. **The five resolver-dependent ingest capabilities remain partial.** Current
   declarations and source-backed limitation are
   `crates/reviewgraphen-ingest/src/rust.rs:159-193`, matching
   `docs/23:54-70`.
5. **Reproduction verdict is not human-visible.** The current generic run and
   CLI do not project such a field; `docs/23:210-219` remains accurate for the
   audited surfaces.

### Not independently reconfirmed

- The historical axum counts (1 substantive + 4 gaps after narrowing) in
  `docs/23:153-166` were not rerun; the document itself labels them not
  independently reproduced. The only axum evidence read here is the later
  recorded projection probe (`docs/24:176-184`).
- I did not rerun the historical full counts `core 422`, `store 202`,
  `reviewer/runtime/report 31/32/38`, or their fault-injection timing from
  `docs/23:112-124`. Focused current tests are listed below; historical counts
  must not be quoted as newly reproduced.
- I did not run Codex/Claude, app-server, or any benchmark/model experiment.
  Process-adapter claims are code/test inspection only.

## 4. Read-only command evidence

Executed from repository root. Cargo writes only normal build artifacts under
the pre-existing ignored `target/`; no product/source/schema/benchmark file was
changed.

| Command | Result |
| --- | --- |
| `cargo run --quiet -p reviewgraphen-cli -- schema list` | Exit 0. Returned the ten closed schema IDs, including generic request/run and report v1–v5. |
| `cargo run --quiet -p reviewgraphen-cli -- --help` | Exit 2. Printed only `review --request … --artifacts … | schema list|print|validate`; there is no quickstart/fake/report command. |
| `cargo test -p reviewgraphen-ingest --test m2 a_changed_public_async_function_synthesizes_a_substantive_obligation -- --exact` | 1 passed. |
| `cargo test -p reviewgraphen-runtime generic::tests::ordinary_git_input_replays_to_identical_non_authority_bytes -- --exact --nocapture` | 1 passed. The test builds an ordinary temporary Git base/head, uses an internal structured driver, validates schema/semantics/non-authority, and proves byte-identical replay (`crates/reviewgraphen-runtime/src/generic.rs:1445-1556`). It is not a public fake CLI. |
| `cargo test -p reviewgraphen-cli` | 3 passed; includes rejection of fixture and detached gate forms (`crates/reviewgraphen-cli/src/lib.rs:302-344`). |
| `cargo test -p reviewgraphen-verifier` | 4 passed; confirms the closed process-free two-descriptor registry. |
| `cargo test -p reviewgraphen-report --test report_v5_schema` | 8 passed; schema/semantic mutation checks only, not generic report integration. |
| `cargo fmt --all -- --check` | Exit 0. |
| `cargo clippy --workspace --all-targets -- -D warnings` | Exit 0. |
| `python3 scripts/validate_bundle.py` | PASS: JSON/TOML/Markdown/link/schema/semantic bundle checks. |
| `mise run bundle:check` | Not a test result: exit 1 because this repository defines no such mise task. Available tasks were `test-ingest` and `trusted-cargo-path`; no fallback claim is made. |

## 5. Audit conclusion

ReviewGraphen has a genuine, narrow generic **proposal pipeline**, not a merely
hand-authored fixture: ordinary Git ingestion, one real Node obligation,
source-bound context, isolated structured observation, strict parsing, replay,
denominator-preserving coverage, and a permanent non-authority ceiling are
implemented. It is not yet the user-defined practical product. The decisive
missing chain is a general real-code Relation/Invariant rule, target-preserving
projection, generic allow-listed verification, human-readable report, and a
fresh provider-free CLI example. Existing authority-heavy M4–M6/report machinery
does not close that chain because it remains fixture/Store-specific and is not
called by generic review.
